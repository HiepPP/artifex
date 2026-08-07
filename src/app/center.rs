//! Center tab strip and tab content.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement, SharedString, Styled as _, div, px,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::app::chrome::{Glyph, empty_state, file_glyph, icon_button};
use crate::app::shell::Shell;
use crate::app::workspace::{FileMode, PreviewKind, TabKind, is_html_path};
use crate::theme::{ActiveTokens as _, Metrics, Radius, Space, Type};

impl Shell {
    pub(crate) fn render_center(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_web_preview(window, cx);
        v_flex()
            .size_full()
            .child(self.render_tab_strip(cx))
            .child(self.render_tab_content(cx))
    }

    /// `DESIGN.md` > Center Tabs. Selected tab is one inset rounded pill of
    /// warm glass; the close control sits at the leading edge and only shows on
    /// the selected or hovered tab.
    fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let light = !cx.tokens().dark;
        let selected = self.workspace().selected;
        let previewable = match self.workspace().selected_tab().map(|tab| &tab.kind) {
            Some(TabKind::File {
                path, preview_view, ..
            }) => preview_view.is_some() || is_html_path(path),
            _ => false,
        };
        let in_preview = matches!(
            self.workspace().selected_tab().map(|tab| &tab.kind),
            Some(TabKind::File {
                mode: FileMode::Preview,
                ..
            })
        );
        // Word-wrap toggle applies to a source editor only. `Some(wrapped)` shows
        // the button and its active state; `None` hides it.
        let wrap_state: Option<bool> = match self.workspace().selected_tab().map(|tab| &tab.kind) {
            Some(TabKind::File {
                editor,
                mode: FileMode::Source,
                ..
            }) => Some(editor.read(cx).wrap),
            _ => None,
        };

        struct Strip {
            index: usize,
            title: String,
            preview: bool,
            glyph: Glyph,
            closable: bool,
        }

        let terminals = self
            .workspace()
            .tabs
            .iter()
            .filter(|tab| tab.is_terminal())
            .count();
        let tabs: Vec<Strip> = self
            .workspace()
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let glyph = match &tab.kind {
                    TabKind::Terminal(_) => Glyph::Mono(IconName::SquareTerminal, c.ink_secondary),
                    TabKind::File { path, .. } => file_glyph(
                        &path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        light,
                    ),
                    TabKind::Image { .. } => Glyph::Mono(IconName::Frame, c.git_untracked),
                    TabKind::Video { .. } => Glyph::Mono(IconName::Eye, c.git_untracked),
                    TabKind::Diff { .. } | TabKind::ImageDiff { .. } => {
                        Glyph::Mono(IconName::Replace, c.git_modified)
                    }
                };
                Strip {
                    index,
                    title: tab.title.clone(),
                    preview: tab.preview,
                    glyph,
                    closable: !tab.is_terminal() || terminals > 1,
                }
            })
            .collect();

        h_flex()
            .h(Metrics::TAB_BAR)
            .w_full()
            .flex_none()
            .items_center()
            .px(Space::XS)
            .bg(crate::app::chrome::chrome_gradient(c))
            .border_b_1()
            .border_color(c.border)
            .child(
                h_flex()
                    .id("tab-scroller")
                    .flex_1()
                    .overflow_x_scroll()
                    .gap(px(2.))
                    .children(tabs.into_iter().map(|tab| {
                        let index = tab.index;
                        let is_selected = index == selected;
                        h_flex()
                            .id(("tab", index))
                            .cursor_pointer()
                            .group(SharedString::from(format!("tab-{index}")))
                            .h(Metrics::ROW)
                            .min_w(px(112.))
                            .max_w(px(220.))
                            .items_center()
                            .gap(Space::XS)
                            .px(Space::S)
                            .rounded(Radius::ROW)
                            // Same pill as the sidebar header: one glass fill
                            // and a top-lit hairline, never a card.
                            .when(is_selected, |this| {
                                this.bg(c.chrome_selection)
                                    .text_color(c.chrome_selection_ink)
                                    .border_t_1()
                                    .border_color(gpui::white().opacity(0.5))
                                    .shadow(crate::app::chrome::shadow_soft())
                            })
                            .when(!is_selected, |this| {
                                this.text_color(c.ink_secondary)
                                    .border_t_1()
                                    .border_color(gpui::transparent_black())
                                    .hover(|this| this.bg(c.hover))
                                    .active(|this| this.bg(c.pressed))
                            })
                            // The slot exists only on a closable tab, so the
                            // last terminal does not carry an empty gutter.
                            .when(tab.closable, |this| {
                                this.child(
                                    div()
                                        .id(("close", index))
                                        .w(px(14.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .child(Icon::new(IconName::Close).xsmall().text_color(
                                            if is_selected {
                                                c.chrome_selection_ink
                                            } else {
                                                c.ink_secondary
                                            },
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.workspace_mut().close_tab(index);
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(tab.glyph.render(false))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(Type::LABEL * ui_zoom)
                                    .when(tab.preview, |this| this.italic().opacity(0.72))
                                    .child(SharedString::from(tab.title)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.workspace_mut().selected = index;
                                cx.notify();
                            }))
                    })),
            )
            // DESIGN.md keeps editor actions in one trailing group after the
            // scroller, with New Terminal always the far-right action.
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap(Space::XS)
                    .px(Space::XS)
                    .when(previewable, |this| {
                        this.child(icon_button(
                            "toggle-preview",
                            if in_preview {
                                IconName::BookOpen
                            } else {
                                IconName::Eye
                            },
                            in_preview,
                            c,
                            cx.listener(|this, _, _, cx| {
                                this.workspace_mut().toggle_mode();
                                cx.notify();
                            }),
                        ))
                    })
                    .when_some(wrap_state, |this, wrapped| {
                        this.child(icon_button(
                            "toggle-wrap",
                            IconName::Menu,
                            wrapped,
                            c,
                            cx.listener(|this, _, window, cx| {
                                this.on_toggle_wrap(&crate::app::shell::ToggleWrap, window, cx)
                            }),
                        ))
                    })
                    .child(icon_button(
                        "search-all",
                        IconName::Search,
                        false,
                        c,
                        cx.listener(|this, _, window, cx| {
                            this.on_search_all(&crate::app::shell::SearchAllFiles, window, cx)
                        }),
                    ))
                    .child(icon_button(
                        "new-terminal",
                        IconName::Plus,
                        false,
                        c,
                        cx.listener(|this, _, window, cx| {
                            this.workspace_mut().open_terminal(window, cx);
                            cx.notify();
                        }),
                    )),
            )
    }

    fn render_tab_content(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let selected = self.workspace().selected;

        // Every terminal stays mounted. Switching tabs changes visibility, not
        // the view tree, so the shell process and its scrollback survive.
        let terminals: Vec<(usize, gpui::Entity<crate::terminal::TerminalView>)> = self
            .workspace()
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| match &tab.kind {
                TabKind::Terminal(view) => Some((index, view.clone())),
                _ => None,
            })
            .collect();

        for (index, view) in &terminals {
            let active = *index == selected;
            view.update(cx, |view, _| view.set_active(active));
        }

        let foreground: Option<AnyElement> = match self.workspace().tabs.get(selected) {
            Some(tab) => match &tab.kind {
                TabKind::Terminal(_) => None,
                TabKind::File {
                    editor,
                    mode,
                    preview_view,
                    ..
                } => {
                    let editor = editor.clone();
                    Some(match (preview_view, mode) {
                        (Some(PreviewKind::Markdown(preview)), FileMode::Preview) => {
                            preview.clone().into_any_element()
                        }
                        (Some(PreviewKind::Web(preview)), FileMode::Preview) => {
                            preview.clone().into_any_element()
                        }
                        _ => editor.into_any_element(),
                    })
                }
                TabKind::Image { path } => Some(image_pane(path.clone(), c).into_any_element()),
                TabKind::Video { view, .. } => Some(match view {
                    Some(view) => view.clone().into_any_element(),
                    // The webview arrives on the next frame, created by
                    // ensure_web_preview.
                    None => div().size_full().bg(c.editor).into_any_element(),
                }),
                TabKind::Diff { view, .. } => Some(view.clone().into_any_element()),
                TabKind::ImageDiff { old, new, .. } => Some(match (old, new) {
                    // One-sided change: show the surviving image full, no
                    // "absent" column.
                    (None, Some(new)) => image_pane(new.clone(), c).into_any_element(),
                    (Some(old), None) => image_pane(old.clone(), c).into_any_element(),
                    _ => h_flex()
                        .size_full()
                        .p(Space::M)
                        .gap(Space::M)
                        .child(image_diff_side("HEAD", old.clone(), c, ui_zoom))
                        .child(image_diff_side("Working", new.clone(), c, ui_zoom))
                        .into_any_element(),
                }),
            },
            None => Some(
                empty_state(
                    IconName::Folder,
                    "Nothing open",
                    "Pick a file in the Explorer, or press Command-T for a terminal.",
                    c,
                    ui_zoom,
                )
                .into_any_element(),
            ),
        };

        div()
            .flex_1()
            .relative()
            .bg(c.editor)
            .children(terminals.into_iter().map(|(index, view)| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .when(index != selected, |this| this.invisible())
                    .child(view)
            }))
            .when_some(foreground, |this, element| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(c.editor)
                        .child(element),
                )
            })
    }
}

/// One image, centred and contained, on the editor surface.
fn image_pane(path: std::path::PathBuf, c: crate::theme::Colors) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(Space::M)
        .bg(c.editor)
        .child(
            gpui::img(path)
                .max_w_full()
                .max_h_full()
                .object_fit(gpui::ObjectFit::Contain),
        )
}

/// One side of the image diff: a label band over the image, or an "absent"
/// placeholder for an added or deleted side.
fn image_diff_side(
    label: &'static str,
    path: Option<std::path::PathBuf>,
    c: crate::theme::Colors,
    ui_zoom: f32,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .rounded(Radius::CONTROL)
        .border_1()
        .border_color(c.border)
        .overflow_hidden()
        .child(
            div()
                .w_full()
                .px(Space::S)
                .py(px(3.))
                .bg(c.raised)
                .text_size(Type::MICRO * ui_zoom)
                .font_family("JetBrains Mono")
                .text_color(c.ink_secondary)
                .child(label),
        )
        .child(match path {
            Some(path) => div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .p(Space::S)
                .child(
                    gpui::img(path)
                        .max_w_full()
                        .max_h_full()
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(Type::CAPTION * ui_zoom)
                .text_color(c.ink_secondary)
                .child("absent")
                .into_any_element(),
        })
}
