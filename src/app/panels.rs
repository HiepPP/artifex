//! Sidebar (Explorer and Git) and the inspector placeholder.

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Hsla, IntoElement, ParentElement, SharedString, Styled as _, Window,
    div, px, uniform_list,
};
use gpui_component::input::Input;
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::app::chrome::{Glyph, file_glyph, folder_glyph, icon_button, pill_tab};
use crate::app::shell::{Shell, SidebarTab};
use crate::services::git::{self, ChangeKind};
use crate::theme::{ActiveTokens as _, Colors, Metrics, Radius, Space, Type};

/// One compact icon button for a change row's hover actions. Smaller than the
/// toolbar's [`icon_button`] so it fits inside a `Metrics::ROW` line.
fn change_action_button(
    id: SharedString,
    icon: IconName,
    tint: Hsla,
    c: Colors,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .cursor_pointer()
        .flex_none()
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(Radius::ROW)
        .hover(|this| this.bg(c.raised))
        .active(|this| this.bg(c.pressed))
        .child(Icon::new(icon).xsmall().text_color(tint))
        .on_click(on_click)
}

impl Shell {
    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let tab = self.sidebar_tab;
        let changed = self.workspace().git.changed_count();

        v_flex()
            .size_full()
            .bg(c.sidebar)
            .border_r_1()
            .border_color(c.border)
            .child(
                // DESIGN.md keeps the sidebar header limited to the Explorer
                // and Git tabs, with the Git change count as a trailing badge.
                h_flex()
                    .h(Metrics::PANEL_HEADER)
                    .w_full()
                    .flex_none()
                    .items_center()
                    .gap(Space::XS)
                    .px(Space::XS)
                    .bg(crate::app::chrome::chrome_gradient(c))
                    .border_b_1()
                    .border_color(c.border)
                    .child(pill_tab(
                        "tab-explorer",
                        IconName::Folder,
                        "Explorer",
                        None,
                        tab == SidebarTab::Explorer,
                        c,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_tab = SidebarTab::Explorer;
                            cx.notify();
                        }),
                    ))
                    .child(pill_tab(
                        "tab-git",
                        IconName::Network,
                        "Git",
                        Some(changed),
                        tab == SidebarTab::Git,
                        c,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_tab = SidebarTab::Git;
                            cx.notify();
                        }),
                    )),
            )
            .child(match tab {
                SidebarTab::Explorer => self.render_explorer(cx).into_any_element(),
                SidebarTab::Git => self.render_git(cx).into_any_element(),
            })
    }

    fn render_explorer(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let entity = cx.entity();
        let count = self.workspace().tree.rows.len();
        let selected = self
            .workspace()
            .selected_tab()
            .and_then(|tab| tab.file_path())
            .map(|p| p.to_path_buf());

        v_flex()
            .flex_1()
            // DESIGN.md puts a compact contextual toolbar directly below the
            // tab bar and right-aligns its actions.
            .child(
                h_flex()
                    .h(Metrics::SECTION_HEADER)
                    .w_full()
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .px(Space::XS)
                    .gap(px(2.))
                    .child(icon_button(
                        "reveal-file",
                        IconName::Frame,
                        false,
                        c,
                        cx.listener(|this, _, _, cx| {
                            let path = this
                                .workspace()
                                .selected_tab()
                                .and_then(|tab| tab.file_path())
                                .map(|p| p.to_path_buf());
                            if let Some(path) = path {
                                this.workspace_mut().tree.reveal(&path);
                                this.set_status("revealed active file");
                            }
                            cx.notify();
                        }),
                    ))
                    .child(icon_button(
                        "refresh-tree",
                        IconName::Redo,
                        false,
                        c,
                        cx.listener(|this, _, _, cx| {
                            this.workspace_mut().reindex();
                            this.set_status("explorer refreshed");
                            cx.notify();
                        }),
                    )),
            )
            .child(
                uniform_list("explorer-rows", count, move |range, _window, cx| {
                    let shell = entity.read(cx);
                    let colors = cx.tokens().c;
                    let light = !cx.tokens().dark;
                    let workspace = shell.workspace();
                    range
                        .map(|index| {
                            let Some(row) = workspace.tree.rows.get(index) else {
                                return div().into_any_element();
                            };
                            let is_dir = row.entry.is_dir;
                            let expanded = row.expanded;
                            let path = row.entry.path.clone();
                            let indent = px(8. + row.depth as f32 * 12.);
                            let is_selected = selected.as_deref() == Some(path.as_path());
                            let ignored = workspace.is_ignored(&path);
                            let glyph = if is_dir {
                                folder_glyph(&row.entry.name, expanded, light)
                            } else {
                                file_glyph(&row.entry.name, light)
                            };
                            let click_path = path.clone();
                            h_flex()
                                .id(("tree", index))
                                // DESIGN.md: the whole row is the hit target and
                                // keeps the pointing-hand cursor.
                                .cursor_pointer()
                                .h(Metrics::ROW)
                                .w_full()
                                .items_center()
                                .pl(indent)
                                .pr(Space::S)
                                .rounded(Radius::ROW)
                                .when(is_selected, |this| this.bg(colors.selection))
                                .hover(|this| this.bg(colors.hover))
                                // The disclosure gutter is reserved on every
                                // row so file and folder labels share one
                                // text indent.
                                .child(div().w(px(14.)).flex_none().flex().items_center().when(
                                    is_dir,
                                    |this| {
                                        this.child(
                                            Icon::new(if expanded {
                                                IconName::ChevronDown
                                            } else {
                                                IconName::ChevronRight
                                            })
                                            .xsmall()
                                            .text_color(colors.ink_secondary),
                                        )
                                    },
                                ))
                                .child(
                                    div()
                                        .w(px(18.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .child(glyph.render(false)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .text_size(Type::BODY)
                                        .text_color(colors.file_tree_foreground)
                                        .child(SharedString::from(row.entry.name.clone())),
                                )
                                // Ignored content stays visible and quiet.
                                .when(ignored, |this| this.opacity(0.45))
                                .on_click({
                                    let entity = entity.clone();
                                    move |event: &gpui::ClickEvent, _window, cx| {
                                        entity.update(cx, |shell, cx| {
                                            if is_dir {
                                                shell.workspace_mut().tree.toggle(&click_path);
                                            } else {
                                                // DESIGN.md > Explorer: single
                                                // click previews, double click
                                                // opens a permanent tab.
                                                let preview = event.click_count() < 2;
                                                shell.workspace_mut().open_file(
                                                    click_path.clone(),
                                                    preview,
                                                    cx,
                                                );
                                            }
                                            cx.notify();
                                        });
                                    }
                                })
                                .into_any_element()
                        })
                        .collect()
                })
                .flex_1()
                .px(Space::XS),
            )
    }

    fn render_git(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let snapshot = self.workspace().git.clone();
        let root = self.workspace().root.clone();
        let name = self.workspace().name.clone();
        let short_root = shorten_path(&root);
        let commit_input = self.workspace().commit_input.clone();
        let pushing = self.workspace().pushing;

        v_flex()
            .id("git-scroll")
            .flex_1()
            .overflow_y_scroll()
            .pb(Space::L)
            .child(
                h_flex()
                    .h(Metrics::SECTION_HEADER)
                    .w_full()
                    .items_center()
                    .justify_end()
                    .px(Space::XS)
                    .gap(px(2.))
                    .child(icon_button("stage-all", IconName::Check, false, c, {
                        let root = root.clone();
                        cx.listener(move |this, _, _, cx| {
                            match git::stage_all(&root) {
                                Ok(()) => this.set_status("staged all"),
                                Err(err) => this.set_status(err),
                            }
                            this.workspace_mut().refresh_git();
                            cx.notify();
                        })
                    }))
                    .child(icon_button(
                        "refresh-git",
                        IconName::Redo,
                        false,
                        c,
                        cx.listener(|this, _, _, cx| {
                            this.workspace_mut().refresh_git();
                            this.set_status("git refreshed");
                            cx.notify();
                        }),
                    )),
            )
            .when(!snapshot.is_repo, |this| {
                this.child(
                    div()
                        .p(Space::L)
                        .text_size(Type::BODY)
                        .text_color(c.ink_secondary)
                        .child("This workspace is not a Git repository."),
                )
            })
            .when(snapshot.is_repo, |this| {
                // DESIGN.md > Git: one compact repository card carrying the
                // workspace name, a shortened path and the current branch.
                this.child(
                    v_flex()
                        .mx(Space::M)
                        .p(Space::M)
                        .gap(px(2.))
                        .rounded(Radius::CONTROL)
                        .border_1()
                        .border_color(c.border)
                        .bg(c.panel)
                        .child(
                            h_flex()
                                .items_center()
                                .gap(Space::XS)
                                .child(Icon::new(IconName::Folder).xsmall().text_color(c.accent))
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .text_size(Type::BODY)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(SharedString::from(name)),
                                ),
                        )
                        .child(
                            div()
                                .font_family("JetBrains Mono")
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .truncate()
                                .child(SharedString::from(short_root)),
                        )
                        .child(
                            h_flex()
                                .pt(Space::XS)
                                .items_center()
                                .gap(Space::XS)
                                .child(
                                    Icon::new(IconName::Network)
                                        .xsmall()
                                        .flex_shrink_0()
                                        .text_color(c.ink_secondary),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .font_family("JetBrains Mono")
                                        .text_size(Type::MICRO)
                                        .child(SharedString::from(snapshot.branch.clone())),
                                ),
                        ),
                )
            })
            .child(self.render_change_section("Staged", &snapshot.staged, true, cx))
            .child(self.render_change_section("Changes", &snapshot.unstaged, false, cx))
            .when(snapshot.is_repo, |this| {
                this.child(
                    v_flex()
                        .mx(Space::M)
                        .mt(Space::M)
                        .gap(Space::S)
                        .child(
                            div()
                                .rounded(Radius::CONTROL)
                                .border_1()
                                .border_color(c.border)
                                .bg(c.editor)
                                .p(Space::S)
                                .child(Input::new(&commit_input).appearance(false)),
                        )
                        // DESIGN.md replaces the commit action with one Push
                        // that stages, commits and pushes in that order.
                        .child(
                            div()
                                .id("push")
                                .cursor_pointer()
                                .h(Metrics::CONTROL)
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(Radius::CONTROL)
                                .bg(c.accent)
                                .text_color(c.accent_ink)
                                .text_size(Type::BODY)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .when(pushing, |this| this.opacity(0.45))
                                .child(if pushing { "Pushing..." } else { "Push" })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.push_commit(cx);
                                })),
                        ),
                )
            })
            .when(!snapshot.commits.is_empty(), |this| {
                this.child(
                    v_flex()
                        .mt(Space::M)
                        .child(
                            h_flex()
                                .h(Metrics::SECTION_HEADER)
                                .items_center()
                                .px(Space::M)
                                .gap(Space::S)
                                .child(
                                    div()
                                        .text_size(Type::BODY)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child("Recent"),
                                ),
                        )
                        .children(snapshot.commits.iter().enumerate().map(|(index, commit)| {
                            v_flex()
                                .px(Space::M)
                                .py(Space::XS)
                                .gap(px(1.))
                                .child(
                                    h_flex()
                                        .items_start()
                                        .gap(Space::S)
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_size(Type::BODY)
                                                .child(SharedString::from(commit.subject.clone())),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .font_family("JetBrains Mono")
                                                .text_size(Type::MICRO)
                                                .text_color(c.ink_secondary)
                                                .child(SharedString::from(
                                                    commit.short_hash.clone(),
                                                )),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .gap(Space::S)
                                        .text_size(Type::MICRO)
                                        .text_color(c.ink_secondary)
                                        .child(SharedString::from(commit.author.clone()))
                                        .child(SharedString::from(git::relative_time(
                                            commit.seconds,
                                        )))
                                        .when(index == 0, |this| {
                                            this.child(
                                                div()
                                                    .px(px(4.))
                                                    .rounded(Radius::ROW)
                                                    .bg(c.raised)
                                                    .text_color(c.accent)
                                                    .child("HEAD"),
                                            )
                                        }),
                                )
                        })),
                )
            })
    }

    /// One change section. `DESIGN.md` keeps Git status in the trailing slot at
    /// rest and overlays the row action in that same slot on hover, so no row
    /// changes width when the pointer arrives.
    fn render_change_section(
        &mut self,
        title: &'static str,
        changes: &[git::Change],
        staged: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = cx.tokens().c;
        let light = !cx.tokens().dark;
        let root = self.workspace().root.clone();
        let armed = self.discard_armed.clone();

        v_flex()
            .w_full()
            .child(
                h_flex()
                    .h(Metrics::SECTION_HEADER)
                    .items_center()
                    .px(Space::M)
                    .gap(Space::S)
                    .child(
                        div()
                            .text_size(Type::BODY)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .font_family("JetBrains Mono")
                            .text_size(Type::MICRO)
                            .text_color(c.ink_secondary)
                            .child(SharedString::from(changes.len().to_string())),
                    ),
            )
            .children(git::change_tree(changes).into_iter().map(|row| {
                let indent = px(12. * row.depth as f32);
                let Some(change) = row.change.and_then(|index| changes.get(index)) else {
                    // DESIGN.md > Git: a directory row carries no status. It
                    // gains one trailing action: revert the whole subtree on
                    // the unstaged side, unstage it on the staged side.
                    let prefix = format!("{}/", row.prefix);
                    let items: Vec<(String, bool)> = changes
                        .iter()
                        .filter(|ch| ch.path.starts_with(&prefix))
                        .map(|ch| (ch.path.clone(), ch.kind == ChangeKind::Untracked))
                        .collect();
                    let group = SharedString::from(format!("change-{title}-dir-{}", row.prefix));
                    let key = SharedString::from(format!("{title}-dir-{}", row.prefix));
                    let dir_armed = armed.as_ref() == Some(&key);
                    let dir_root = root.clone();
                    return h_flex()
                        .id(SharedString::from(format!("{title}-dir-{}", row.prefix)))
                        .group(group.clone())
                        .h(Metrics::ROW)
                        .w_full()
                        .items_center()
                        .pl(Space::M + indent)
                        .pr(Space::M)
                        .gap(Space::S)
                        .hover(|this| this.bg(c.hover))
                        .child(
                            Icon::new(IconName::Folder)
                                .xsmall()
                                .flex_shrink_0()
                                .text_color(c.ink_secondary),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(Type::CAPTION)
                                .text_color(c.ink_secondary)
                                .child(SharedString::from(row.label)),
                        )
                        .child(
                            h_flex()
                                .flex_none()
                                .items_center()
                                .gap(px(2.))
                                .opacity(0.)
                                .when(dir_armed, |this| this.opacity(1.))
                                .group_hover(group, |this| this.opacity(1.))
                                .child(if staged {
                                    change_action_button(
                                        SharedString::from(format!("unstage-{title}-{}", row.prefix)),
                                        IconName::Minus,
                                        c.ink_secondary,
                                        c,
                                        cx.listener(move |this, _, _, cx| {
                                            for (path, _) in &items {
                                                let _ = git::unstage(&dir_root, path);
                                            }
                                            this.set_status("unstaged");
                                            this.workspace_mut().refresh_git();
                                            cx.notify();
                                        }),
                                    )
                                    .into_any_element()
                                } else {
                                    let tint = if dir_armed { c.git_deleted } else { c.ink_secondary };
                                    change_action_button(
                                        SharedString::from(format!("revert-{title}-{}", row.prefix)),
                                        IconName::Undo,
                                        tint,
                                        c,
                                        cx.listener(move |this, _, _, cx| {
                                            this.arm_or_discard(
                                                key.clone(),
                                                items.clone(),
                                                dir_root.clone(),
                                                cx,
                                            );
                                        }),
                                    )
                                    .into_any_element()
                                }),
                        )
                        .into_any_element();
                };
                let path = change.path.clone();
                let kind = change.kind;
                let untracked = kind == ChangeKind::Untracked;
                let colour = match kind {
                    ChangeKind::Added => c.git_added,
                    ChangeKind::Deleted => c.git_deleted,
                    ChangeKind::Untracked => c.git_untracked,
                    ChangeKind::Conflicted => c.git_deleted,
                    _ => c.git_modified,
                };
                let name = row.label;
                let glyph = file_glyph(&name, light);
                let group = SharedString::from(format!("change-{title}-{path}"));
                let open_path = path.clone();
                let stage_path = path.clone();
                let stage_root = root.clone();
                let jump_target = root.join(&path);
                let revert_key = SharedString::from(format!("{title}-{path}"));
                let row_armed = armed.as_ref() == Some(&revert_key);
                let revert_items = vec![(path.clone(), untracked)];
                let revert_root = root.clone();
                // Three icons on the unstaged side, two on the staged side, so
                // the action strip is wider when a revert button is present.
                let action_w = if staged { px(50.) } else { px(74.) };

                h_flex()
                    .id(SharedString::from(format!("{title}-{path}")))
                    .group(group.clone())
                    .cursor_pointer()
                    .h(Metrics::ROW)
                    .w_full()
                    .items_center()
                    .pl(Space::M + indent)
                    .pr(Space::M)
                    .gap(Space::S)
                    .hover(|this| this.bg(c.hover))
                    .child(glyph.render(false))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(Type::BODY)
                            .child(SharedString::from(name)),
                    )
                    .child(
                        div()
                            .w(action_w)
                            .h_full()
                            .flex_none()
                            .relative()
                            .child(
                                // The status letter reads while the row is at
                                // rest and fades out once the actions appear.
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .items_center()
                                    .justify_end()
                                    .when(row_armed, |this| this.opacity(0.))
                                    .group_hover(group.clone(), |this| this.opacity(0.))
                                    .child(
                                        div()
                                            .font_family("JetBrains Mono")
                                            .text_size(Type::MICRO)
                                            .text_color(colour)
                                            .child(kind.short()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .absolute()
                                    .inset_0()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(2.))
                                    .opacity(0.)
                                    .when(row_armed, |this| this.opacity(1.))
                                    .group_hover(group, |this| this.opacity(1.))
                                    .child(change_action_button(
                                        SharedString::from(format!("jump-{title}-{path}")),
                                        IconName::ExternalLink,
                                        c.ink_secondary,
                                        c,
                                        cx.listener(move |this, _, _, cx| {
                                            // Keep the row's diff-open from also
                                            // firing for this click.
                                            cx.stop_propagation();
                                            this.workspace_mut()
                                                .open_file(jump_target.clone(), false, cx);
                                            cx.notify();
                                        }),
                                    ))
                                    .when(!staged, |this| {
                                        let tint =
                                            if row_armed { c.git_deleted } else { c.ink_secondary };
                                        this.child(change_action_button(
                                            SharedString::from(format!("revert-{title}-{path}")),
                                            IconName::Undo,
                                            tint,
                                            c,
                                            cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.arm_or_discard(
                                                    revert_key.clone(),
                                                    revert_items.clone(),
                                                    revert_root.clone(),
                                                    cx,
                                                );
                                            }),
                                        ))
                                    })
                                    .child(change_action_button(
                                        SharedString::from(format!("stage-{title}-{path}")),
                                        if staged { IconName::Minus } else { IconName::Plus },
                                        c.ink_secondary,
                                        c,
                                        cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let result = if staged {
                                                git::unstage(&stage_root, &stage_path)
                                            } else {
                                                git::stage(&stage_root, &stage_path)
                                            };
                                            match result {
                                                Ok(()) => this.set_status(if staged {
                                                    "unstaged"
                                                } else {
                                                    "staged"
                                                }),
                                                Err(err) => this.set_status(err),
                                            }
                                            this.workspace_mut().refresh_git();
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.workspace_mut()
                            .open_diff(open_path.clone(), staged, untracked, cx);
                        cx.notify();
                    }))
                    .into_any_element()
            }))
    }

    /// Runs the two-click discard guard for a change row. The first revert
    /// click arms the row and starts a disarm timer; a second click on the
    /// same row within the window runs the destructive discard.
    fn arm_or_discard(
        &mut self,
        key: SharedString,
        items: Vec<(String, bool)>,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.discard_armed.as_ref() == Some(&key) {
            // Second click on the same row: run the destructive discard.
            self.discard_armed = None;
            let mut failure = None;
            for (path, untracked) in &items {
                if let Err(err) = git::discard(&root, path, *untracked) {
                    failure = Some(err);
                    break;
                }
            }
            match failure {
                Some(err) => self.set_status(err),
                None => self.set_status("discarded"),
            }
            self.workspace_mut().refresh_git();
            cx.notify();
        } else {
            // First click: arm, and disarm on a timeout if not confirmed.
            self.discard_armed = Some(key.clone());
            self.set_status("click revert again to discard");
            cx.spawn(async move |shell, cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let _ = shell.update(cx, |shell, cx| {
                    if shell.discard_armed.as_ref() == Some(&key) {
                        shell.discard_armed = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
    }

    /// The inspector pane. The Swift build hosts the Gemma sidecar here; the
    /// POC ports no assistant, so the pane carries the identity and metadata of
    /// the active tab instead and keeps the three-pane width rules exercised.
    pub(crate) fn render_inspector(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::app::workspace::TabKind;

        let c = cx.tokens().c;
        let light = !cx.tokens().dark;
        let workspace = self.workspace();
        let root = workspace.root.clone();

        struct Identity {
            glyph: Glyph,
            title: String,
            caption: String,
            rows: Vec<(&'static str, String)>,
        }

        let identity = workspace.selected_tab().map(|tab| match &tab.kind {
            TabKind::File { path, editor, .. } => {
                let editor = editor.read(cx);
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                Identity {
                    glyph: file_glyph(&name, light),
                    caption: format!("File - {} lines", editor.line_count()),
                    title: name,
                    rows: vec![
                        ("Path", relative),
                        ("Bytes", editor.byte_len().to_string()),
                        ("Language", editor.language().name().to_string()),
                        (
                            "Unsaved",
                            if editor.dirty { "yes" } else { "no" }.to_string(),
                        ),
                    ],
                }
            }
            TabKind::Terminal(view) => Identity {
                glyph: Glyph::Mono(IconName::SquareTerminal, c.ink_secondary),
                title: view.read(cx).session.title.to_string(),
                caption: "Terminal".to_string(),
                rows: vec![
                    ("Shell", "/bin/zsh -l".to_string()),
                    ("Working directory", root.to_string_lossy().to_string()),
                    (
                        "Exited",
                        if view.read(cx).session.exited {
                            "yes"
                        } else {
                            "no"
                        }
                        .to_string(),
                    ),
                ],
            },
            TabKind::Image { path } => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                Identity {
                    glyph: Glyph::Mono(IconName::Frame, c.git_untracked),
                    title: name,
                    caption: "Image".to_string(),
                    rows: vec![("Path", relative), ("Bytes", bytes.to_string())],
                }
            }
            TabKind::Video { path, .. } => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                Identity {
                    glyph: Glyph::Mono(IconName::Eye, c.git_untracked),
                    title: name,
                    caption: "Video".to_string(),
                    rows: vec![("Path", relative), ("Bytes", bytes.to_string())],
                }
            }
            TabKind::ImageDiff { path, old, new } => Identity {
                glyph: Glyph::Mono(IconName::Replace, c.git_modified),
                title: path.clone(),
                caption: "Image diff".to_string(),
                rows: vec![
                    ("HEAD", if old.is_some() { "present" } else { "absent" }.to_string()),
                    (
                        "Working",
                        if new.is_some() { "present" } else { "absent" }.to_string(),
                    ),
                ],
            },
            TabKind::Diff { path, staged, text, .. } => Identity {
                glyph: Glyph::Mono(IconName::Replace, c.git_modified),
                title: path.clone(),
                caption: if *staged { "Staged diff" } else { "Diff" }.to_string(),
                rows: vec![
                    (
                        "Additions",
                        text.lines()
                            .filter(|l| l.starts_with('+'))
                            .count()
                            .to_string(),
                    ),
                    (
                        "Deletions",
                        text.lines()
                            .filter(|l| l.starts_with('-'))
                            .count()
                            .to_string(),
                    ),
                    (
                        "Hunks",
                        text.lines()
                            .filter(|l| l.starts_with("@@"))
                            .count()
                            .to_string(),
                    ),
                ],
            },
        });

        v_flex()
            .size_full()
            .bg(c.sidebar)
            .border_l_1()
            .border_color(c.border)
            .child(
                h_flex()
                    .h(Metrics::PANEL_HEADER)
                    .w_full()
                    .flex_none()
                    .items_center()
                    .gap(Space::S)
                    .px(Space::M)
                    .bg(c.chrome)
                    .border_b_1()
                    .border_color(c.border)
                    .child(
                        identity
                            .as_ref()
                            .map(|i| i.glyph.clone())
                            .unwrap_or(Glyph::Mono(IconName::Inspector, c.ink_secondary))
                            .render(true),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(1.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(Type::LABEL)
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(
                                        identity
                                            .as_ref()
                                            .map(|i| i.title.clone())
                                            .unwrap_or_else(|| "Inspector".into()),
                                    )),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(Type::MICRO)
                                    .text_color(c.ink_secondary)
                                    .child(SharedString::from(
                                        identity
                                            .as_ref()
                                            .map(|i| i.caption.clone())
                                            .unwrap_or_else(|| "No tab selected".into()),
                                    )),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("inspector-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(Space::M)
                    .gap(Space::S)
                    .children(identity.into_iter().flat_map(|identity| identity.rows).map(
                        |(label, value)| {
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(Type::MICRO)
                                        .text_color(c.ink_secondary)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .font_family("JetBrains Mono")
                                        .text_size(Type::CAPTION)
                                        .child(SharedString::from(value)),
                                )
                        },
                    )),
            )
    }
}

/// A path shortened for the repository card: home becomes `~`, and a deep path
/// keeps only its last three components.
fn shorten_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().to_string();
    let home = std::env::var("HOME").unwrap_or_default();
    let text = if !home.is_empty() && text.starts_with(&home) {
        text.replacen(&home, "~", 1)
    } else {
        text
    };
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() <= 4 {
        return text;
    }
    format!(".../{}", parts[parts.len() - 3..].join("/"))
}
