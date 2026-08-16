//! Small shared chrome pieces.
//!
//! One place for the shapes that repeat across the rail, the sidebar header,
//! the tab strip and the status bar, so they stay identical the way
//! `DESIGN.md` requires.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, IntoElement, MouseButton, SharedString, Window,
    WindowControlArea, div, img, px,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, tooltip::Tooltip, v_flex};

use gpui::{Background, BoxShadow, linear_color_stop, linear_gradient, point};

use crate::services::material_icons;
use crate::theme::{Colors, Metrics, Radius, Space, Type};

/// `DESIGN.md` > AtelierChromeBackground: the chrome wash with a faint top
/// light, shared by the toolbar, the tab strip, panel headers and the status
/// bar so all four read as one piece of hardware.
pub fn chrome_gradient(c: Colors) -> Background {
    linear_gradient(
        180.,
        linear_color_stop(c.chrome.blend(gpui::white().opacity(0.10)), 0.),
        linear_color_stop(c.chrome.blend(gpui::black().opacity(0.04)), 1.),
    )
}

/// Atelier's warm soft shadow: rgb(0.18, 0.12, 0.08) never pure black.
pub fn shadow_soft() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: gpui::hsla(0.07, 0.38, 0.13, 0.12),
        offset: point(px(0.), px(1.)),
        blur_radius: px(3.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Atelier's floating-panel shadow: radius 24, y 12, warm dark at 0.22.
pub fn shadow_floating() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: gpui::hsla(0.06, 0.4, 0.1, 0.22),
        offset: point(px(0.), px(12.)),
        blur_radius: px(24.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// The empty stretch of a unified toolbar row. It fills the space between
/// controls and carries the window-drag behaviour there, so buttons keep their
/// clicks and everything else on the row still moves the window.
///
/// `app_owns_titlebar_drag` stops AppKit from dragging the window and from
/// waiting to disambiguate a double-click, so both actions are handled here.
/// `WindowControlArea::Drag` is the portable hint; on macOS the hit-test hook
/// it feeds is a no-op, which is why the explicit handlers exist.
pub fn toolbar_drag_filler() -> impl IntoElement {
    div()
        .h_full()
        .flex_1()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count >= 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
}

/// A 30-point icon-only control. `DESIGN.md` > Iconography.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    selected: bool,
    c: Colors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = id.into();
    let label = icon_button_label(&id);
    div()
        .id(id)
        .cursor_pointer()
        .size(px(30.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(Radius::ROW)
        .when(selected, |this| this.bg(c.chrome_selection))
        .hover(|this| this.bg(c.hover))
        .active(|this| this.bg(c.pressed))
        .child(Icon::new(icon).small().text_color(if selected {
            c.chrome_selection_ink
        } else {
            c.ink_secondary
        }))
        .tooltip_text(label)
        .on_click(on_click)
}

/// An icon-only control for the graphite global toolbar.
///
/// The public shape mirrors [`icon_button`], while the ink and interaction
/// fills stay on the rail palette so the control remains readable in both
/// appearances.
pub fn toolbar_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    selected: bool,
    c: Colors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = id.into();
    let label = icon_button_label(&id);
    div()
        .id(id)
        .cursor_pointer()
        .size(px(30.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(Radius::ROW)
        .when(selected, |this| this.bg(c.rail_selection))
        .hover(|this| this.bg(c.rail_hover))
        .active(|this| this.bg(c.rail_pressed))
        .child(Icon::new(icon).small().text_color(c.rail_foreground))
        .tooltip_text(label)
        .on_click(on_click)
}

fn icon_button_label(id: &ElementId) -> &'static str {
    match id.to_string().as_str() {
        "toggle-sidebar" => "Toggle navigator",
        "toggle-appearance" => "Toggle appearance",
        "toggle-changes" => "Show changes",
        "focus-mode" => "Toggle focus mode",
        "toggle-inspector" => "Toggle context rail",
        "refresh-tree" => "Refresh files",
        "stage-all" => "Stage all changes",
        "refresh-git" => "Refresh changes",
        "toggle-wrap" => "Toggle word wrap",
        "search-all" => "Search all files",
        "new-terminal" => "New terminal",
        _ => "Action",
    }
}

/// High-contrast count used by every non-zero badge on the graphite rail.
pub fn rail_count_badge(count: usize, c: Colors, ui_zoom: f32) -> impl IntoElement {
    div()
        .flex_none()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(c.accent)
        .font_family("JetBrains Mono")
        .text_size(Type::MICRO * ui_zoom)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(c.accent_ink)
        .child(SharedString::from(count.to_string()))
}

/// Monospaced count with a semantic tint. `DESIGN.md` > AtelierCountBadge.
///
/// The tint carries the meaning, so the fill stays a wash of the same colour
/// rather than a neutral chip competing with the label beside it.
pub fn count_badge(count: usize, tint: Hsla, _c: Colors, ui_zoom: f32) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(5.))
        .py(px(1.))
        .rounded(Radius::ROW)
        .bg(tint.opacity(0.14))
        .font_family("JetBrains Mono")
        .text_size(Type::MICRO * ui_zoom)
        .text_color(tint)
        .child(SharedString::from(count.to_string()))
}

/// A header tab: icon, label, optional count. Each tab claims an equal share
/// of the header, so two tabs read as one segmented control split down the
/// middle instead of two pills floating at the leading edge.
pub fn pill_tab(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    count: Option<usize>,
    selected: bool,
    c: Colors,
    ui_zoom: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let ink = if selected {
        c.chrome_selection_ink
    } else {
        c.ink_secondary
    };
    h_flex()
        .id(id)
        .cursor_pointer()
        .h(Metrics::ROW)
        // Equal share of the header. `min_w(0)` lets the share shrink below the
        // label width, so a narrow sidebar truncates instead of overflowing.
        .flex_1()
        .min_w(px(0.))
        .items_center()
        .justify_center()
        .gap(Space::XS)
        .px(Space::S)
        .rounded(Radius::ROW)
        // `DESIGN.md`: one inset pill of translucent chrome-selection glass
        // with a top-lit hairline. The hairline is what stops it reading as a
        // floating card.
        .when(selected, |this| {
            this.bg(c.chrome_selection).border_t_1().border_color(
                c.chrome_selection
                    .opacity(0.0)
                    .blend(gpui::white().opacity(0.5)),
            )
        })
        .when(!selected, |this| {
            this.border_t_1()
                .border_color(gpui::transparent_black())
                .hover(|this| this.bg(c.hover))
        })
        .child(Icon::new(icon).xsmall().flex_none().text_color(ink))
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .text_size(Type::LABEL * ui_zoom)
                .text_color(ink)
                .when(selected, |this| this.font_weight(gpui::FontWeight::MEDIUM))
                .child(label),
        )
        .when_some(count.filter(|n| *n > 0), |this, count| {
            this.child(count_badge(count, c.git_modified, c, ui_zoom))
        })
        .on_click(on_click)
}

/// `DESIGN.md` > AtelierEmptyState: one calm icon well, a serif title and a
/// short message.
pub fn empty_state(
    icon: IconName,
    title: &'static str,
    message: &'static str,
    c: Colors,
    ui_zoom: f32,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(Space::M)
        .child(
            div()
                .size(px(56.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(Radius::PANEL)
                .bg(c.raised.opacity(0.6))
                .child(Icon::new(icon).text_color(c.ink_secondary)),
        )
        .child(
            div()
                .font_family("Times New Roman")
                .text_size(Type::TITLE * ui_zoom)
                .text_color(c.ink)
                .child(title),
        )
        .child(
            div()
                .max_w(px(320.))
                .text_center()
                .text_size(Type::BODY * ui_zoom)
                .text_color(c.ink_secondary)
                .child(message),
        )
}

/// A file or folder glyph. Either a tinted monochrome icon from the
/// gpui-component set, or a full-colour Material SVG resolved from the ported
/// theme. Identity only; Git state stays in its own slot.
#[derive(Clone)]
pub enum Glyph {
    /// A gpui-component icon, tinted. Used for terminals, diffs, and previews
    /// the Material theme has no file to match.
    Mono(IconName, Hsla),
    /// A ported Material icon, resolved to an embedded SVG resource path.
    Material(SharedString),
}

impl Glyph {
    /// Renders the glyph at one of the two sizes the surfaces use: the `xsmall`
    /// tree/tab/quick glyph, or the `small` inspector-header glyph.
    pub fn render(self, small: bool) -> AnyElement {
        match self {
            Glyph::Mono(icon, tint) => {
                let icon = Icon::new(icon).text_color(tint);
                let icon = if small { icon.small() } else { icon.xsmall() };
                icon.into_any_element()
            }
            Glyph::Material(path) => {
                let side = if small { px(16.) } else { px(14.) };
                img(path).size(side).flex_none().into_any_element()
            }
        }
    }
}

/// The colour glyph for a file, resolved from the ported Material icon theme by
/// name and extension.
pub fn file_glyph(name: &str, light: bool) -> Glyph {
    Glyph::Material(material_icons::file_icon(name, light))
}

/// The colour glyph for a folder, split by open state.
pub fn folder_glyph(name: &str, expanded: bool, light: bool) -> Glyph {
    Glyph::Material(material_icons::folder_icon(name, expanded, light))
}

/// The project command trigger. `DESIGN.md` gives it a 420-point command-centre
/// width, centred against the whole window and middle-truncated.
pub fn project_menu(name: &str, path: &str, c: Colors, ui_zoom: f32) -> impl IntoElement {
    v_flex()
        .id("project-menu")
        .cursor_pointer()
        .w(Metrics::PROJECT_MENU_WIDTH)
        .h(px(30.))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(Radius::CONTROL)
        .border_1()
        .border_color(c.border.opacity(0.6))
        .bg(c.raised.opacity(0.5))
        .hover(|this| this.bg(c.hover))
        .child(
            div()
                .max_w_full()
                .truncate()
                .text_size(Type::LABEL * ui_zoom)
                .text_color(c.ink)
                .child(SharedString::from(name.to_string())),
        )
        .tooltip_text(path)
}

/// Adds one shared text tooltip to lightweight interactive chrome.
pub trait QuietTooltip: Sized + gpui::StatefulInteractiveElement {
    fn tooltip_text(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        self.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
    }
}
impl<T> QuietTooltip for T where T: gpui::StatefulInteractiveElement {}
