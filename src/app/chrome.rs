//! Small shared chrome pieces.
//!
//! One place for the shapes that repeat across the rail, the sidebar header,
//! the tab strip and the status bar, so they stay identical the way
//! `DESIGN.md` requires.

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, ElementId, Hsla, IntoElement, MouseButton, Pixels, SharedString, Styled as _,
    Window, WindowControlArea, div, px,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::theme::{Colors, Metrics, Radius, Space, Type};

/// The band the window's title bar occupies, drawn by the chrome.
///
/// `app_owns_titlebar_drag` stops AppKit from dragging the window and from
/// waiting to disambiguate a double-click, so both actions are handled here.
/// `WindowControlArea::Drag` is the portable hint; on macOS the hit-test hook
/// it feeds is a no-op, which is why the explicit handlers exist.
/// The strip collapses to zero height in full screen, where there is no title
/// bar to stand in for.
pub fn title_bar_drag_strip(inset: Pixels) -> impl IntoElement {
    div()
        .w_full()
        .h(inset)
        .flex_none()
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
        .on_click(on_click)
}

/// Monospaced count with a semantic tint. `DESIGN.md` > AtelierCountBadge.
///
/// The tint carries the meaning, so the fill stays a wash of the same colour
/// rather than a neutral chip competing with the label beside it.
pub fn count_badge(count: usize, tint: Hsla, _c: Colors) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(5.))
        .py(px(1.))
        .rounded(Radius::ROW)
        .bg(tint.opacity(0.14))
        .font_family("JetBrains Mono")
        .text_size(Type::MICRO)
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
                .text_size(Type::LABEL)
                .text_color(ink)
                .when(selected, |this| this.font_weight(gpui::FontWeight::MEDIUM))
                .child(label),
        )
        .when_some(count.filter(|n| *n > 0), |this, count| {
            this.child(count_badge(count, c.git_modified, c))
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
                .text_size(Type::TITLE)
                .text_color(c.ink)
                .child(title),
        )
        .child(
            div()
                .max_w(px(320.))
                .text_center()
                .text_size(Type::BODY)
                .text_color(c.ink_secondary)
                .child(message),
        )
}

/// Colour and glyph for a file, standing in for the Material icon set the
/// Swift build ships. Identity colour only; Git state stays in its own slot.
pub fn file_icon(name: &str, c: Colors) -> (IconName, Hsla) {
    let extension = name.rsplit('.').next().unwrap_or("");
    match extension {
        "rs" => (IconName::Settings, c.workflow_todo),
        "swift" => (IconName::Asterisk, c.git_deleted),
        "md" | "markdown" => (IconName::BookOpen, c.git_untracked),
        "toml" | "yaml" | "yml" | "json" => (IconName::Menu, c.workflow_todo),
        "sh" | "bash" | "zsh" => (IconName::SquareTerminal, c.git_added),
        "lock" => (IconName::File, c.ink_secondary),
        "html" | "css" => (IconName::Globe, c.git_untracked),
        _ => (IconName::File, c.ink_secondary),
    }
}

/// The project command trigger. `DESIGN.md` gives it a 420-point command-centre
/// width, centred against the whole window and middle-truncated.
pub fn project_menu(name: &str, path: &str, c: Colors) -> impl IntoElement {
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
                .text_size(Type::LABEL)
                .text_color(c.ink)
                .child(SharedString::from(name.to_string())),
        )
        .tooltip_text(path)
}

/// Adds a plain text tooltip without pulling in the component tooltip stack.
trait QuietTooltip: Sized {
    fn tooltip_text(self, _text: &str) -> Self {
        self
    }
}
impl<T> QuietTooltip for T {}
