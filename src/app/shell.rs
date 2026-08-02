//! Application shell: workspace rail, three-pane split, status bar.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding,
    ParentElement, Pixels, Render, SharedString, Styled as _, Window, actions, div,
    linear_color_stop, linear_gradient, px,
};
use gpui_component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::app::chrome::{icon_button, project_menu, title_bar_drag_strip};
use crate::app::overlays::OverlayState;
use crate::app::workspace::Workspace;
use crate::theme::{self, ActiveTokens as _, LayoutMode, Metrics, Radius, Space, Type};

actions!(
    atelier,
    [
        ToggleSidebar,
        ToggleInspector,
        ToggleSidebarTab,
        ToggleFocusMode,
        QuickOpen,
        CommandPalette,
        SearchAllFiles,
        NewTerminal,
        CloseTab,
        SaveFile,
        TogglePreview,
        ZoomIn,
        ZoomOut,
        ToggleAppearance,
        CancelOverlay,
        AddWorkspace,
        NextWorkspace,
        Workspace1,
        Workspace2,
        Workspace3,
        Workspace4,
        Workspace5,
        Workspace6,
        Workspace7,
        Workspace8,
        Workspace9,
    ]
);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Explorer,
    Git,
}

pub struct Shell {
    pub workspaces: Vec<Workspace>,
    pub active: usize,
    // DESIGN.md: panel visibility and the selected sidebar tab are window
    // state, shared by every workspace.
    pub shows_sidebar: bool,
    pub shows_inspector: bool,
    pub sidebar_tab: SidebarTab,
    /// `DESIGN.md`: the focus control is the master side-panel visibility
    /// control. It hides every panel together and restores the layout's
    /// complete default set, never a partial snapshot.
    pub focus_mode: bool,
    pub layout: LayoutMode,
    pub zoom: f32,
    pub dark: bool,
    pub split: Entity<ResizableState>,
    pub overlay: OverlayState,
    pub status: Option<SharedString>,
    focus: FocusHandle,
}

impl Shell {
    pub fn build(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let root = resolve_root();
        eprintln!("rustelier: workspace root {}", root.display());

        let split = cx.new(|_| ResizableState::default());
        let workspace = Workspace::open(root, window, cx);
        let dark = gpui_component::Theme::global(cx).is_dark();

        let shell = cx.new(|cx| Self {
            workspaces: vec![workspace],
            active: 0,
            shows_sidebar: true,
            shows_inspector: false,
            sidebar_tab: SidebarTab::Explorer,
            focus_mode: false,
            layout: LayoutMode::Standard,
            zoom: 1.0,
            dark,
            split,
            overlay: OverlayState::default(),
            status: None,
            focus: cx.focus_handle(),
        });
        shell.update(cx, |shell, cx| shell.scan_workspace(0, cx));
        shell
    }

    /// Builds the file index and the Git snapshot for one workspace off the
    /// main thread.
    ///
    /// Both walk the whole tree. Run inline they block the `open_window`
    /// callback, so a large root means the window is never created at all.
    fn scan_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(root) = self.workspaces.get(index).map(|w| w.root.clone()) else {
            return;
        };
        cx.spawn(async move |shell, cx| {
            let (files, git) = cx
                .background_spawn(async move {
                    (
                        crate::services::file_index::build(&root),
                        crate::services::git::snapshot(&root),
                    )
                })
                .await;
            shell
                .update(cx, |shell, cx| {
                    if let Some(workspace) = shell.workspaces.get_mut(index) {
                        workspace.apply_scan(files, git);
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    pub fn workspace_mut(&mut self) -> &mut Workspace {
        let index = self.active;
        &mut self.workspaces[index]
    }

    pub fn set_status(&mut self, text: impl Into<SharedString>) {
        self.status = Some(text.into());
    }

    fn select_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.workspaces.len() && index != self.active {
            self.active = index;
            self.workspace_mut().refresh_git();
            cx.notify();
        }
    }

    /// Opens the native folder picker, then opens the chosen folder as a
    /// workspace.
    ///
    /// `App::prompt_for_paths` cannot say which folder the panel starts in, so
    /// the panel comes from `rfd`, which can. The picker runs off the main
    /// thread's current pass and reports back through the window context.
    fn add_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.picker_start_directory();

        cx.spawn_in(window, async move |shell, cx| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Open Workspace")
                .set_directory(&start)
                .pick_folder()
                .await;
            let Some(folder) = picked else {
                return;
            };
            let path = folder.path().to_path_buf();
            let _ = shell.update_in(cx, |shell, window, cx| {
                shell.open_workspace_folder(path, window, cx)
            });
        })
        .detach();
    }

    /// Where the folder picker opens. `~/Projects` is the working habit this
    /// POC is measured against; the last folder opened wins once there is one,
    /// and the home directory is the fallback.
    fn picker_start_directory(&self) -> PathBuf {
        let last_parent = self
            .workspaces
            .last()
            .and_then(|workspace| workspace.root.parent().map(|p| p.to_path_buf()))
            .filter(|path| path.is_dir());
        if let Some(parent) = last_parent {
            return parent;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let projects = home.as_ref().map(|home| home.join("Projects"));
        match (projects, home) {
            (Some(projects), _) if projects.is_dir() => projects,
            (_, Some(home)) => home,
            _ => PathBuf::from("/"),
        }
    }

    /// Adds `path` as a workspace, or selects it when it is already open.
    fn open_workspace_folder(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = crate::services::git::repo_root(&path);
        if let Some(index) = self
            .workspaces
            .iter()
            .position(|workspace| workspace.root == root)
        {
            self.select_workspace(index, cx);
            self.set_status(format!("{} is already open", root.display()));
            cx.notify();
            return;
        }

        let workspace = Workspace::open(root, window, cx);
        self.workspaces.push(workspace);
        self.active = self.workspaces.len() - 1;
        self.scan_workspace(self.active, cx);
        cx.notify();
    }

    pub(crate) fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shows_sidebar = !self.shows_sidebar;
        cx.notify();
    }

    pub(crate) fn on_toggle_inspector(
        &mut self,
        _: &ToggleInspector,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shows_inspector = !self.shows_inspector;
        cx.notify();
    }

    pub(crate) fn on_toggle_sidebar_tab(
        &mut self,
        _: &ToggleSidebarTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_tab = match self.sidebar_tab {
            SidebarTab::Explorer => SidebarTab::Git,
            SidebarTab::Git => SidebarTab::Explorer,
        };
        cx.notify();
    }

    pub(crate) fn on_toggle_focus_mode(
        &mut self,
        _: &ToggleFocusMode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_mode = !self.focus_mode;
        if self.focus_mode {
            self.shows_sidebar = false;
            self.shows_inspector = false;
        } else {
            self.shows_sidebar = true;
            self.shows_inspector = self.layout.allows_inspector();
        }
        cx.notify();
    }

    pub(crate) fn on_new_terminal(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_mut().open_terminal(window, cx);
        cx.notify();
    }

    fn on_close_tab(&mut self, _: &CloseTab, _: &mut Window, cx: &mut Context<Self>) {
        let index = self.workspace().selected;
        self.workspace_mut().close_tab(index);
        cx.notify();
    }

    pub(crate) fn on_save(&mut self, _: &SaveFile, _: &mut Window, cx: &mut Context<Self>) {
        use crate::app::workspace::TabKind;
        let Some(tab) = self.workspace().selected_tab() else {
            return;
        };
        let TabKind::File { editor, .. } = &tab.kind else {
            return;
        };
        let editor = editor.clone();
        let result = editor.update(cx, |editor, _| editor.save());
        match result {
            Ok(()) => self.set_status("saved"),
            Err(err) => self.set_status(format!("save failed: {err}")),
        }
        self.workspace_mut().refresh_git();
        cx.notify();
    }

    pub(crate) fn on_toggle_preview(
        &mut self,
        _: &TogglePreview,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_mut().toggle_mode();
        cx.notify();
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = (self.zoom + 0.1).min(2.0);
        cx.notify();
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = (self.zoom - 0.1).max(0.8);
        cx.notify();
    }

    pub(crate) fn on_toggle_appearance(
        &mut self,
        _: &ToggleAppearance,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dark = !self.dark;
        let dark = self.dark;
        theme::set_dark(dark, cx);
        cx.notify();
    }

    pub(crate) fn on_add_workspace(
        &mut self,
        _: &AddWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_workspace(window, cx);
    }

    fn on_next_workspace(&mut self, _: &NextWorkspace, _: &mut Window, cx: &mut Context<Self>) {
        let next = (self.active + 1) % self.workspaces.len();
        self.select_workspace(next, cx);
    }

    /// `DESIGN.md` > Workspace Rail. Fixed 176 points, dark in both
    /// appearances, one graphite-to-petrol gradient as its only depth effect.
    fn render_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let active = self.active;
        let total = self.workspaces.len();

        v_flex()
            .w(Metrics::RAIL_WIDTH)
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(c.rail_border)
            .bg(linear_gradient(
                180.,
                linear_color_stop(c.rail_top, 0.),
                linear_color_stop(c.rail_bottom, 1.),
            ))
            .child(
                h_flex()
                    .h(Metrics::PANEL_HEADER)
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px(Space::M)
                    .child(
                        div()
                            .text_size(Type::LABEL)
                            .text_color(c.rail_secondary)
                            .child("Workspaces"),
                    )
                    .child(
                        div()
                            .font_family("JetBrains Mono")
                            .text_size(Type::MICRO)
                            .text_color(c.rail_secondary)
                            .child(SharedString::from(total.to_string())),
                    ),
            )
            .child(
                v_flex()
                    .id("rail-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(Space::S)
                    .gap(Metrics::RAIL_ITEM_GAP)
                    .children(
                        self.workspaces
                            .iter()
                            .enumerate()
                            .map(|(index, workspace)| {
                                let selected = index == active;
                                let changed = workspace.git.changed_count();
                                v_flex()
                                    .id(("workspace", index))
                                    .cursor_pointer()
                                    .h(Metrics::RAIL_ITEM_HEIGHT)
                                    .justify_center()
                                    .gap(px(1.))
                                    .px(Space::S)
                                    .rounded(Radius::ROW)
                                    .when(selected, |this| {
                                        this.bg(c.rail_selection)
                                            .border_1()
                                            .border_color(c.rail_border)
                                    })
                                    .when(!selected, |this| {
                                        this.border_1().border_color(gpui::transparent_black())
                                    })
                                    .hover(|this| this.bg(c.rail_hover))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap(Space::XS)
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .truncate()
                                                    .text_size(Type::BODY)
                                                    .text_color(c.rail_foreground)
                                                    .when(selected, |this| {
                                                        this.font_weight(gpui::FontWeight::SEMIBOLD)
                                                    })
                                                    .child(SharedString::from(
                                                        workspace.name.clone(),
                                                    )),
                                            )
                                            .when(changed > 0, |this| {
                                                this.child(
                                                    div()
                                                        .flex_none()
                                                        .font_family("JetBrains Mono")
                                                        .text_size(Type::MICRO)
                                                        .text_color(c.rail_foreground)
                                                        .child(SharedString::from(
                                                            changed.to_string(),
                                                        )),
                                                )
                                            }),
                                    )
                                    .when(index < 9, |this| {
                                        this.child(
                                            div()
                                                .font_family("JetBrains Mono")
                                                .text_size(Type::MICRO)
                                                .text_color(c.rail_secondary)
                                                .child(SharedString::from(format!(
                                                    "⌘{}",
                                                    index + 1
                                                ))),
                                        )
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_workspace(index, cx)
                                    }))
                            }),
                    ),
            )
            .child(
                h_flex()
                    .id("add-workspace")
                    .cursor_pointer()
                    .m(Space::S)
                    .h(Metrics::CONTROL)
                    .items_center()
                    .gap(Space::XS)
                    .px(Space::S)
                    .rounded(Radius::ROW)
                    .hover(|this| this.bg(c.rail_hover))
                    .child(
                        Icon::new(IconName::Plus)
                            .xsmall()
                            .text_color(c.rail_secondary),
                    )
                    .child(
                        div()
                            .text_size(Type::LABEL)
                            .text_color(c.rail_foreground)
                            .child("Add Workspace"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.add_workspace(window, cx))),
            )
    }

    /// `DESIGN.md` > Workspace Chrome. The unified compact toolbar: sidebar
    /// toggle in navigation placement, project commands in the principal menu
    /// centred against the whole window, view controls in primary actions.
    fn render_toolbar(&mut self, title_inset: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let workspace = self.workspace();
        let name = workspace.name.clone();
        let path = workspace.root.to_string_lossy().to_string();
        let shows_sidebar = self.shows_sidebar;
        let shows_inspector = self.shows_inspector;
        let dark = self.dark;

        v_flex()
            .w_full()
            .flex_none()
            .bg(c.chrome)
            .border_b_1()
            .border_color(c.border)
            .child(title_bar_drag_strip(title_inset))
            .child(
                h_flex()
                    .h(Metrics::TAB_BAR)
                    .w_full()
                    .items_center()
                    .px(Space::S)
                    // The traffic lights sit over the leading edge, so the
                    // first control starts clear of them.
                    .pl(px(84.))
                    .child(
                        h_flex()
                            .flex_1()
                            .items_center()
                            .gap(Space::XS)
                            .child(icon_button(
                                "toggle-sidebar",
                                IconName::PanelLeft,
                                shows_sidebar,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_sidebar(&ToggleSidebar, window, cx)
                                }),
                            )),
                    )
                    .child(project_menu(&name, &path, c))
                    .child(
                        h_flex()
                            .flex_1()
                            .items_center()
                            .justify_end()
                            .gap(Space::XS)
                            .child(icon_button(
                                "toggle-appearance",
                                if dark { IconName::Moon } else { IconName::Sun },
                                false,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_appearance(&ToggleAppearance, window, cx)
                                }),
                            ))
                            .child(icon_button(
                                "quick-open",
                                IconName::Search,
                                false,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_quick_open(&QuickOpen, window, cx)
                                }),
                            ))
                            .child(icon_button(
                                "focus-mode",
                                IconName::Maximize,
                                self.focus_mode,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_focus_mode(&ToggleFocusMode, window, cx)
                                }),
                            ))
                            .child(icon_button(
                                "toggle-inspector",
                                IconName::PanelRight,
                                shows_inspector,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_inspector(&ToggleInspector, window, cx)
                                }),
                            )),
                    ),
            )
    }

    /// `DESIGN.md` > Workspace Chrome: branch, focus state, token estimate and
    /// zoom in the 26-point status bar.
    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::app::workspace::TabKind;

        let c = cx.tokens().c;
        let workspace = self.workspace();
        let branch = if workspace.git.is_repo {
            workspace.git.branch.clone()
        } else {
            "no repository".to_string()
        };
        let head = workspace.git.head_short.clone();
        let changed = workspace.git.changed_count();
        let layout = match self.layout {
            LayoutMode::Compact => "compact",
            LayoutMode::Standard => "standard",
            LayoutMode::Wide => "wide",
        };
        // The estimate is deliberately rough; exact tokenisation depends on the
        // model, which is why DESIGN.md prefixes it with a tilde.
        let tokens = match workspace.selected_tab().map(|tab| &tab.kind) {
            Some(TabKind::File { editor, .. }) => Some(editor.read(cx).byte_len() / 4),
            _ => None,
        };
        let status = self.status.clone();

        h_flex()
            .h(Metrics::STATUS_BAR)
            .w_full()
            .flex_none()
            .items_center()
            .px(Space::M)
            .gap(Space::M)
            .bg(c.chrome)
            .border_t_1()
            .border_color(c.border)
            .text_size(Type::MICRO)
            .text_color(c.ink_secondary)
            .child(
                h_flex()
                    .id("branch")
                    .cursor_pointer()
                    .items_center()
                    .gap(Space::XS)
                    .child(Icon::new(IconName::Network).xsmall())
                    .child(div().child(SharedString::from(branch)))
                    .when(!head.is_empty(), |this| {
                        this.child(
                            div()
                                .font_family("JetBrains Mono")
                                .text_color(c.ink_secondary.opacity(0.7))
                                .child(SharedString::from(head)),
                        )
                    }),
            )
            .when(changed > 0, |this| {
                this.child(
                    div()
                        .text_color(c.git_modified)
                        .child(SharedString::from(format!("{changed} changed"))),
                )
            })
            .child(div().flex_1())
            .when_some(status, |this, text| this.child(div().child(text)))
            .child(div().child(SharedString::from(layout)))
            .when_some(tokens, |this, tokens| {
                this.child(
                    div()
                        .font_family("JetBrains Mono")
                        .child(SharedString::from(format!("~{tokens} tokens"))),
                )
            })
            .child(
                div()
                    .id("zoom")
                    .cursor_pointer()
                    .font_family("JetBrains Mono")
                    .child(SharedString::from(format!("{:.0}%", self.zoom * 100.)))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.on_zoom_in(&ZoomIn, window, cx)),
                    ),
            )
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let title_inset = theme::title_bar_inset(window);
        let width = f32::from(window.viewport_size().width) - f32::from(Metrics::RAIL_WIDTH);
        let layout = LayoutMode::for_width(width);
        if layout != self.layout {
            // Layout mode is derived, never a source of truth, so reading it
            // here cannot start a mutation loop during the layout pass.
            self.layout = layout;
        }

        let show_sidebar = self.shows_sidebar && layout.allows_sidebar() && !self.focus_mode;
        let show_inspector = self.shows_inspector && layout.allows_inspector() && !self.focus_mode;

        div()
            .id("shell")
            .track_focus(&self.focus)
            .key_context("Shell")
            .size_full()
            .bg(c.canvas)
            .text_color(c.ink)
            .text_size(Type::BODY)
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_inspector))
            .on_action(cx.listener(Self::on_toggle_sidebar_tab))
            .on_action(cx.listener(Self::on_toggle_focus_mode))
            .on_action(cx.listener(Self::on_new_terminal))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_toggle_appearance))
            .on_action(cx.listener(Self::on_add_workspace))
            .on_action(cx.listener(Self::on_next_workspace))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_command_palette))
            .on_action(cx.listener(Self::on_search_all))
            .on_action(cx.listener(Self::on_cancel_overlay))
            // The query field claims Escape in its own key context, handles it
            // and then propagates the component kit's action rather than the
            // key. Closing the overlay means listening for that action too.
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                    this.on_cancel_overlay(&CancelOverlay, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Workspace1, _, cx| this.select_workspace(0, cx)))
            .on_action(cx.listener(|this, _: &Workspace2, _, cx| this.select_workspace(1, cx)))
            .on_action(cx.listener(|this, _: &Workspace3, _, cx| this.select_workspace(2, cx)))
            .on_action(cx.listener(|this, _: &Workspace4, _, cx| this.select_workspace(3, cx)))
            .on_action(cx.listener(|this, _: &Workspace5, _, cx| this.select_workspace(4, cx)))
            .on_action(cx.listener(|this, _: &Workspace6, _, cx| this.select_workspace(5, cx)))
            .on_action(cx.listener(|this, _: &Workspace7, _, cx| this.select_workspace(6, cx)))
            .on_action(cx.listener(|this, _: &Workspace8, _, cx| this.select_workspace(7, cx)))
            .on_action(cx.listener(|this, _: &Workspace9, _, cx| this.select_workspace(8, cx)))
            .child(
                v_flex()
                    .size_full()
                    .child(self.render_toolbar(title_inset, cx))
                    .child(
                        h_flex()
                            .flex_1()
                            .w_full()
                            .child(self.render_rail(cx))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .h_full()
                                    .relative()
                                    .child(
                                        div()
                                            .flex_1()
                                            .w_full()
                                            .min_h(px(0.))
                                            .overflow_hidden()
                                            .child(
                                                h_resizable("workspace-split")
                                                    .with_state(&self.split)
                                                    .child(
                                                        resizable_panel()
                                                            .size(Metrics::SIDEBAR_IDEAL)
                                                            .size_range(
                                                                Metrics::SIDEBAR_MIN
                                                                    ..Metrics::SIDEBAR_MAX,
                                                            )
                                                            .visible(show_sidebar)
                                                            .child(self.render_sidebar(cx)),
                                                    )
                                                    .child(
                                                        resizable_panel()
                                                            .size_range(
                                                                Metrics::CENTER_MIN..px(4000.),
                                                            )
                                                            .child(self.render_center(cx)),
                                                    )
                                                    .child(
                                                        resizable_panel()
                                                            .size(Metrics::INSPECTOR_IDEAL)
                                                            .size_range(
                                                                Metrics::INSPECTOR_MIN
                                                                    ..Metrics::INSPECTOR_MAX,
                                                            )
                                                            .visible(show_inspector)
                                                            .child(self.render_inspector(cx)),
                                                    ),
                                            ),
                                    )
                                    .child(self.render_status_bar(cx))
                                    .children(self.render_overlay(window, cx)),
                            ),
                    ),
            )
    }
}

/// Picks the workspace root for this launch.
///
/// LaunchServices sets the working directory to `/`, so the `current_dir`
/// fallback would otherwise make the whole filesystem the workspace. A
/// directory with no parent is the filesystem root, which is never a
/// workspace; the home directory is what a Finder or Dock launch lands on.
fn resolve_root() -> PathBuf {
    let explicit = std::env::args().nth(2).map(PathBuf::from);
    let current = std::env::current_dir().ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);

    [explicit, current, home]
        .into_iter()
        .flatten()
        .find_map(plausible_root)
        .unwrap_or_else(std::env::temp_dir)
}

/// `None` for anything that is not a real directory below the filesystem root.
fn plausible_root(path: PathBuf) -> Option<PathBuf> {
    let path = std::fs::canonicalize(path).ok()?;
    if !path.is_dir() || path.parent().is_none() {
        return None;
    }
    Some(path)
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-r", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-t", ToggleInspector, None),
        KeyBinding::new("cmd-e", ToggleSidebarTab, None),
        KeyBinding::new("cmd-shift-e", ToggleFocusMode, None),
        KeyBinding::new("cmd-p", QuickOpen, None),
        KeyBinding::new("cmd-shift-p", CommandPalette, None),
        KeyBinding::new("cmd-shift-f", SearchAllFiles, None),
        KeyBinding::new("cmd-t", NewTerminal, None),
        KeyBinding::new("cmd-q", CloseTab, None),
        KeyBinding::new("cmd-s", SaveFile, None),
        KeyBinding::new("cmd-d", TogglePreview, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", AddWorkspace, None),
        KeyBinding::new("cmd-`", NextWorkspace, None),
        KeyBinding::new("escape", CancelOverlay, None),
        // A plain Escape never arrives while a text field holds focus: macOS
        // hands non-printing keys to the input method first, and GPUI drops the
        // key when the input method reports it handled. Adding Command sets the
        // platform modifier, which skips that path entirely.
        KeyBinding::new("cmd-escape", CancelOverlay, None),
        KeyBinding::new("cmd-1", Workspace1, None),
        KeyBinding::new("cmd-2", Workspace2, None),
        KeyBinding::new("cmd-3", Workspace3, None),
        KeyBinding::new("cmd-4", Workspace4, None),
        KeyBinding::new("cmd-5", Workspace5, None),
        KeyBinding::new("cmd-6", Workspace6, None),
        KeyBinding::new("cmd-7", Workspace7, None),
        KeyBinding::new("cmd-8", Workspace8, None),
        KeyBinding::new("cmd-9", Workspace9, None),
    ]);
}
