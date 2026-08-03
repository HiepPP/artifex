//! Center tab strip and tab content.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement, SharedString, Styled as _, div, px,
    uniform_list,
};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::app::chrome::{empty_state, file_icon, icon_button};
use crate::app::shell::Shell;
use crate::app::workspace::{FileMode, TabKind};
use crate::theme::{ActiveTokens as _, Metrics, Radius, Space, Type};

impl Shell {
    pub(crate) fn render_center(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let selected = self.workspace().selected;
        let previewable = matches!(
            self.workspace().selected_tab().map(|tab| &tab.kind),
            Some(TabKind::File {
                preview_view: Some(_),
                ..
            })
        );
        let in_preview = matches!(
            self.workspace().selected_tab().map(|tab| &tab.kind),
            Some(TabKind::File {
                mode: FileMode::Preview,
                ..
            })
        );

        struct Strip {
            index: usize,
            title: String,
            preview: bool,
            icon: IconName,
            tint: gpui::Hsla,
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
                let (icon, tint) = match &tab.kind {
                    TabKind::Terminal(_) => (IconName::SquareTerminal, c.ink_secondary),
                    TabKind::File { path, .. } => file_icon(
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                            .as_str(),
                        c,
                    ),
                    TabKind::Diff { .. } => (IconName::Replace, c.git_modified),
                };
                Strip {
                    index,
                    title: tab.title.clone(),
                    preview: tab.preview,
                    icon,
                    tint,
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
                            .child(Icon::new(tab.icon).xsmall().text_color(tab.tint))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(Type::LABEL)
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
                        (Some(preview), FileMode::Preview) => preview.clone().into_any_element(),
                        _ => editor.into_any_element(),
                    })
                }
                TabKind::Diff { text, .. } => Some(render_diff(text, cx).into_any_element()),
            },
            None => Some(
                empty_state(
                    IconName::Folder,
                    "Nothing open",
                    "Pick a file in the Explorer, or press Command-T for a terminal.",
                    c,
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

fn render_diff(text: &str, cx: &mut Context<Shell>) -> impl IntoElement {
    let c = cx.tokens().c;
    let lines: Vec<String> = text
        .lines()
        .filter(|line| {
            // DESIGN.md: omit raw git file metadata from the preview.
            !(line.starts_with("diff --git")
                || line.starts_with("index ")
                || line.starts_with("--- ")
                || line.starts_with("+++ "))
        })
        .take(20_000)
        .map(|l| l.to_string())
        .collect();
    let count = lines.len();

    uniform_list("diff-rows", count, move |range, _window, cx| {
        let colors = cx.tokens().c;
        range
            .map(|index| {
                let line = lines.get(index).cloned().unwrap_or_default();
                let color = if line.starts_with('+') {
                    colors.git_added
                } else if line.starts_with('-') {
                    colors.git_deleted
                } else if line.starts_with("@@") {
                    colors.git_untracked
                } else {
                    colors.ink
                };
                h_flex()
                    .w_full()
                    .child(
                        div()
                            .w(px(48.))
                            .flex_none()
                            .pr(Space::S)
                            .text_right()
                            .text_color(colors.ink_secondary.opacity(0.7))
                            .child(SharedString::from((index + 1).to_string())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(color)
                            .child(SharedString::from(line)),
                    )
                    .into_any_element()
            })
            .collect()
    })
    .size_full()
    .p(Space::S)
    .bg(c.editor)
    .font_family("JetBrains Mono")
    .text_size(Type::BODY)
}
