//! Sidebar (Explorer and Git) and the inspector placeholder.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Hsla, IntoElement, ParentElement, SharedString, Styled as _, Window,
    div, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::app::chrome::{file_glyph, folder_glyph, icon_button};
use crate::app::shell::{Shell, SidebarTab};
use crate::app::workspace::{ExplorerInventoryEntry, Workspace, build_explorer_inventory};
use crate::services::fs_tree::Row;
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

#[derive(Clone)]
struct ExplorerRow {
    path: PathBuf,
    name: String,
    depth: usize,
    is_dir: bool,
    expanded: bool,
}

impl From<&Row> for ExplorerRow {
    fn from(row: &Row) -> Self {
        Self {
            path: row.entry.path.clone(),
            name: row.entry.name.clone(),
            depth: row.depth,
            is_dir: row.entry.is_dir,
            expanded: row.expanded,
        }
    }
}

/// Builds a filtered tree without mutating the lazy Explorer. Every matched
/// file brings its ancestor folders, while an empty query returns the exact
/// expanded state that existed before filtering.
fn explorer_rows(workspace: &Workspace, query: &str) -> Vec<ExplorerRow> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return workspace.tree.rows.iter().map(ExplorerRow::from).collect();
    }

    filtered_inventory_rows(
        &workspace.root,
        &workspace.explorer_inventory,
        &workspace.tree.rows,
        &query,
    )
}

fn filtered_inventory_rows(
    root: &Path,
    inventory: &[ExplorerInventoryEntry],
    visible_rows: &[Row],
    query: &str,
) -> Vec<ExplorerRow> {
    let mut paths: HashMap<PathBuf, (bool, bool)> = HashMap::new();
    let mut add_path = |path: &Path, is_dir: bool, expanded: bool| {
        paths
            .entry(path.to_path_buf())
            .and_modify(|value| {
                value.0 |= is_dir;
                value.1 |= expanded;
            })
            .or_insert((is_dir, expanded));
    };

    for entry in inventory.iter().filter(|entry| {
        entry
            .path
            .strip_prefix(root)
            .unwrap_or(&entry.path)
            .to_string_lossy()
            .to_lowercase()
            .contains(query)
    }) {
        let mut ancestors = Vec::new();
        let mut current = entry.path.parent();
        while let Some(dir) = current {
            if dir == root {
                break;
            }
            if !dir.starts_with(root) {
                break;
            }
            ancestors.push(dir.to_path_buf());
            current = dir.parent();
        }
        for ancestor in ancestors.iter().rev() {
            add_path(ancestor, true, true);
        }
        add_path(&entry.path, entry.is_dir, entry.is_dir);
    }

    // Keep the current lazy rows responsive until the background inventory
    // lands. The complete inventory remains separate from the text index.
    for row in visible_rows {
        let relative = row
            .entry
            .path
            .strip_prefix(root)
            .unwrap_or(&row.entry.path)
            .to_string_lossy()
            .to_lowercase();
        if !relative.contains(&query) {
            continue;
        }
        let mut current = row.entry.path.parent();
        while let Some(dir) = current {
            if dir == root || !dir.starts_with(root) {
                break;
            }
            add_path(dir, true, true);
            current = dir.parent();
        }
        add_path(&row.entry.path, row.entry.is_dir, row.expanded);
    }

    let directories: HashSet<PathBuf> = paths
        .iter()
        .filter_map(|(path, (is_dir, _))| is_dir.then_some(path.clone()))
        .collect();
    let mut rows: Vec<_> = paths
        .into_iter()
        .map(|(path, (is_dir, expanded))| {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let depth = relative.components().count().saturating_sub(1);
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            ExplorerRow {
                path,
                name,
                depth,
                is_dir,
                expanded,
            }
        })
        .collect();
    rows.sort_by(|a, b| compare_explorer_paths(&a.path, &b.path, root, &directories));
    rows
}

fn compare_explorer_paths(
    left: &Path,
    right: &Path,
    root: &Path,
    directories: &HashSet<PathBuf>,
) -> Ordering {
    let left_parts: Vec<_> = left
        .strip_prefix(root)
        .unwrap_or(left)
        .components()
        .collect();
    let right_parts: Vec<_> = right
        .strip_prefix(root)
        .unwrap_or(right)
        .components()
        .collect();
    let shared = left_parts
        .iter()
        .zip(&right_parts)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == left_parts.len() || shared == right_parts.len() {
        return left_parts.len().cmp(&right_parts.len());
    }

    let mut left_child = root.to_path_buf();
    let mut right_child = root.to_path_buf();
    for part in left_parts.iter().take(shared + 1) {
        left_child.push(part.as_os_str());
    }
    for part in right_parts.iter().take(shared + 1) {
        right_child.push(part.as_os_str());
    }
    match (
        directories.contains(&left_child),
        directories.contains(&right_child),
    ) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => {
            let left_name = left_parts[shared].as_os_str().to_string_lossy();
            let right_name = right_parts[shared].as_os_str().to_string_lossy();
            left_name
                .to_lowercase()
                .cmp(&right_name.to_lowercase())
                .then_with(|| left_name.cmp(&right_name))
        }
    }
}

impl Shell {
    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let tab = self.sidebar_tab;
        let changed = self.workspace().git.changed_count();
        let (title, icon) = match tab {
            SidebarTab::Explorer => ("Files", IconName::Folder),
            SidebarTab::Git => ("Changes", IconName::Network),
        };

        v_flex()
            .size_full()
            .bg(c.sidebar)
            .border_r_1()
            .border_color(c.border)
            .child(
                h_flex()
                    .h(Metrics::PANEL_HEADER)
                    .w_full()
                    .flex_none()
                    .items_center()
                    .gap(Space::S)
                    .px(Space::M)
                    .bg(crate::app::chrome::chrome_gradient(c))
                    .border_b_1()
                    .border_color(c.border)
                    .child(Icon::new(icon).xsmall().text_color(c.ink_secondary))
                    .child(
                        div()
                            .flex_1()
                            .text_size(Type::HEADLINE * ui_zoom)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .when(tab == SidebarTab::Git && changed > 0, |this| {
                        this.child(
                            div()
                                .font_family("JetBrains Mono")
                                .text_size(Type::MICRO * ui_zoom)
                                .text_color(c.git_modified)
                                .child(SharedString::from(changed.to_string())),
                        )
                    }),
            )
            .child(match tab {
                SidebarTab::Explorer => self.render_explorer(cx).into_any_element(),
                SidebarTab::Git => self.render_git(cx).into_any_element(),
            })
    }

    fn render_explorer(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let entity = cx.entity();
        if let Some(request) = self.workspace_mut().take_explorer_inventory_request() {
            let request_root = request.root.clone();
            let generation = request.generation;
            cx.spawn(async move |shell, cx| {
                let (inventory, cancelled) = cx
                    .background_spawn(async move {
                        let inventory = build_explorer_inventory(&request);
                        let cancelled = request.is_cancelled();
                        (inventory, cancelled)
                    })
                    .await;
                if cancelled {
                    return;
                }
                let _ = shell.update(cx, |shell, cx| {
                    let Some(workspace) = shell
                        .workspaces
                        .iter_mut()
                        .find(|workspace| workspace.root == request_root)
                    else {
                        return;
                    };
                    if workspace.apply_explorer_inventory(generation, inventory) {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let filter = self.workspace().explorer_filter.clone();
        if self.workspace().explorer_filter_subscription.is_none() {
            let subscription = cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.workspace_mut().explorer_filter_subscription = Some(subscription);
        }
        let filter_value = filter.read(cx).value().to_string();
        let filter_active = !filter_value.trim().is_empty();
        let rows = explorer_rows(self.workspace(), &filter_value);
        let count = rows.len();
        let branch = self.workspace().git.branch.clone();
        let selected = self
            .workspace()
            .selected_tab()
            .and_then(|tab| tab.file_path())
            .map(|p| p.to_path_buf());

        v_flex()
            .flex_1()
            .child(
                h_flex()
                    .h(px(44.))
                    .w_full()
                    .flex_none()
                    .items_center()
                    .px(Space::M)
                    .gap(Space::XS)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .items_center()
                            .gap(Space::XS)
                            .px(Space::XS)
                            .font_family("JetBrains Mono")
                            .text_size(Type::MICRO * ui_zoom)
                            .text_color(c.ink_secondary)
                            .child(Icon::new(IconName::Network).xsmall())
                            .child(div().flex_1().truncate().child(SharedString::from(branch))),
                    ),
            )
            .child(
                h_flex()
                    .h(px(44.))
                    .w_full()
                    .flex_none()
                    .items_center()
                    .px(Space::S)
                    .gap(Space::XS)
                    .child(
                        div().id("filter-files").flex_1().h(Metrics::FIELD).child(
                            Input::new(&filter)
                                .small()
                                .cleanable(true)
                                .prefix(Icon::new(IconName::Search).xsmall()),
                        ),
                    )
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
                v_flex()
                    .flex_1()
                    .when(count == 0 && filter_active, |this| {
                        this.child(
                            div()
                                .px(Space::L)
                                .py(Space::M)
                                .text_size(Type::CAPTION * ui_zoom)
                                .text_color(c.ink_secondary)
                                .child("No matching files"),
                        )
                    })
                    .when(count > 0, |this| {
                        this.child(
                            uniform_list("explorer-rows", count, move |range, _window, cx| {
                                let shell = entity.read(cx);
                                let colors = cx.tokens().c;
                                let light = !cx.tokens().dark;
                                let workspace = shell.workspace();
                                range
                                    .map(|index| {
                                        let Some(row) = rows.get(index) else {
                                            return div().into_any_element();
                                        };
                                        let is_dir = row.is_dir;
                                        let expanded = row.expanded;
                                        let path = row.path.clone();
                                        let indent = px(8. + row.depth as f32 * 12.);
                                        let is_selected =
                                            selected.as_deref() == Some(path.as_path());
                                        let ignored = workspace.is_ignored(&path);
                                        let glyph = if is_dir {
                                            folder_glyph(&row.name, expanded, light)
                                        } else {
                                            file_glyph(&row.name, light)
                                        };
                                        let click_path = path.clone();
                                        h_flex()
                                            .id(("tree", index))
                                            .cursor_pointer()
                                            .h(Metrics::TREE_ROW)
                                            .w_full()
                                            .items_center()
                                            .pl(indent)
                                            .pr(Space::S)
                                            .rounded(Radius::ROW)
                                            .when(is_selected, |this| this.bg(colors.selection))
                                            .hover(|this| this.bg(colors.hover))
                                            .child(
                                                div()
                                                    .w(px(14.))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .when(is_dir, |this| {
                                                        this.child(
                                                            Icon::new(if expanded {
                                                                IconName::ChevronDown
                                                            } else {
                                                                IconName::ChevronRight
                                                            })
                                                            .xsmall()
                                                            .text_color(colors.ink_secondary),
                                                        )
                                                    }),
                                            )
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
                                                    .text_size(Type::BODY * ui_zoom)
                                                    .text_color(colors.file_tree_foreground)
                                                    .child(SharedString::from(row.name.clone())),
                                            )
                                            .when(ignored, |this| this.opacity(0.45))
                                            .on_click({
                                                let entity = entity.clone();
                                                move |event: &gpui::ClickEvent, _window, cx| {
                                                    entity.update(cx, |shell, cx| {
                                                        if is_dir {
                                                            if !filter_active {
                                                                shell
                                                                    .workspace_mut()
                                                                    .tree
                                                                    .toggle(&click_path);
                                                            }
                                                        } else {
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
                    }),
            )
    }

    fn render_git(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
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
                        .text_size(Type::BODY * ui_zoom)
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
                                        .text_size(Type::BODY * ui_zoom)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(SharedString::from(name)),
                                ),
                        )
                        .child(
                            div()
                                .font_family("JetBrains Mono")
                                .text_size(Type::MICRO * ui_zoom)
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
                                        .text_size(Type::MICRO * ui_zoom)
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
                                .text_size(Type::BODY * ui_zoom)
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
                                        .text_size(Type::BODY * ui_zoom)
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
                                                .text_size(Type::BODY * ui_zoom)
                                                .child(SharedString::from(commit.subject.clone())),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .font_family("JetBrains Mono")
                                                .text_size(Type::MICRO * ui_zoom)
                                                .text_color(c.ink_secondary)
                                                .child(SharedString::from(
                                                    commit.short_hash.clone(),
                                                )),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .gap(Space::S)
                                        .text_size(Type::MICRO * ui_zoom)
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
        let ui_zoom = self.ui_zoom;
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
                            .text_size(Type::BODY * ui_zoom)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .font_family("JetBrains Mono")
                            .text_size(Type::MICRO * ui_zoom)
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
                                .text_size(Type::CAPTION * ui_zoom)
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
                                        SharedString::from(format!(
                                            "unstage-{title}-{}",
                                            row.prefix
                                        )),
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
                                    let tint = if dir_armed {
                                        c.git_deleted
                                    } else {
                                        c.ink_secondary
                                    };
                                    change_action_button(
                                        SharedString::from(format!(
                                            "revert-{title}-{}",
                                            row.prefix
                                        )),
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
                            .text_size(Type::BODY * ui_zoom)
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
                                            .text_size(Type::MICRO * ui_zoom)
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
                                            this.workspace_mut().open_file(
                                                jump_target.clone(),
                                                false,
                                                cx,
                                            );
                                            cx.notify();
                                        }),
                                    ))
                                    .when(!staged, |this| {
                                        let tint = if row_armed {
                                            c.git_deleted
                                        } else {
                                            c.ink_secondary
                                        };
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
                                        if staged {
                                            IconName::Minus
                                        } else {
                                            IconName::Plus
                                        },
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

    /// Reading context for the active document: outline, local links and Git
    /// provenance. This stays read-only and never becomes an agent surface.
    pub(crate) fn render_inspector(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::app::workspace::{FileMode, PreviewKind, TabKind};

        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let light = !cx.tokens().dark;
        let workspace = self.workspace();
        let root = workspace.root.clone();
        let git_snapshot = workspace.git.clone();

        let markdown_context = workspace.selected_tab().and_then(|tab| match &tab.kind {
            TabKind::File {
                mode: FileMode::Preview,
                preview_view: Some(PreviewKind::Markdown(preview)),
                ..
            } => {
                let view = preview.read(cx);
                Some((preview.clone(), view.outline_items(), view.linked_files()))
            }
            _ => None,
        });

        struct Identity {
            section: &'static str,
            rows: Vec<(&'static str, String)>,
        }

        let identity = workspace.selected_tab().map(|tab| match &tab.kind {
            TabKind::File { path, editor, .. } => {
                let editor = editor.read(cx);
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                Identity {
                    section: "FILE",
                    rows: vec![
                        ("Path", relative),
                        ("Bytes", editor.byte_len().to_string()),
                        ("Lines", editor.line_count().to_string()),
                        ("Language", editor.language().name().to_string()),
                        (
                            "Unsaved",
                            if editor.dirty { "yes" } else { "no" }.to_string(),
                        ),
                    ],
                }
            }
            TabKind::Terminal(view) => Identity {
                section: "TERMINAL",
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
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                Identity {
                    section: "FILE",
                    rows: vec![("Path", relative), ("Bytes", bytes.to_string())],
                }
            }
            TabKind::Video { path, .. } => {
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                Identity {
                    section: "FILE",
                    rows: vec![("Path", relative), ("Bytes", bytes.to_string())],
                }
            }
            TabKind::ImageDiff { old, new, .. } => Identity {
                section: "DIFF",
                rows: vec![
                    (
                        "HEAD",
                        if old.is_some() { "present" } else { "absent" }.to_string(),
                    ),
                    (
                        "Working",
                        if new.is_some() { "present" } else { "absent" }.to_string(),
                    ),
                ],
            },
            TabKind::Diff { text, .. } => Identity {
                section: "DIFF",
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

        let last_commit = git_snapshot.commits.first().cloned();
        let mut authors = Vec::new();
        for commit in &git_snapshot.commits {
            if !authors.contains(&commit.author) {
                authors.push(commit.author.clone());
            }
            if authors.len() == 3 {
                break;
            }
        }

        v_flex()
            .size_full()
            .bg(c.sidebar)
            .border_l_1()
            .border_color(c.border)
            .child(
                v_flex()
                    .id("inspector-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .when_some(markdown_context, |this, (preview, headings, links)| {
                        this.when(!headings.is_empty(), |this| {
                            this.child(
                                v_flex()
                                    .px(Space::M)
                                    .py(Space::M)
                                    .gap(px(2.))
                                    .border_b_1()
                                    .border_color(c.border)
                                    .child(
                                        div()
                                            .pb(Space::S)
                                            .text_size(Type::MICRO * ui_zoom)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(c.ink_secondary)
                                            .child("ON THIS PAGE"),
                                    )
                                    .children(headings.into_iter().enumerate().map(
                                        |(index, (block, level, title, active))| {
                                            let preview = preview.clone();
                                            h_flex()
                                                .id(("context-outline", index))
                                                .h(px(28.))
                                                .w_full()
                                                .cursor_pointer()
                                                .items_center()
                                                .rounded(Radius::ROW)
                                                .pl(px(level.saturating_sub(1) as f32 * 10. + 6.))
                                                .pr(Space::XS)
                                                .text_size(Type::CAPTION * ui_zoom)
                                                .text_color(if active {
                                                    c.accent
                                                } else if level <= 2 {
                                                    c.ink
                                                } else {
                                                    c.ink_secondary
                                                })
                                                .when(active, |this| {
                                                    this.font_weight(gpui::FontWeight::SEMIBOLD)
                                                })
                                                .hover(|this| this.bg(c.raised))
                                                .child(
                                                    div()
                                                        .min_w(px(0.))
                                                        .flex_1()
                                                        .truncate()
                                                        .child(title),
                                                )
                                                .on_click(move |_, _, cx| {
                                                    preview.update(cx, |view, cx| {
                                                        view.scroll_to_block(block, cx);
                                                    });
                                                })
                                        },
                                    )),
                            )
                        })
                        .when(!links.is_empty(), |this| {
                            this.child(
                                v_flex()
                                    .px(Space::M)
                                    .py(Space::M)
                                    .gap(px(2.))
                                    .border_b_1()
                                    .border_color(c.border)
                                    .child(
                                        div()
                                            .pb(Space::S)
                                            .text_size(Type::MICRO * ui_zoom)
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(c.ink_secondary)
                                            .child("LINKED FILES"),
                                    )
                                    .children(links.into_iter().enumerate().map(
                                        |(index, path)| {
                                            let label = path
                                                .strip_prefix(&root)
                                                .unwrap_or(&path)
                                                .to_string_lossy()
                                                .to_string();
                                            let glyph = file_glyph(&label, light);
                                            h_flex()
                                                .id(("context-link", index))
                                                .h(px(30.))
                                                .w_full()
                                                .cursor_pointer()
                                                .items_center()
                                                .gap(Space::S)
                                                .rounded(Radius::ROW)
                                                .px(Space::XS)
                                                .hover(|this| this.bg(c.raised))
                                                .child(glyph.render(false))
                                                .child(
                                                    div()
                                                        .min_w(px(0.))
                                                        .flex_1()
                                                        .truncate()
                                                        .text_size(Type::CAPTION * ui_zoom)
                                                        .child(SharedString::from(label)),
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.workspace_mut().open_file(
                                                        path.clone(),
                                                        false,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }))
                                        },
                                    )),
                            )
                        })
                    })
                    .when_some(last_commit, |this, commit| {
                        this.child(
                            v_flex()
                                .px(Space::M)
                                .py(Space::M)
                                .gap(Space::XS)
                                .border_b_1()
                                .border_color(c.border)
                                .child(
                                    div()
                                        .pb(Space::XS)
                                        .text_size(Type::MICRO * ui_zoom)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(c.ink_secondary)
                                        .child("LAST COMMIT"),
                                )
                                .child(
                                    div()
                                        .text_size(Type::CAPTION * ui_zoom)
                                        .line_height(px(18. * ui_zoom))
                                        .child(SharedString::from(commit.subject)),
                                )
                                .child(
                                    h_flex()
                                        .gap(Space::S)
                                        .font_family("JetBrains Mono")
                                        .text_size(Type::MICRO * ui_zoom)
                                        .text_color(c.ink_secondary)
                                        .child(SharedString::from(commit.short_hash))
                                        .child(SharedString::from(git::relative_time(
                                            commit.seconds,
                                        )))
                                        .child(SharedString::from(commit.author)),
                                ),
                        )
                    })
                    .when(!authors.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .px(Space::M)
                                .py(Space::M)
                                .gap(Space::S)
                                .border_b_1()
                                .border_color(c.border)
                                .child(
                                    div()
                                        .text_size(Type::MICRO * ui_zoom)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(c.ink_secondary)
                                        .child("AUTHORS"),
                                )
                                .children(authors.into_iter().map(|author| {
                                    h_flex()
                                        .items_center()
                                        .gap(Space::S)
                                        .child(
                                            div()
                                                .size(px(22.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded_full()
                                                .bg(c.raised)
                                                .text_size(Type::MICRO * ui_zoom)
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .child(SharedString::from(
                                                    author
                                                        .chars()
                                                        .next()
                                                        .map_or("?".to_string(), |c| {
                                                            c.to_uppercase().to_string()
                                                        }),
                                                )),
                                        )
                                        .child(
                                            div()
                                                .min_w(px(0.))
                                                .flex_1()
                                                .truncate()
                                                .text_size(Type::CAPTION * ui_zoom)
                                                .child(SharedString::from(author)),
                                        )
                                })),
                        )
                    })
                    .when(git_snapshot.is_repo, |this| {
                        let changed = git_snapshot.changed_count();
                        this.child(
                            v_flex()
                                .px(Space::M)
                                .py(Space::M)
                                .gap(Space::S)
                                .border_b_1()
                                .border_color(c.border)
                                .child(
                                    div()
                                        .text_size(Type::MICRO * ui_zoom)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(c.ink_secondary)
                                        .child("STATUS"),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(Space::S)
                                        .child(Icon::new(IconName::Check).xsmall().text_color(
                                            if changed == 0 {
                                                c.git_added
                                            } else {
                                                c.git_modified
                                            },
                                        ))
                                        .child(div().text_size(Type::CAPTION * ui_zoom).child(
                                            SharedString::from(if changed == 0 {
                                                "Working tree clean".to_string()
                                            } else {
                                                format!("{changed} changed files")
                                            }),
                                        )),
                                )
                                .child(
                                    div()
                                        .font_family("JetBrains Mono")
                                        .text_size(Type::MICRO * ui_zoom)
                                        .text_color(c.ink_secondary)
                                        .child(SharedString::from(format!(
                                            "{}  {}",
                                            git_snapshot.branch, git_snapshot.head_short
                                        ))),
                                ),
                        )
                    })
                    .when_some(identity, |this, identity| {
                        this.child(
                            v_flex()
                                .px(Space::M)
                                .py(Space::M)
                                .gap(Space::S)
                                .child(
                                    div()
                                        .text_size(Type::MICRO * ui_zoom)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(c.ink_secondary)
                                        .child(identity.section),
                                )
                                .children(identity.rows.into_iter().map(|(label, value)| {
                                    v_flex()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(Type::MICRO * ui_zoom)
                                                .text_color(c.ink_secondary)
                                                .child(label),
                                        )
                                        .child(
                                            div()
                                                .font_family("JetBrains Mono")
                                                .text_size(Type::CAPTION * ui_zoom)
                                                .child(SharedString::from(value)),
                                        )
                                })),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod explorer_filter_tests {
    use super::*;

    #[test]
    fn filter_finds_inventory_file_outside_the_lazy_tree() {
        let root = PathBuf::from("/repo");
        let inventory = vec![
            ExplorerInventoryEntry {
                path: root.join("ignored"),
                is_dir: true,
            },
            ExplorerInventoryEntry {
                path: root.join("ignored/huge.bin"),
                is_dir: false,
            },
        ];

        let rows = filtered_inventory_rows(&root, &inventory, &[], "huge");
        let paths: Vec<_> = rows.iter().map(|row| row.path.clone()).collect();
        assert_eq!(
            paths,
            vec![root.join("ignored"), root.join("ignored/huge.bin")]
        );
        assert!(rows.first().is_some_and(|row| row.expanded));
    }

    #[test]
    fn case_insensitive_sort_has_a_stable_tie_break() {
        let root = PathBuf::from("/repo");
        let directories = HashSet::new();
        let left = root.join("README.md");
        let right = root.join("Readme.md");

        let ordering = compare_explorer_paths(&left, &right, &root, &directories);
        assert_ne!(ordering, Ordering::Equal);
        assert_eq!(
            ordering,
            compare_explorer_paths(&right, &left, &root, &directories).reverse()
        );
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
