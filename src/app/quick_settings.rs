//! Native macOS status item and stateful quick-settings popover.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickSettingsSnapshot {
    pub zoom: f32,
    pub ui_zoom: f32,
    pub focus_mode: bool,
    pub shows_sidebar: bool,
    pub shows_inspector: bool,
    pub sidebar_available: bool,
    pub inspector_available: bool,
    pub dark: bool,
    pub word_wrap: bool,
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    use async_channel::Sender;
    use gpui::App;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSBezelStyle, NSBox, NSBoxType, NSButton, NSControlSize, NSControlStateValueOff,
        NSControlStateValueOn, NSFont, NSImage, NSPopover, NSPopoverBehavior,
        NSSquareStatusItemLength, NSStatusBar, NSStatusItem, NSSwitch, NSTextAlignment,
        NSTextField, NSView, NSViewController, NSWindowCollectionBehavior,
    };
    use objc2_foundation::{NSObject, NSPoint, NSRect, NSRectEdge, NSSize, NSString};

    use super::QuickSettingsSnapshot;
    use crate::app::menu::Quit;
    use crate::app::shell::{
        ResetUiZoom, ResetZoom, ToggleAppearance, ToggleFocusMode, ToggleInspector, ToggleSidebar,
        ToggleWrap, UiZoomIn, UiZoomOut, ZoomIn, ZoomOut,
    };

    const PANEL_WIDTH: f64 = 300.0;
    const PANEL_HEIGHT: f64 = 454.0;
    const CONTENT_X: f64 = 16.0;
    const CONTENT_WIDTH: f64 = PANEL_WIDTH - CONTENT_X * 2.0;

    static ACTION_SENDER: OnceLock<Sender<QuickAction>> = OnceLock::new();

    thread_local! {
        static CONTROLLER: RefCell<Option<Retained<QuickSettingsTarget>>> = const {
            RefCell::new(None)
        };
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum QuickAction {
        ZoomOut,
        ZoomIn,
        ToggleFocusMode,
        ToggleSidebar,
        ToggleInspector,
        ToggleWrap,
        ToggleAppearance,
        ResetZoom,
        Quit,
        UiZoomOut,
        UiZoomIn,
    }

    struct QuickSettingsControls {
        zoom_value: Retained<NSTextField>,
        zoom_out: Retained<NSButton>,
        zoom_in: Retained<NSButton>,
        ui_zoom_value: Retained<NSTextField>,
        ui_zoom_out: Retained<NSButton>,
        ui_zoom_in: Retained<NSButton>,
        focus_mode: Retained<NSSwitch>,
        sidebar: Retained<NSSwitch>,
        inspector: Retained<NSSwitch>,
        word_wrap: Retained<NSSwitch>,
        dark_mode: Retained<NSSwitch>,
        reset_zoom: Retained<NSButton>,
        quit: Retained<NSButton>,
    }

    struct QuickSettingsIvars {
        popover: Retained<NSPopover>,
        _status_item: Retained<NSStatusItem>,
        controls: QuickSettingsControls,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements. Every retained
        // AppKit object and this target are main-thread-only.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = QuickSettingsIvars]
        struct QuickSettingsTarget;

        // SAFETY: NSObjectProtocol has no extra implementation requirements.
        unsafe impl NSObjectProtocol for QuickSettingsTarget {}

        impl QuickSettingsTarget {
            #[unsafe(method(togglePopover:))]
            fn toggle_popover(&self, sender: &NSButton) {
                let popover = &self.ivars().popover;
                if popover.isShown() {
                    popover.close();
                } else {
                    popover.showRelativeToRect_ofView_preferredEdge(
                        sender.bounds(),
                        sender,
                        NSRectEdge::MinY,
                    );
                    move_popover_to_active_space(popover);
                }
            }

            #[unsafe(method(performQuickAction:))]
            fn perform_quick_action(&self, sender: &NSButton) {
                if let Some(action) = QuickAction::from_tag(sender.tag()) {
                    self.send(action);
                }
            }

            #[unsafe(method(performQuickToggle:))]
            fn perform_quick_toggle(&self, sender: &NSSwitch) {
                if let Some(action) = QuickAction::from_tag(sender.tag()) {
                    self.send(action);
                }
            }
        }
    );

    impl QuickSettingsTarget {
        fn new(
            mtm: MainThreadMarker,
            popover: Retained<NSPopover>,
            status_item: Retained<NSStatusItem>,
            controls: QuickSettingsControls,
        ) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(QuickSettingsIvars {
                popover,
                _status_item: status_item,
                controls,
            });
            // SAFETY: This calls NSObject's valid initializer on a fresh object.
            unsafe { msg_send![super(this), init] }
        }

        fn send(&self, action: QuickAction) {
            if let Some(sender) = ACTION_SENDER.get() {
                let _ = sender.try_send(action);
            }
        }

        fn sync(&self, snapshot: QuickSettingsSnapshot) {
            let controls = &self.ivars().controls;
            let zoom = snapshot.zoom.clamp(0.8, 2.0);
            controls
                .zoom_value
                .setStringValue(&NSString::from_str(&format!("{:.0}%", zoom * 100.0)));
            controls.zoom_out.setEnabled(zoom > 0.8 + f32::EPSILON);
            controls.zoom_in.setEnabled(zoom < 2.0 - f32::EPSILON);
            let ui_zoom = snapshot.ui_zoom.clamp(0.8, 1.4);
            controls
                .ui_zoom_value
                .setStringValue(&NSString::from_str(&format!("{:.0}%", ui_zoom * 100.0)));
            controls
                .ui_zoom_out
                .setEnabled(ui_zoom > 0.8 + f32::EPSILON);
            controls.ui_zoom_in.setEnabled(ui_zoom < 1.4 - f32::EPSILON);
            controls
                .focus_mode
                .setState(control_state(snapshot.focus_mode));
            controls
                .sidebar
                .setState(control_state(snapshot.shows_sidebar));
            controls
                .inspector
                .setState(control_state(snapshot.shows_inspector));
            controls.dark_mode.setState(control_state(snapshot.dark));
            controls
                .word_wrap
                .setState(control_state(snapshot.word_wrap));
            controls.sidebar.setEnabled(snapshot.sidebar_available);
            controls.inspector.setEnabled(snapshot.inspector_available);
        }
    }

    impl QuickAction {
        fn tag(self) -> isize {
            match self {
                Self::ZoomOut => 0,
                Self::ZoomIn => 1,
                Self::ToggleFocusMode => 2,
                Self::ToggleSidebar => 3,
                Self::ToggleInspector => 4,
                Self::ToggleWrap => 5,
                Self::ToggleAppearance => 6,
                Self::ResetZoom => 7,
                Self::Quit => 8,
                Self::UiZoomOut => 9,
                Self::UiZoomIn => 10,
            }
        }

        fn from_tag(tag: isize) -> Option<Self> {
            match tag {
                0 => Some(Self::ZoomOut),
                1 => Some(Self::ZoomIn),
                2 => Some(Self::ToggleFocusMode),
                3 => Some(Self::ToggleSidebar),
                4 => Some(Self::ToggleInspector),
                5 => Some(Self::ToggleWrap),
                6 => Some(Self::ToggleAppearance),
                7 => Some(Self::ResetZoom),
                8 => Some(Self::Quit),
                9 => Some(Self::UiZoomOut),
                10 => Some(Self::UiZoomIn),
                _ => None,
            }
        }
    }

    pub fn init(cx: &mut App) {
        if ACTION_SENDER.get().is_some() {
            return;
        }

        let Some(mtm) = MainThreadMarker::new() else {
            eprintln!("artifex: quick settings must initialize on the main thread");
            return;
        };

        let (sender, receiver) = async_channel::unbounded();
        if ACTION_SENDER.set(sender).is_err() {
            return;
        }

        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setAnimates(false);
        popover.setContentSize(NSSize::new(PANEL_WIDTH, PANEL_HEIGHT));

        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSSquareStatusItemLength);
        let controls = build_controls(mtm);
        let target = QuickSettingsTarget::new(mtm, popover.clone(), status_item.clone(), controls);

        install_content(&popover, &target, mtm);
        install_status_button(&status_item, &target, mtm);
        CONTROLLER.with(|controller| *controller.borrow_mut() = Some(target));

        cx.spawn(async move |cx| {
            while let Ok(action) = receiver.recv().await {
                let _ = cx.update(|cx| dispatch(action, cx));
            }
        })
        .detach();
    }

    pub fn sync(snapshot: QuickSettingsSnapshot) {
        CONTROLLER.with(|controller| {
            if let Some(controller) = controller.borrow().as_ref() {
                controller.sync(snapshot);
            }
        });
    }

    fn build_controls(mtm: MainThreadMarker) -> QuickSettingsControls {
        QuickSettingsControls {
            zoom_value: value_label("100%", frame(198.0, 386.0, 48.0, 28.0), mtm),
            zoom_out: action_button(
                "-",
                QuickAction::ZoomOut,
                frame(164.0, 386.0, 28.0, 28.0),
                mtm,
            ),
            zoom_in: action_button(
                "+",
                QuickAction::ZoomIn,
                frame(252.0, 386.0, 28.0, 28.0),
                mtm,
            ),
            ui_zoom_value: value_label("100%", frame(198.0, 352.0, 48.0, 28.0), mtm),
            ui_zoom_out: action_button(
                "-",
                QuickAction::UiZoomOut,
                frame(164.0, 352.0, 28.0, 28.0),
                mtm,
            ),
            ui_zoom_in: action_button(
                "+",
                QuickAction::UiZoomIn,
                frame(252.0, 352.0, 28.0, 28.0),
                mtm,
            ),
            focus_mode: toggle(
                QuickAction::ToggleFocusMode,
                frame(242.0, 275.0, 38.0, 24.0),
                mtm,
            ),
            sidebar: toggle(
                QuickAction::ToggleSidebar,
                frame(242.0, 241.0, 38.0, 24.0),
                mtm,
            ),
            inspector: toggle(
                QuickAction::ToggleInspector,
                frame(242.0, 207.0, 38.0, 24.0),
                mtm,
            ),
            word_wrap: toggle(
                QuickAction::ToggleWrap,
                frame(242.0, 131.0, 38.0, 24.0),
                mtm,
            ),
            dark_mode: toggle(
                QuickAction::ToggleAppearance,
                frame(242.0, 55.0, 38.0, 24.0),
                mtm,
            ),
            reset_zoom: action_button(
                "Reset Text Size",
                QuickAction::ResetZoom,
                frame(12.0, 7.0, 132.0, 28.0),
                mtm,
            ),
            quit: action_button(
                "Quit Artifex",
                QuickAction::Quit,
                frame(174.0, 7.0, 114.0, 28.0),
                mtm,
            ),
        }
    }

    fn install_status_button(
        status_item: &NSStatusItem,
        target: &QuickSettingsTarget,
        mtm: MainThreadMarker,
    ) {
        let Some(button) = status_item.button(mtm) else {
            return;
        };
        let Some(image) = symbol_image("slider.horizontal.3", "Artifex quick settings") else {
            return;
        };
        image.setTemplate(true);
        button.setImage(Some(&image));
        button.setToolTip(Some(&NSString::from_str("Artifex Quick Settings")));
        // SAFETY: The target implements togglePopover: with one object argument.
        unsafe {
            button.setTarget(Some(target as &AnyObject));
            button.setAction(Some(sel!(togglePopover:)));
        }
    }

    fn install_content(popover: &NSPopover, target: &QuickSettingsTarget, mtm: MainThreadMarker) {
        let root = NSView::initWithFrame(
            NSView::alloc(mtm),
            frame(0.0, 0.0, PANEL_WIDTH, PANEL_HEIGHT),
        );

        root.addSubview(&section_label("Text Size", 420.0, mtm));
        root.addSubview(&row_label("Content", 390.0, mtm));
        root.addSubview(&row_label("Interface", 356.0, mtm));
        add_separator(&root, 338.0, mtm);
        root.addSubview(&section_label("Display", 309.0, mtm));
        root.addSubview(&row_label("Focus mode", 278.0, mtm));
        root.addSubview(&row_label("Sidebar", 244.0, mtm));
        root.addSubview(&row_label("Inspector", 210.0, mtm));
        add_separator(&root, 193.0, mtm);
        root.addSubview(&section_label("Code", 165.0, mtm));
        root.addSubview(&row_label("Word wrap", 134.0, mtm));
        add_separator(&root, 117.0, mtm);
        root.addSubview(&section_label("Appearance", 89.0, mtm));
        root.addSubview(&row_label("Dark mode", 58.0, mtm));
        add_separator(&root, 41.0, mtm);

        let controls = &target.ivars().controls;
        for button in [
            &controls.zoom_out,
            &controls.zoom_in,
            &controls.ui_zoom_out,
            &controls.ui_zoom_in,
            &controls.reset_zoom,
            &controls.quit,
        ] {
            // SAFETY: QuickSettingsTarget implements performQuickAction:.
            unsafe {
                button.setTarget(Some(target as &AnyObject));
                button.setAction(Some(sel!(performQuickAction:)));
            }
            root.addSubview(button);
        }
        root.addSubview(&controls.zoom_value);
        root.addSubview(&controls.ui_zoom_value);

        for toggle in [
            &controls.focus_mode,
            &controls.sidebar,
            &controls.inspector,
            &controls.word_wrap,
            &controls.dark_mode,
        ] {
            // SAFETY: QuickSettingsTarget implements performQuickToggle:.
            unsafe {
                toggle.setTarget(Some(target as &AnyObject));
                toggle.setAction(Some(sel!(performQuickToggle:)));
            }
            root.addSubview(toggle);
        }

        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        popover.setContentViewController(Some(&controller));
    }

    fn action_button(
        title: &str,
        action: QuickAction,
        frame: NSRect,
        mtm: MainThreadMarker,
    ) -> Retained<NSButton> {
        // SAFETY: Target and action are installed after the target is created.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
        };
        button.setFrame(frame);
        button.setTag(action.tag());
        button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        button.setControlSize(NSControlSize::Small);
        button
    }

    fn toggle(action: QuickAction, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSSwitch> {
        let toggle = NSSwitch::initWithFrame(NSSwitch::alloc(mtm), frame);
        toggle.setTag(action.tag());
        toggle.setControlSize(NSControlSize::Small);
        toggle
    }

    fn section_label(text: &str, y: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        label.setFrame(frame(CONTENT_X, y, CONTENT_WIDTH, 18.0));
        label.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
        label
    }

    fn row_label(text: &str, y: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        label.setFrame(frame(CONTENT_X, y, 190.0, 20.0));
        label
    }

    fn value_label(text: &str, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        label.setFrame(frame);
        label.setAlignment(NSTextAlignment::Center);
        label
    }

    fn add_separator(root: &NSView, y: f64, mtm: MainThreadMarker) {
        let separator =
            NSBox::initWithFrame(NSBox::alloc(mtm), frame(CONTENT_X, y, CONTENT_WIDTH, 1.0));
        separator.setBoxType(NSBoxType::Separator);
        root.addSubview(&separator);
    }

    fn frame(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
    }

    fn control_state(value: bool) -> isize {
        if value {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        }
    }

    fn symbol_image(symbol: &str, label: &str) -> Option<Retained<NSImage>> {
        NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(symbol),
            Some(&NSString::from_str(label)),
        )
    }

    fn move_popover_to_active_space(popover: &NSPopover) {
        let Some(controller) = popover.contentViewController() else {
            return;
        };
        let Some(window) = controller.view().window() else {
            return;
        };
        window.setCollectionBehavior(
            window.collectionBehavior() | NSWindowCollectionBehavior::MoveToActiveSpace,
        );
    }

    fn dispatch(action: QuickAction, cx: &mut App) {
        match action {
            QuickAction::ZoomOut => cx.dispatch_action(&ZoomOut),
            QuickAction::ZoomIn => cx.dispatch_action(&ZoomIn),
            QuickAction::ToggleFocusMode => cx.dispatch_action(&ToggleFocusMode),
            QuickAction::ToggleSidebar => cx.dispatch_action(&ToggleSidebar),
            QuickAction::ToggleInspector => cx.dispatch_action(&ToggleInspector),
            QuickAction::ToggleWrap => cx.dispatch_action(&ToggleWrap),
            QuickAction::ToggleAppearance => cx.dispatch_action(&ToggleAppearance),
            QuickAction::ResetZoom => cx.dispatch_action(&ResetZoom),
            QuickAction::Quit => cx.dispatch_action(&Quit),
            QuickAction::UiZoomOut => cx.dispatch_action(&UiZoomOut),
            QuickAction::UiZoomIn => cx.dispatch_action(&UiZoomIn),
        }
        if action == QuickAction::ResetZoom {
            cx.dispatch_action(&ResetUiZoom);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quick_action_tags_round_trip() {
            let actions = [
                QuickAction::ZoomOut,
                QuickAction::ZoomIn,
                QuickAction::ToggleFocusMode,
                QuickAction::ToggleSidebar,
                QuickAction::ToggleInspector,
                QuickAction::ToggleWrap,
                QuickAction::ToggleAppearance,
                QuickAction::ResetZoom,
                QuickAction::Quit,
                QuickAction::UiZoomOut,
                QuickAction::UiZoomIn,
            ];

            for action in actions {
                assert_eq!(QuickAction::from_tag(action.tag()), Some(action));
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{init, sync};

#[cfg(not(target_os = "macos"))]
pub fn init(_: &mut gpui::App) {}

#[cfg(not(target_os = "macos"))]
pub fn sync(_: QuickSettingsSnapshot) {}
