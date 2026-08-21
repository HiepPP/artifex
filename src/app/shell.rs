//! Application shell: workspace rail, three-pane split, status bar.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, EntityId, FocusHandle, Focusable,
    IntoElement, KeyBinding, ParentElement, Pixels, Render, SharedString, Styled as _, Window,
    actions, div, linear_color_stop, linear_gradient, px,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_component::{Icon, IconName, Sizable as _, h_flex, v_flex};

use crate::app::chrome::{rail_count_badge, toolbar_icon_button};
use crate::app::editor::EditorView;
use crate::app::overlays::OverlayState;
use crate::app::workspace::{FileMode, PreviewKind, TabKind, Workspace, is_html_path};
use crate::services::git;
use crate::services::session;
use crate::services::settings;
use crate::services::watch::{RootChange, WatchHub};
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
        ToggleWrap,
        ToggleTabCloseButtons,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        UiZoomIn,
        UiZoomOut,
        ResetUiZoom,
        ToggleAppearance,
        CancelOverlay,
        AddWorkspace,
        NextWorkspace,
        CommitPush,
        RevealInExplorer,
        NavigateBack,
        NavigateForward,
        FindInFile,
        FindReplace,
        FindNext,
        FindPrev,
        InsertFileReference,
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

#[derive(Clone, PartialEq, Eq)]
struct ActiveSurface {
    root: PathBuf,
    tab_id: usize,
    mode: Option<FileMode>,
    entity: Option<EntityId>,
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
    pub ui_zoom: f32,
    pub dark: bool,
    pub word_wrap: bool,
    pub shows_tab_close_buttons: bool,
    pub split: Entity<ResizableState>,
    pub overlay: OverlayState,
    pub status: Option<SharedString>,
    /// The change-row key currently armed for a one-click-away discard. The
    /// Git panel shows a first revert click as armed, and only a second click
    /// on the same row runs the destructive discard. Cleared on a timeout.
    pub(crate) discard_armed: Option<SharedString>,
    /// `None` when the platform refused a watcher. The Explorer refresh control
    /// and `Workspace: Rebuild File Index` stay the manual fallback.
    watch: Option<WatchHub>,
    focus: FocusHandle,
    active_surface: Option<ActiveSurface>,
    /// Last session snapshot written to disk. Render compares against it so
    /// only a real state change costs a write.
    last_session: Option<session::SessionState>,
    /// Last durable preference snapshot scheduled for an atomic write.
    last_settings: Option<settings::SettingsState>,
    pub(crate) terminal_event_sources: HashSet<EntityId>,
    pub(crate) markdown_event_sources: HashSet<EntityId>,
}

impl Shell {
    pub fn build(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let split = cx.new(|_| ResizableState::default());
        let state = session::load();
        let preferences = settings::load().unwrap_or_else(|| {
            let mut preferences = settings::SettingsState::default();
            if let Some(state) = state.as_ref() {
                preferences.shows_sidebar = state.legacy_shows_sidebar;
                preferences.shows_inspector = state.legacy_shows_inspector;
                preferences.content_zoom = state.legacy_zoom.clamp(0.8, 2.0);
                preferences.dark = state.legacy_dark;
            }
            preferences
        });
        settings::set_word_wrap(preferences.word_wrap, cx);
        let (workspaces, active) = Self::restore_session(state.as_ref(), window, cx);

        let dark = match preferences.dark {
            Some(dark) => {
                theme::set_dark(dark, cx);
                dark
            }
            None => gpui_component::Theme::global(cx).is_dark(),
        };
        let shows_sidebar = preferences.shows_sidebar;
        let shows_inspector = preferences.shows_inspector;
        let sidebar_tab = match state.as_ref().map(|state| state.sidebar_tab) {
            Some(session::SidebarTabState::Git) => SidebarTab::Git,
            _ => SidebarTab::Explorer,
        };
        let zoom = preferences.content_zoom.clamp(0.8, 2.0);
        let ui_zoom = preferences.ui_zoom.clamp(0.8, 1.4);
        let shows_tab_close_buttons = preferences.shows_tab_close_buttons;
        theme::set_editor_zoom(zoom, cx);
        theme::set_ui_zoom(ui_zoom, cx);

        let shell = cx.new(|cx| Self {
            workspaces,
            active,
            shows_sidebar,
            shows_inspector,
            sidebar_tab,
            focus_mode: false,
            layout: LayoutMode::Standard,
            zoom,
            ui_zoom,
            dark,
            word_wrap: preferences.word_wrap,
            shows_tab_close_buttons,
            split,
            overlay: OverlayState::default(),
            status: None,
            discard_armed: None,
            watch: WatchHub::new(),
            focus: cx.focus_handle(),
            active_surface: None,
            last_session: None,
            last_settings: None,
            terminal_event_sources: HashSet::new(),
            markdown_event_sources: HashSet::new(),
        });
        shell.update(cx, |shell, cx| {
            shell.observe_watch(cx);
            for index in 0..shell.workspaces.len() {
                shell.watch_workspace(index);
                shell.scan_workspace(index, true, true, cx);
            }
        });
        let focus = shell.read(cx).focus.clone();
        window.focus(&focus, cx);
        shell
    }

    /// Reopens the workspaces and file tabs from the previous session, per
    /// `DESIGN.md` > Session Persistence. A missing session file, a deleted
    /// root, or a deleted file each degrade silently: the entry is skipped,
    /// and with nothing left the shell falls back to `resolve_root`.
    fn restore_session(
        state: Option<&session::SessionState>,
        window: &mut Window,
        cx: &mut App,
    ) -> (Vec<Workspace>, usize) {
        let mut workspaces: Vec<Workspace> = Vec::new();
        let mut active = 0;

        if let Some(state) = state {
            active = state.active;
            for saved in &state.workspaces {
                if !saved.root.is_dir() {
                    continue;
                }
                let mut workspace = Workspace::open(saved.root.clone(), window, cx);
                // `repo_root` can fold two saved paths into one root.
                if workspaces.iter().any(|open| open.root == workspace.root) {
                    continue;
                }
                for file in &saved.files {
                    if !file.path.is_file() {
                        continue;
                    }
                    workspace.open_file(file.path.clone(), false, cx);
                    if let Some(tab) = workspace.tabs.last_mut()
                        && let TabKind::File {
                            mode, preview_view, ..
                        } = &mut tab.kind
                    {
                        // An HTML preview restores by mode alone; its webview
                        // is created lazily on the first Preview render.
                        *mode = if file.preview
                            && (preview_view.is_some() || is_html_path(&file.path))
                        {
                            FileMode::Preview
                        } else {
                            FileMode::Source
                        };
                    }
                }
                let restored = saved.selected.as_ref().and_then(|path| {
                    workspace
                        .tabs
                        .iter()
                        .position(|tab| tab.file_path() == Some(path.as_path()))
                });
                let terminal = workspace.tabs.iter().position(|tab| tab.is_terminal());
                if let Some(index) = restored.or(terminal) {
                    workspace.selected = index;
                }
                workspaces.push(workspace);
            }
        }

        if workspaces.is_empty() {
            let root = resolve_root();
            eprintln!("artifex: workspace root {}", root.display());
            workspaces.push(Workspace::open(root, window, cx));
        }
        let active = active.min(workspaces.len() - 1);
        (workspaces, active)
    }

    /// Writes the session to disk when it differs from the last write.
    ///
    /// Called from `render`, so every state change that notifies is caught
    /// without threading a persist call through each mutation site. The
    /// snapshot is a handful of paths, so building it per frame is cheap.
    fn persist_session(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.session_snapshot();
        if self.last_session.as_ref() == Some(&snapshot) {
            return;
        }
        self.last_session = Some(snapshot.clone());
        cx.background_spawn(async move {
            session::save(&snapshot);
        })
        .detach();
    }

    /// Writes durable user preferences independently from workspace restore.
    fn persist_settings(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.settings_snapshot();
        if self.last_settings.as_ref() == Some(&snapshot) {
            return;
        }
        self.last_settings = Some(snapshot.clone());
        cx.background_spawn(async move {
            settings::save(&snapshot);
        })
        .detach();
    }

    fn settings_snapshot(&self) -> settings::SettingsState {
        let mut state = settings::SettingsState::default();
        state.shows_sidebar = self.shows_sidebar;
        state.shows_inspector = self.shows_inspector;
        state.content_zoom = self.zoom;
        state.ui_zoom = self.ui_zoom;
        state.dark = Some(self.dark);
        state.word_wrap = self.word_wrap;
        state.shows_tab_close_buttons = self.shows_tab_close_buttons;
        state
    }

    fn session_snapshot(&self) -> session::SessionState {
        let workspaces = self
            .workspaces
            .iter()
            .map(|workspace| {
                let files = workspace
                    .tabs
                    .iter()
                    .filter_map(|tab| match &tab.kind {
                        TabKind::File { path, mode, .. } => Some(session::FileTabState {
                            path: path.clone(),
                            preview: matches!(mode, FileMode::Preview),
                        }),
                        TabKind::Image { path } | TabKind::Video { path, .. } => {
                            Some(session::FileTabState {
                                path: path.clone(),
                                preview: false,
                            })
                        }
                        _ => None,
                    })
                    .collect();
                let selected = workspace
                    .selected_tab()
                    .and_then(|tab| tab.file_path().map(Path::to_path_buf));
                session::WorkspaceState {
                    root: workspace.root.clone(),
                    selected,
                    files,
                }
            })
            .collect();
        let mut state = session::SessionState::new(self.active, workspaces);
        state.sidebar_tab = match self.sidebar_tab {
            SidebarTab::Explorer => session::SidebarTabState::Explorer,
            SidebarTab::Git => session::SidebarTabState::Git,
        };
        state
    }

    /// Drains the debounced watcher stream for as long as the shell lives.
    ///
    /// One task for every workspace: the hub tags each batch with the root it
    /// came from, so nothing has to be respawned when a workspace is added.
    fn observe_watch(&mut self, cx: &mut Context<Self>) {
        let Some(changes) = self.watch.as_ref().map(|hub| hub.changes.clone()) else {
            return;
        };
        cx.spawn(async move |shell, cx| {
            while let Ok(batch) = changes.recv().await {
                if shell
                    .update(cx, |shell, cx| shell.apply_watch(batch, cx))
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }

    /// Registers one workspace root with the watcher.
    fn watch_workspace(&mut self, index: usize) {
        let Some(root) = self.workspaces.get(index).map(|w| w.root.clone()) else {
            return;
        };
        if let Some(hub) = self.watch.as_mut() {
            hub.watch(&root);
        }
    }

    fn apply_watch(&mut self, batch: Vec<RootChange>, cx: &mut Context<Self>) {
        for change in batch {
            let found = self
                .workspaces
                .iter()
                .position(|workspace| workspace.root == change.root);
            if let Some(index) = found {
                self.scan_workspace(index, change.index, change.git, cx);
            }
        }
    }

    /// Rebuilds the file index and the Git snapshot for one workspace off the
    /// main thread.
    ///
    /// Both walk the whole tree. Run inline they block the `open_window`
    /// callback, so a large root means the window is never created at all.
    /// `index` and `git` are requested separately because the watcher can tell
    /// a rename from a write, and only a rename changes the file set.
    fn scan_workspace(
        &mut self,
        workspace_index: usize,
        want_index: bool,
        want_git: bool,
        cx: &mut Context<Self>,
    ) {
        if !want_index && !want_git {
            return;
        }
        let Some(workspace) = self.workspaces.get_mut(workspace_index) else {
            return;
        };
        // One walk at a time. Anything asked for mid-walk is folded into the
        // run that follows it.
        if workspace.scan.running {
            workspace.scan.queued_index |= want_index;
            workspace.scan.queued_git |= want_git;
            return;
        }
        workspace.scan.running = true;
        let root = workspace.root.clone();
        // The rail can be reordered or a workspace closed while the walk runs,
        // so the result is matched back by root, not by the index it started
        // from.
        let scan_root = root.clone();

        cx.spawn(async move |shell, cx| {
            let (files, git) = cx
                .background_spawn(async move {
                    (
                        want_index.then(|| crate::services::file_index::build(&root)),
                        want_git.then(|| crate::services::git::snapshot(&root)),
                    )
                })
                .await;
            shell
                .update(cx, |shell, cx| {
                    let found = shell
                        .workspaces
                        .iter()
                        .position(|workspace| workspace.root == scan_root);
                    let Some(index) = found else {
                        return;
                    };
                    let queued = shell
                        .workspaces
                        .get_mut(index)
                        .map(|workspace| {
                            workspace.apply_scan(files, git);
                            workspace.scan.running = false;
                            let queued = (workspace.scan.queued_index, workspace.scan.queued_git);
                            workspace.scan.queued_index = false;
                            workspace.scan.queued_git = false;
                            queued
                        })
                        .unwrap_or((false, false));
                    cx.notify();
                    shell.scan_workspace(index, queued.0, queued.1, cx);
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

    fn active_editor(&self) -> Option<Entity<EditorView>> {
        match self.workspace().selected_tab().map(|tab| &tab.kind) {
            Some(TabKind::File { editor, .. }) => Some(editor.clone()),
            _ => None,
        }
    }

    fn active_surface(&self) -> Option<ActiveSurface> {
        let workspace = self.workspaces.get(self.active)?;
        let tab = workspace.selected_tab()?;
        let (mode, entity) = match &tab.kind {
            TabKind::Terminal(view) => (None, Some(view.entity_id())),
            TabKind::File { editor, mode, .. } => (Some(*mode), Some(editor.entity_id())),
            TabKind::Diff { view, .. } => (None, Some(view.entity_id())),
            TabKind::Image { .. } | TabKind::Video { .. } | TabKind::ImageDiff { .. } => {
                (None, None)
            }
        };
        Some(ActiveSurface {
            root: workspace.root.clone(),
            tab_id: tab.id,
            mode,
            entity,
        })
    }

    fn focus_active_surface(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = match self.workspace().selected_tab().map(|tab| &tab.kind) {
            Some(TabKind::Terminal(view)) => Some(view.read(cx).focus_handle(cx)),
            Some(TabKind::File {
                editor,
                mode: FileMode::Source,
                ..
            }) => Some(editor.read(cx).focus_handle(cx)),
            Some(TabKind::Diff { view, .. }) => Some(view.read(cx).focus_handle(cx)),
            _ => None,
        };
        window.focus(handle.as_ref().unwrap_or(&self.focus), cx);
    }

    /// Stages, commits and pushes, in that order. One path for the Git panel
    /// button and `Cmd-Return`.
    pub(crate) fn push_commit(&mut self, cx: &mut Context<Self>) {
        if self.workspace().pushing {
            return;
        }
        let subject = self
            .workspace()
            .commit_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let root = self.workspace().root.clone();
        self.workspace_mut().pushing = true;
        match git::commit_and_push(&root, &subject) {
            Ok(message) => self.set_status(message),
            Err(err) => self.set_status(err),
        }
        self.workspace_mut().pushing = false;
        self.workspace_mut().refresh_git();
        cx.notify();
    }

    fn on_commit_push(&mut self, _: &CommitPush, _: &mut Window, cx: &mut Context<Self>) {
        self.push_commit(cx);
    }

    /// Shows the active file in the Explorer: opens the sidebar on the
    /// Explorer tab and expands the tree down to the file.
    fn on_reveal_in_explorer(
        &mut self,
        _: &RevealInExplorer,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .workspace()
            .selected_tab()
            .and_then(|tab| tab.file_path().map(Path::to_path_buf))
        else {
            return;
        };
        self.shows_sidebar = true;
        self.sidebar_tab = SidebarTab::Explorer;
        self.workspace_mut().tree.reveal(&path);
        cx.notify();
    }

    fn on_navigate_back(&mut self, _: &NavigateBack, _: &mut Window, cx: &mut Context<Self>) {
        self.workspace_mut().navigate(-1, cx);
        cx.notify();
    }

    fn on_navigate_forward(&mut self, _: &NavigateForward, _: &mut Window, cx: &mut Context<Self>) {
        self.workspace_mut().navigate(1, cx);
        cx.notify();
    }

    fn on_find_in_file(&mut self, _: &FindInFile, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| editor.open_find(false, window, cx));
        }
    }

    fn on_find_replace(&mut self, _: &FindReplace, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| editor.open_find(true, window, cx));
        }
    }

    fn on_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| editor.find_step(1, cx));
        }
    }

    fn on_find_prev(&mut self, _: &FindPrev, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor() {
            editor.update(cx, |editor, cx| editor.find_step(-1, cx));
        }
    }

    /// Types `path:line` for the active file into the workspace terminal and
    /// selects it, the way the parent app references editor selections.
    fn on_insert_file_reference(
        &mut self,
        _: &InsertFileReference,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let (path, line) = {
            let editor = editor.read(cx);
            (editor.path.clone(), editor.cursor_line())
        };
        let root = self.workspace().root.clone();
        let display = path.strip_prefix(&root).unwrap_or(&path).display();
        let reference = format!("{display}:{line} ");

        let terminal = self
            .workspace()
            .tabs
            .iter()
            .position(|tab| tab.is_terminal());
        let Some(index) = terminal else {
            return;
        };
        if let Some(TabKind::Terminal(view)) = self.workspace().tabs.get(index).map(|tab| &tab.kind)
        {
            view.read(cx).session.write(reference.into_bytes());
        }
        self.workspace_mut().selected = index;
        cx.notify();
    }

    fn select_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.workspaces.len() || index == self.active {
            return;
        }
        self.active = index;
        // A watched workspace is already current, so switching costs nothing.
        // Without a watcher the snapshot is stale, and reading it here would
        // walk the whole tree on the main thread, so it goes to the background.
        let watched = self.watch.is_some();
        if !watched {
            self.scan_workspace(index, false, true, cx);
        }
        cx.notify();
    }

    /// Moves the workspace at `from` so it ends up at `to` in the final list.
    ///
    /// Dropping on a row below inserts after it and dropping on a row above
    /// inserts before it, which is what plain remove-then-insert gives.
    fn move_workspace(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.workspaces.len() {
            return;
        }
        let active_root = self
            .workspaces
            .get(self.active)
            .map(|workspace| workspace.root.clone());
        let workspace = self.workspaces.remove(from);
        let to = to.min(self.workspaces.len());
        self.workspaces.insert(to, workspace);
        if let Some(root) = active_root {
            if let Some(index) = self
                .workspaces
                .iter()
                .position(|workspace| workspace.root == root)
            {
                self.active = index;
            }
        }
        cx.notify();
    }

    /// Closes one workspace. The last one stays open because the shell always
    /// renders an active workspace.
    fn close_workspace(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.workspaces.len() <= 1 || index >= self.workspaces.len() {
            return;
        }
        self.workspaces.remove(index);
        if index < self.active {
            self.active -= 1;
        } else if index == self.active {
            self.active = self.active.min(self.workspaces.len() - 1);
        }
        cx.notify();
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
    /// POC is measured against; the home directory is the fallback.
    fn picker_start_directory(&self) -> PathBuf {
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
        self.watch_workspace(self.active);
        self.scan_workspace(self.active, true, true, cx);
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
        let web_preview = match &tab.kind {
            TabKind::File {
                path,
                preview_view: Some(PreviewKind::Web(view)),
                ..
            } => Some((view.clone(), path.clone())),
            _ => None,
        };
        let result = editor.update(cx, |editor, _| editor.save());
        match result {
            Ok(()) => {
                // A saved HTML file reloads its preview.
                if let Some((view, path)) = web_preview {
                    view.update(cx, |view, _| {
                        view.load_url(&format!("file://{}", path.display()));
                    });
                }
                self.set_status("saved")
            }
            Err(err) => self.set_status(format!("save failed: {err}")),
        }
        // The watcher sees the write and refreshes off the main thread. Reading
        // the status here as well would walk the tree on every Cmd-S.
        if self.watch.is_none() {
            self.workspace_mut().refresh_git();
        }
        cx.notify();
    }

    pub(crate) fn on_toggle_preview(
        &mut self,
        _: &TogglePreview,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // From an HTML text diff, Preview opens the working-tree file
        // rendered, per DESIGN.md > Git.
        let html_diff = match self.workspace().selected_tab().map(|tab| &tab.kind) {
            Some(TabKind::Diff { path, .. }) if is_html_path(Path::new(path)) => {
                Some(self.workspace().root.join(path))
            }
            _ => None,
        };
        if let Some(path) = html_diff {
            if path.is_file() {
                self.workspace_mut().open_file(path, false, cx);
                self.workspace_mut().toggle_mode();
            }
            cx.notify();
            return;
        }
        self.workspace_mut().toggle_mode();
        cx.notify();
    }

    pub(crate) fn on_toggle_wrap(
        &mut self,
        _: &ToggleWrap,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.word_wrap = !self.word_wrap;
        settings::set_word_wrap(self.word_wrap, cx);
        for workspace in &mut self.workspaces {
            workspace.toggle_wrap(self.word_wrap, cx);
        }
        cx.notify();
    }

    fn on_toggle_tab_close_buttons(
        &mut self,
        _: &ToggleTabCloseButtons,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shows_tab_close_buttons = !self.shows_tab_close_buttons;
        cx.notify();
    }

    /// Creates the native webview for the selected HTML tab in Preview mode.
    ///
    /// Lazy and render-driven because building one needs the window handle,
    /// which neither `open_file` nor session restore is given.
    pub(crate) fn ensure_web_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.workspace().selected;
        let (path, video) = match self.workspace().tabs.get(selected).map(|tab| &tab.kind) {
            Some(TabKind::File {
                path,
                mode: FileMode::Preview,
                preview_view: None,
                ..
            }) if is_html_path(path) => (path.clone(), false),
            Some(TabKind::Video { path, view: None }) => (path.clone(), true),
            _ => return,
        };
        let raw = {
            let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
                self.set_status("web preview failed: no window handle");
                return;
            };
            match wry::WebViewBuilder::new().build_as_child(&handle) {
                Ok(raw) => raw,
                Err(err) => {
                    self.set_status(format!("web preview failed: {err}"));
                    return;
                }
            }
        };
        let url = if video {
            // A bare video URL gets Safari's standalone media document, which
            // dims the whole frame behind its hover controls. A wrapper page
            // with a plain <video> keeps the frame clean; DESIGN.md > File
            // Previews.
            match write_video_wrapper(&path) {
                Some(wrapper) => format!("file://{}", wrapper.display()),
                None => {
                    self.set_status("video preview failed: cannot write wrapper page");
                    return;
                }
            }
        } else {
            format!("file://{}", path.display())
        };
        let view = cx.new(|cx| gpui_wry::WebView::new(raw, window, cx));
        view.update(cx, |view, _| {
            view.load_url(&url);
        });
        match self
            .workspace_mut()
            .tabs
            .get_mut(selected)
            .map(|tab| &mut tab.kind)
        {
            Some(TabKind::File { preview_view, .. }) if !video => {
                *preview_view = Some(PreviewKind::Web(view));
            }
            Some(TabKind::Video { view: slot, .. }) if video => {
                *slot = Some(view);
            }
            _ => {}
        }
    }

    /// A native webview keeps painting over the GPUI canvas unless it is
    /// hidden explicitly, so every frame reconciles visibility: only the
    /// selected Preview tab of the active workspace shows, and never under
    /// an overlay.
    fn sync_webviews(&mut self, cx: &mut Context<Self>) {
        let overlay_open = self.overlay.kind.is_some();
        let mut views = Vec::new();
        for (workspace_index, workspace) in self.workspaces.iter().enumerate() {
            for (tab_index, tab) in workspace.tabs.iter().enumerate() {
                let front = workspace_index == self.active
                    && tab_index == workspace.selected
                    && !overlay_open;
                match &tab.kind {
                    TabKind::File {
                        mode,
                        preview_view: Some(PreviewKind::Web(view)),
                        ..
                    } => views.push((view.clone(), front && *mode == FileMode::Preview)),
                    TabKind::Video {
                        view: Some(view), ..
                    } => views.push((view.clone(), front)),
                    _ => {}
                }
            }
        }
        for (view, visible) in views {
            view.update(cx, |view, _| {
                if visible {
                    view.show();
                } else {
                    view.hide();
                }
            });
        }
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = (self.zoom + 0.1).min(2.0);
        cx.set_global(theme::EditorZoom(self.zoom));
        cx.notify();
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = (self.zoom - 0.1).max(0.8);
        cx.set_global(theme::EditorZoom(self.zoom));
        cx.notify();
    }

    fn on_reset_zoom(&mut self, _: &ResetZoom, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        cx.set_global(theme::EditorZoom(self.zoom));
        cx.notify();
    }

    fn on_ui_zoom_in(&mut self, _: &UiZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.ui_zoom = (self.ui_zoom + 0.1).min(1.4);
        theme::set_ui_zoom(self.ui_zoom, cx);
        cx.notify();
    }

    fn on_ui_zoom_out(&mut self, _: &UiZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.ui_zoom = (self.ui_zoom - 0.1).max(0.8);
        theme::set_ui_zoom(self.ui_zoom, cx);
        cx.notify();
    }

    fn on_reset_ui_zoom(&mut self, _: &ResetUiZoom, _: &mut Window, cx: &mut Context<Self>) {
        self.ui_zoom = 1.0;
        theme::set_ui_zoom(self.ui_zoom, cx);
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

    /// `DESIGN.md` > Workspace Rail. Fixed 230 points, dark in both
    /// appearances, one graphite-to-petrol gradient as its only depth effect.
    fn render_rail(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let active = self.active;
        let total = self.workspaces.len();
        let active_workspace = self.workspace();
        let active_name = active_workspace.name.clone();
        let active_branch = if active_workspace.git.is_repo {
            active_workspace.git.branch.clone()
        } else {
            "no repository".to_string()
        };
        let active_changed = active_workspace.git.changed_count();
        let selected_tab = self.sidebar_tab;
        let shell = cx.entity();

        v_flex()
            .w(Metrics::RAIL_WIDTH)
            .flex_none()
            .h_full()
            .relative()
            .border_r_1()
            .border_color(c.rail_border)
            .bg(linear_gradient(
                180.,
                linear_color_stop(c.rail_top, 0.),
                linear_color_stop(c.rail_bottom, 1.),
            ))
            // Atelier's rail sheen: a faint top light falling into shade, so
            // the rail reads as one lit panel instead of a flat fill.
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(gpui::white().opacity(0.06), 0.),
                linear_color_stop(gpui::black().opacity(0.08), 1.),
            )))
            .child(
                h_flex()
                    .h(px(64.))
                    .flex_none()
                    .items_center()
                    .gap(Space::S)
                    .px(Space::M)
                    .child(
                        div()
                            .flex_none()
                            .size(px(34.))
                            .rounded(Radius::CONTROL)
                            .bg(c.accent)
                            .text_color(c.accent_ink)
                            .text_size(Type::HEADLINE * ui_zoom)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(SharedString::from(
                                active_name
                                    .chars()
                                    .next()
                                    .map_or('A', |initial| initial)
                                    .to_uppercase()
                                    .to_string(),
                            )),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(2.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(Type::BODY * ui_zoom)
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(c.rail_foreground)
                                    .child(SharedString::from(active_name)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(Space::XS)
                                    .text_size(Type::MICRO * ui_zoom)
                                    .text_color(c.rail_secondary)
                                    .child(Icon::new(IconName::Network).xsmall())
                                    .child(
                                        div()
                                            .truncate()
                                            .font_family("JetBrains Mono")
                                            .child(SharedString::from(active_branch)),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_none()
                    .px(Space::S)
                    .gap(Space::XS)
                    .child(
                        h_flex()
                            .id("rail-files")
                            .cursor_pointer()
                            .h(px(40.))
                            .items_center()
                            .gap(Space::S)
                            .px(Space::S)
                            .rounded(Radius::ROW)
                            .text_color(c.rail_foreground)
                            .when(selected_tab == SidebarTab::Explorer, |this| {
                                this.bg(c.rail_selection)
                                    .border_1()
                                    .border_color(gpui::white().opacity(0.16))
                            })
                            .when(selected_tab != SidebarTab::Explorer, |this| {
                                this.border_1()
                                    .border_color(gpui::transparent_black())
                                    .hover(|this| this.bg(c.rail_hover))
                            })
                            .child(Icon::new(IconName::Folder).small())
                            .child(
                                div()
                                    .text_size(Type::BODY * ui_zoom)
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Files"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_tab = SidebarTab::Explorer;
                                this.shows_sidebar = true;
                                this.focus_mode = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        h_flex()
                            .id("rail-search")
                            .cursor_pointer()
                            .h(px(40.))
                            .items_center()
                            .gap(Space::S)
                            .px(Space::S)
                            .rounded(Radius::ROW)
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .text_color(c.rail_foreground)
                            .hover(|this| this.bg(c.rail_hover))
                            .child(Icon::new(IconName::Search).small())
                            .child(
                                div()
                                    .text_size(Type::BODY * ui_zoom)
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Search"),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_search_all(&SearchAllFiles, window, cx)
                            })),
                    )
                    .child(
                        h_flex()
                            .id("rail-changes")
                            .cursor_pointer()
                            .h(px(40.))
                            .items_center()
                            .gap(Space::S)
                            .px(Space::S)
                            .rounded(Radius::ROW)
                            .text_color(c.rail_foreground)
                            .when(selected_tab == SidebarTab::Git, |this| {
                                this.bg(c.rail_selection)
                                    .border_1()
                                    .border_color(gpui::white().opacity(0.16))
                            })
                            .when(selected_tab != SidebarTab::Git, |this| {
                                this.border_1()
                                    .border_color(gpui::transparent_black())
                                    .hover(|this| this.bg(c.rail_hover))
                            })
                            .child(Icon::new(IconName::Network).small())
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(Type::BODY * ui_zoom)
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Changes"),
                            )
                            .when(active_changed > 0, |this| {
                                this.child(rail_count_badge(active_changed, c, ui_zoom))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_tab = SidebarTab::Git;
                                this.shows_sidebar = true;
                                this.focus_mode = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .h(Metrics::PANEL_HEADER)
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px(Space::M)
                    .child(
                        div()
                            .text_size(Type::LABEL * ui_zoom)
                            .text_color(c.rail_secondary)
                            .child("WORKSPACES"),
                    )
                    .child(
                        div()
                            .font_family("JetBrains Mono")
                            .text_size(Type::MICRO * ui_zoom)
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
                    .children(workspace_rail_rows(&self.workspaces, active).map(
                        |(index, workspace, selected)| {
                            let changed = workspace.git.changed_count();
                            let name = workspace.name.clone();
                            let path = workspace.root.to_string_lossy().to_string();
                            let shell = shell.clone();
                            v_flex()
                                .id(("workspace", index))
                                .cursor_pointer()
                                .h(Metrics::RAIL_ITEM_HEIGHT)
                                .justify_center()
                                .gap(px(1.))
                                .px(Space::S)
                                .rounded(Radius::ROW)
                                .when(selected, |this| {
                                    // Atelier's glass pill: a top-lit
                                    // hairline over the fill plus a soft
                                    // drop, so selection sits above the
                                    // rail instead of staining it.
                                    this.bg(c.rail_selection)
                                        .border_1()
                                        .border_color(gpui::white().opacity(0.16))
                                        .shadow(crate::app::chrome::shadow_soft())
                                })
                                .when(!selected, |this| {
                                    this.border_1()
                                        .border_color(gpui::transparent_black())
                                        .hover(|this| this.bg(c.rail_hover))
                                        .active(|this| this.bg(c.rail_pressed))
                                })
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(Space::XS)
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_size(Type::BODY * ui_zoom)
                                                .text_color(c.rail_foreground)
                                                .when(selected, |this| {
                                                    this.font_weight(gpui::FontWeight::SEMIBOLD)
                                                })
                                                .child(SharedString::from(workspace.name.clone())),
                                        )
                                        .when(changed > 0, |this| {
                                            this.child(rail_count_badge(changed, c, ui_zoom))
                                        }),
                                )
                                .when(index < 9, |this| {
                                    this.child(
                                        div()
                                            .font_family("JetBrains Mono")
                                            .text_size(Type::MICRO * ui_zoom)
                                            .text_color(c.rail_secondary)
                                            .child(SharedString::from(format!("⌘{}", index + 1))),
                                    )
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_workspace(index, cx)
                                }))
                                .on_drag(DraggedWorkspace { index }, move |dragged, _, _, cx| {
                                    let name = name.clone();
                                    let colors = c;
                                    let _ = dragged;
                                    cx.new(|_| WorkspaceDragPreview { name, c: colors })
                                })
                                .drag_over::<DraggedWorkspace>(move |style, _, _, _| {
                                    style.bg(c.rail_hover).border_color(c.accent)
                                })
                                .on_drop(cx.listener(
                                    move |this, dragged: &DraggedWorkspace, _, cx| {
                                        this.move_workspace(dragged.index, index, cx)
                                    },
                                ))
                                .context_menu(move |menu, _, _| {
                                    let path_finder = path.clone();
                                    let path_copy = path.clone();
                                    let activate = shell.clone();
                                    let move_up = shell.clone();
                                    let move_down = shell.clone();
                                    let close = shell.clone();
                                    menu.item(
                                        PopupMenuItem::new("Activate Workspace")
                                            .disabled(selected)
                                            .on_click(move |_, _, cx| {
                                                activate.update(cx, |shell, cx| {
                                                    shell.select_workspace(index, cx)
                                                })
                                            }),
                                    )
                                    .item(PopupMenuItem::new("Show in Finder").on_click(
                                        move |_, _, _| {
                                            std::process::Command::new("open")
                                                .arg("-R")
                                                .arg(&path_finder)
                                                .spawn()
                                                .ok();
                                        },
                                    ))
                                    .item(PopupMenuItem::new("Copy Project Path").on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                path_copy.clone(),
                                            ));
                                        },
                                    ))
                                    .separator()
                                    .item(
                                        PopupMenuItem::new("Move Up")
                                            .disabled(index == 0)
                                            .on_click(move |_, _, cx| {
                                                move_up.update(cx, |shell, cx| {
                                                    shell.move_workspace(
                                                        index,
                                                        index.saturating_sub(1),
                                                        cx,
                                                    )
                                                })
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new("Move Down")
                                            .disabled(index + 1 >= total)
                                            .on_click(move |_, _, cx| {
                                                move_down.update(cx, |shell, cx| {
                                                    shell.move_workspace(index, index + 1, cx)
                                                })
                                            }),
                                    )
                                    .separator()
                                    .item(
                                        PopupMenuItem::new("Close Workspace")
                                            .disabled(total <= 1)
                                            .on_click(move |_, _, cx| {
                                                close.update(cx, |shell, cx| {
                                                    shell.close_workspace(index, cx)
                                                })
                                            }),
                                    )
                                })
                        },
                    )),
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
                            .text_size(Type::LABEL * ui_zoom)
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
        let layout = self.layout;
        let compact = layout == LayoutMode::Compact;
        let shows_sidebar = self.shows_sidebar && !self.focus_mode;
        let shows_inspector = self.shows_inspector && layout.allows_inspector() && !self.focus_mode;
        let dark = self.dark;

        // One unified compact row, like atelier's toolbar: the traffic lights,
        // the sidebar toggle and the project menu all share a single band.
        // The empty stretches are drag fillers, so the window still moves.
        v_flex()
            .w_full()
            .flex_none()
            .bg(c.toolbar)
            .border_b_1()
            .border_color(c.rail_border)
            .child(
                h_flex()
                    .h(Metrics::TOP_CHROME)
                    .w_full()
                    .items_center()
                    .px(Space::S)
                    // The traffic lights sit over the leading edge, so the
                    // first control starts clear of them. Full screen hides
                    // them and releases the inset.
                    .when(title_inset > px(0.), |this| this.pl(px(84.)))
                    .child(
                        h_flex()
                            .when(compact, |this| this.flex_none())
                            .when(!compact, |this| this.flex_1())
                            .h_full()
                            .items_center()
                            .gap(Space::XS)
                            .child(toolbar_icon_button(
                                "toggle-sidebar",
                                IconName::PanelLeft,
                                shows_sidebar,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_sidebar(&ToggleSidebar, window, cx)
                                }),
                            ))
                            .when(!compact, |this| {
                                this.child(
                                    div()
                                        .ml(Space::XS)
                                        .text_size(Type::UI * self.ui_zoom)
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(c.rail_foreground)
                                        .child("Reading Room"),
                                )
                            })
                            .child(crate::app::chrome::toolbar_drag_filler()),
                    )
                    .child(
                        h_flex()
                            .id("global-search")
                            .cursor_pointer()
                            .when(compact, |this| this.flex_1().min_w(px(0.)))
                            .when(!compact, |this| {
                                this.w(Metrics::PROJECT_MENU_WIDTH).flex_none()
                            })
                            .h(Metrics::FIELD)
                            .items_center()
                            .gap(Space::S)
                            .px(Space::M)
                            .rounded(Radius::CONTROL)
                            .bg(c.editor)
                            .border_1()
                            .border_color(c.border)
                            .hover(|this| this.border_color(c.accent.opacity(0.7)))
                            .child(
                                Icon::new(IconName::Search)
                                    .xsmall()
                                    .text_color(c.ink_secondary),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(Type::LABEL * self.ui_zoom)
                                    .text_color(c.ink_secondary)
                                    .child("Search files, symbols, commits..."),
                            )
                            .when(!compact, |this| {
                                this.child(
                                    div()
                                        .font_family("JetBrains Mono")
                                        .text_size(Type::MICRO * self.ui_zoom)
                                        .text_color(c.ink_secondary.opacity(0.8))
                                        .child("⌘K"),
                                )
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_quick_open(&QuickOpen, window, cx)
                            })),
                    )
                    .child(
                        h_flex()
                            .when(compact, |this| this.flex_none())
                            .when(!compact, |this| this.flex_1())
                            .h_full()
                            .items_center()
                            .justify_end()
                            .gap(Space::XS)
                            .child(crate::app::chrome::toolbar_drag_filler())
                            .child(toolbar_icon_button(
                                "toggle-appearance",
                                if dark { IconName::Moon } else { IconName::Sun },
                                false,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_appearance(&ToggleAppearance, window, cx)
                                }),
                            ))
                            .child(toolbar_icon_button(
                                "toggle-changes",
                                IconName::Network,
                                self.sidebar_tab == SidebarTab::Git
                                    && self.shows_sidebar
                                    && !self.focus_mode,
                                c,
                                cx.listener(|this, _, _, cx| {
                                    this.sidebar_tab = SidebarTab::Git;
                                    this.shows_sidebar = true;
                                    this.focus_mode = false;
                                    cx.notify();
                                }),
                            ))
                            .child(toolbar_icon_button(
                                "focus-mode",
                                IconName::Maximize,
                                self.focus_mode,
                                c,
                                cx.listener(|this, _, window, cx| {
                                    this.on_toggle_focus_mode(&ToggleFocusMode, window, cx)
                                }),
                            ))
                            .when(layout.allows_inspector(), |this| {
                                this.child(toolbar_icon_button(
                                    "toggle-inspector",
                                    IconName::PanelRight,
                                    shows_inspector,
                                    c,
                                    cx.listener(|this, _, window, cx| {
                                        this.on_toggle_inspector(&ToggleInspector, window, cx)
                                    }),
                                ))
                            }),
                    ),
            )
    }

    /// `DESIGN.md` > Workspace Status: Git identity leads; real active-surface
    /// metadata, working-tree state, token estimate and zoom trail.
    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let ui_zoom = self.ui_zoom;
        let condensed = self.layout != LayoutMode::Wide;
        let workspace = self.workspace();
        let branch = if workspace.git.is_repo {
            workspace.git.branch.clone()
        } else {
            "no repository".to_string()
        };
        let head = workspace.git.head_short.clone();
        let changed = workspace.git.changed_count();
        let git_state = workspace.git.is_repo.then_some(if changed == 0 {
            (true, "Working Tree Clean")
        } else {
            (false, "Working Tree Dirty")
        });
        let (surface, document_mode, line_ending, tokens) = match workspace
            .selected_tab()
            .map(|tab| &tab.kind)
        {
            Some(TabKind::File {
                path,
                editor,
                mode,
                preview_view,
            }) => {
                let editor = editor.read(cx);
                let surface = Some(file_status_label(path, editor.language().name()));
                let previewable = preview_view.is_some() || is_html_path(path);
                let document_mode = previewable.then_some(match mode {
                    FileMode::Preview => "Preview",
                    FileMode::Source => "Raw",
                });
                let line_ending = Some(editor.line_ending());
                (
                    surface,
                    document_mode,
                    line_ending,
                    Some(editor.byte_len() / 4),
                )
            }
            Some(TabKind::Terminal(_)) => (Some("Terminal".to_string()), None, None, None),
            Some(TabKind::Image { .. }) => (Some("Image".to_string()), None, None, None),
            Some(TabKind::Video { .. }) => (Some("Video".to_string()), None, None, None),
            Some(TabKind::Diff { path, .. }) => (
                Some(format!(
                    "{} Diff",
                    file_status_label(Path::new(path), "Text")
                )),
                None,
                None,
                None,
            ),
            Some(TabKind::ImageDiff { .. }) => (Some("Image Diff".to_string()), None, None, None),
            None => (None, None, None, None),
        };
        let status = self.status.clone();
        let changed_label = condensed
            .then(|| format!("{changed} ch"))
            .unwrap_or_else(|| format!("{changed} changed"));

        h_flex()
            .h(Metrics::STATUS_BAR)
            .w_full()
            .flex_none()
            .items_center()
            .px(Space::M)
            .gap(if condensed { Space::S } else { Space::M })
            .overflow_hidden()
            .bg(crate::app::chrome::chrome_gradient(c))
            .border_t_1()
            .border_color(c.border)
            .text_size(Type::MICRO * ui_zoom)
            .text_color(c.ink_secondary)
            .child(
                h_flex()
                    .min_w(px(0.))
                    .items_center()
                    .gap(Space::XS)
                    .child(Icon::new(IconName::Network).xsmall())
                    .child(
                        div()
                            .min_w(px(0.))
                            .when(condensed, |this| this.max_w(px(80.)))
                            .truncate()
                            .child(SharedString::from(branch)),
                    )
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
                        .flex_none()
                        .text_color(c.git_modified)
                        .child(SharedString::from(changed_label)),
                )
            })
            .child(div().flex_1().min_w(px(0.)))
            .when_some(status.filter(|_| !condensed), |this, text| {
                this.child(div().max_w(px(180.)).truncate().child(text))
            })
            .when_some(surface, |this, surface| this.child(div().child(surface)))
            .when_some(document_mode, |this, mode| this.child(div().child(mode)))
            .when_some(line_ending, |this, line_ending| {
                this.child(div().font_family("JetBrains Mono").child(line_ending))
            })
            .when_some(git_state, |this, (clean, label)| {
                let label = if condensed {
                    if clean { "Clean" } else { "Dirty" }
                } else {
                    label
                };
                this.child(
                    h_flex()
                        .items_center()
                        .gap(Space::XS)
                        .child(div().size(px(6.)).rounded_full().bg(if clean {
                            c.git_added
                        } else {
                            c.git_modified
                        }))
                        .child(label),
                )
            })
            .when_some(tokens, |this, tokens| {
                this.child(
                    div()
                        .font_family("JetBrains Mono")
                        .child(SharedString::from(if condensed {
                            format!("~{tokens} tok")
                        } else {
                            format!("~{tokens} tokens")
                        })),
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
        self.persist_session(cx);
        self.persist_settings(cx);
        self.sync_webviews(cx);
        let active_surface = self.active_surface();
        if active_surface != self.active_surface {
            self.active_surface = active_surface;
            let shell = cx.entity();
            window.defer(cx, move |window, cx| {
                let _ = shell.update(cx, |shell, cx| {
                    shell.focus_active_surface(window, cx);
                });
            });
        }
        let c = cx.tokens().c;
        let title_inset = theme::title_bar_inset(window);
        let width = f32::from(window.viewport_size().width) - f32::from(Metrics::RAIL_WIDTH);
        let layout = LayoutMode::for_width(width);
        if layout != self.layout {
            // Layout mode is derived, never a source of truth, so reading it
            // here cannot start a mutation loop during the layout pass.
            self.layout = layout;
        }

        crate::app::quick_settings::sync(crate::app::quick_settings::QuickSettingsSnapshot {
            zoom: self.zoom,
            ui_zoom: self.ui_zoom,
            focus_mode: self.focus_mode,
            shows_sidebar: self.shows_sidebar,
            shows_inspector: self.shows_inspector,
            sidebar_available: layout.allows_sidebar() && !self.focus_mode,
            inspector_available: layout.allows_inspector() && !self.focus_mode,
            dark: self.dark,
            word_wrap: self.word_wrap,
            shows_tab_close_buttons: self.shows_tab_close_buttons,
        });

        let show_sidebar = self.shows_sidebar && layout.allows_sidebar() && !self.focus_mode;
        let show_inspector = self.shows_inspector && layout.allows_inspector() && !self.focus_mode;
        let show_compact_sidebar =
            self.shows_sidebar && layout == LayoutMode::Compact && !self.focus_mode;

        div()
            .id("shell")
            .track_focus(&self.focus)
            .key_context("Shell")
            .size_full()
            .bg(c.canvas)
            .text_color(c.ink)
            .text_size(Type::BODY * self.ui_zoom)
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_inspector))
            .on_action(cx.listener(Self::on_toggle_sidebar_tab))
            .on_action(cx.listener(Self::on_toggle_focus_mode))
            .on_action(cx.listener(Self::on_new_terminal))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_commit_push))
            .on_action(cx.listener(Self::on_reveal_in_explorer))
            .on_action(cx.listener(Self::on_navigate_back))
            .on_action(cx.listener(Self::on_navigate_forward))
            .on_action(cx.listener(Self::on_find_in_file))
            .on_action(cx.listener(Self::on_find_replace))
            .on_action(cx.listener(Self::on_find_next))
            .on_action(cx.listener(Self::on_find_prev))
            .on_action(cx.listener(Self::on_insert_file_reference))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_toggle_wrap))
            .on_action(cx.listener(Self::on_toggle_tab_close_buttons))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_reset_zoom))
            .on_action(cx.listener(Self::on_ui_zoom_in))
            .on_action(cx.listener(Self::on_ui_zoom_out))
            .on_action(cx.listener(Self::on_reset_ui_zoom))
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
                                                    .when(show_sidebar, |split| {
                                                        split.child(
                                                            resizable_panel()
                                                                .flex_none()
                                                                .size(Metrics::SIDEBAR_IDEAL)
                                                                .size_range(
                                                                    Metrics::SIDEBAR_MIN
                                                                        ..Metrics::SIDEBAR_MAX,
                                                                )
                                                                .child(self.render_sidebar(cx)),
                                                        )
                                                    })
                                                    .child(
                                                        resizable_panel()
                                                            .size_range(
                                                                Metrics::CENTER_MIN..px(4000.),
                                                            )
                                                            .child(self.render_center(window, cx)),
                                                    )
                                                    .when(show_inspector, |split| {
                                                        split.child(
                                                            resizable_panel()
                                                                .flex_none()
                                                                .size(Metrics::INSPECTOR_IDEAL)
                                                                .size_range(
                                                                    Metrics::INSPECTOR_MIN
                                                                        ..Metrics::INSPECTOR_MAX,
                                                                )
                                                                .child(self.render_inspector(cx)),
                                                        )
                                                    }),
                                            ),
                                    )
                                    .when(show_compact_sidebar, |this| {
                                        this.child(
                                            div()
                                                .id("compact-navigator-scrim")
                                                .absolute()
                                                .top(px(0.))
                                                .right(px(0.))
                                                .bottom(Metrics::STATUS_BAR)
                                                .left(Metrics::SIDEBAR_IDEAL)
                                                .cursor_pointer()
                                                .bg(gpui::black().opacity(0.12))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.shows_sidebar = false;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .id("compact-navigator")
                                                .absolute()
                                                .top(px(0.))
                                                .bottom(Metrics::STATUS_BAR)
                                                .left(px(0.))
                                                .w(Metrics::SIDEBAR_IDEAL)
                                                .bg(c.sidebar)
                                                .shadow_xl()
                                                .child(self.render_sidebar(cx)),
                                        )
                                    })
                                    .child(self.render_status_bar(cx))
                                    .children(self.render_overlay(window, cx)),
                            ),
                    ),
            )
    }
}

fn file_status_label(path: &Path, language: &str) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md" | "markdown") => "Markdown".to_string(),
        Some("txt") => "Plain Text".to_string(),
        Some(extension) if language.eq_ignore_ascii_case("text") => extension.to_ascii_uppercase(),
        _ => language.to_string(),
    }
}

pub(crate) fn workspace_rail_rows<T>(
    items: &[T],
    active: usize,
) -> impl Iterator<Item = (usize, &T, bool)> {
    items
        .iter()
        .enumerate()
        .map(move |(index, item)| (index, item, index == active))
}

/// Picks the workspace root for this launch.
///
/// LaunchServices sets the working directory to `/`, so the `current_dir`
/// fallback would otherwise make the whole filesystem the workspace. A
/// directory with no parent is the filesystem root, which is never a
/// workspace; the home directory is what a Finder or Dock launch lands on.
/// Writes the player page for one video to the temp directory: black
/// surface, a plain `<video>` with the native control bar and no dimming
/// media document around it.
fn write_video_wrapper(video: &Path) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("artifex-video");
    std::fs::create_dir_all(&dir).ok()?;
    let name = video.file_name()?.to_string_lossy().replace('/', "-");
    let page = dir.join(format!("{name}.html"));
    // Custom controls in a bar BELOW the frame: WebKit's native <video>
    // controls draw a dimming scrim over the picture on hover, which defeats
    // screenshotting the frame. Nothing here ever covers the video.
    let html = format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;height:100%;background:#000;overflow:hidden}}
body{{display:flex;flex-direction:column}}
video{{flex:1;min-height:0;width:100%;object-fit:contain;outline:none}}
#bar{{height:36px;flex:none;display:flex;align-items:center;gap:10px;
  padding:0 12px;background:#111;color:#ddd;
  font:12px 'JetBrains Mono',Menlo,monospace;user-select:none}}
button{{background:none;border:none;color:#ddd;font:inherit;cursor:pointer;padding:2px 6px}}
button:hover{{color:#fff}}
#seek{{flex:1;accent-color:#B4552D}}
</style></head><body>
<video id="v" src="file://{}" preload="metadata" playsinline></video>
<div id="bar">
  <button id="play">&#9654;</button>
  <span id="time">0:00</span>
  <input id="seek" type="range" min="0" max="1000" value="0">
  <span id="dur">0:00</span>
  <button id="mute">&#128266;</button>
</div>
<script>
const v=document.getElementById('v'),play=document.getElementById('play'),
seek=document.getElementById('seek'),time=document.getElementById('time'),
dur=document.getElementById('dur'),mute=document.getElementById('mute');
const fmt=s=>{{s=Math.floor(s||0);return Math.floor(s/60)+':'+String(s%60).padStart(2,'0')}};
const toggle=()=>v.paused?v.play():v.pause();
play.onclick=toggle;v.onclick=toggle;
document.onkeydown=e=>{{if(e.key===' '){{e.preventDefault();toggle()}}}};
v.onplay=()=>play.innerHTML='&#10074;&#10074;';
v.onpause=()=>play.innerHTML='&#9654;';
v.onloadedmetadata=()=>dur.textContent=fmt(v.duration);
v.ontimeupdate=()=>{{time.textContent=fmt(v.currentTime);
  if(v.duration)seek.value=Math.round(v.currentTime/v.duration*1000)}};
seek.oninput=()=>{{if(v.duration)v.currentTime=seek.value/1000*v.duration}};
mute.onclick=()=>{{v.muted=!v.muted;mute.innerHTML=v.muted?'&#128263;':'&#128266;'}};
</script>
</body></html>"##,
        video.display()
    );
    std::fs::write(&page, html).ok()?;
    Some(page)
}

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
        KeyBinding::new("cmd-k", QuickOpen, None),
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
        // Parent app binds both Cmd-O and Cmd-0 to the same open action.
        KeyBinding::new("cmd-o", AddWorkspace, None),
        // Shift folds into the produced character: Cmd-Shift-; arrives as ":".
        KeyBinding::new("cmd-:", NewTerminal, None),
        KeyBinding::new("cmd-enter", CommitPush, None),
        KeyBinding::new("cmd-b", RevealInExplorer, None),
        KeyBinding::new("ctrl--", NavigateBack, None),
        // GPUI folds Shift into the produced character for punctuation keys,
        // so Ctrl-Shift-- is indistinguishable from Ctrl--. Forward moves to
        // the neighbouring unshifted key instead; DESIGN.md records the
        // divergence.
        KeyBinding::new("ctrl-=", NavigateForward, None),
        KeyBinding::new("cmd-f", FindInFile, None),
        KeyBinding::new("cmd-alt-f", FindReplace, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrev, None),
        KeyBinding::new("cmd-shift-c", InsertFileReference, None),
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

/// Drag payload for reordering the workspace rail.
#[derive(Clone)]
struct DraggedWorkspace {
    index: usize,
}

/// The floating pill that follows the cursor while a rail row is dragged.
struct WorkspaceDragPreview {
    name: String,
    c: theme::Colors,
}

impl Render for WorkspaceDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(Space::S)
            .py(px(4.))
            .rounded(Radius::ROW)
            .border_1()
            .border_color(gpui::white().opacity(0.16))
            .bg(self.c.rail_selection)
            .shadow(crate::app::chrome::shadow_floating())
            .text_size(Type::BODY * theme::ui_zoom(cx))
            .text_color(self.c.rail_foreground)
            .child(SharedString::from(self.name.clone()))
    }
}
