//! One live workspace: root, file tree, git state, index and tabs.
//!
//! Sessions stay alive when another workspace is selected. Only the selected
//! workspace renders; the rest keep their tabs, editors and shell processes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _, Entity, Window};

use crate::app::editor::EditorView;
use crate::app::markdown::MarkdownView;
use crate::services::file_index::{self, IndexedFile};
use crate::services::fs_tree::FileTree;
use crate::services::git::{self, GitSnapshot};
use crate::terminal::TerminalView;
use gpui_component::input::InputState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    Source,
    Preview,
}

pub enum TabKind {
    Terminal(Entity<TerminalView>),
    File {
        path: PathBuf,
        editor: Entity<EditorView>,
        mode: FileMode,
        /// Present only for a file that has a preview mode, so the Markdown
        /// document is parsed once per open instead of once per frame.
        preview_view: Option<Entity<MarkdownView>>,
    },
    Diff {
        path: String,
        staged: bool,
        text: String,
    },
}

pub struct Tab {
    pub id: usize,
    pub title: String,
    pub kind: TabKind,
    /// A preview tab is replaced by the next single-click open.
    pub preview: bool,
}

impl Tab {
    pub fn is_terminal(&self) -> bool {
        matches!(self.kind, TabKind::Terminal(_))
    }

    pub fn file_path(&self) -> Option<&Path> {
        match &self.kind {
            TabKind::File { path, .. } => Some(path.as_path()),
            _ => None,
        }
    }
}

pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    pub tree: FileTree,
    pub git: GitSnapshot,
    pub index: Vec<IndexedFile>,
    /// Every indexed file plus its ancestor directories. A path missing from
    /// this set is Git-ignored, which `DESIGN.md` renders at reduced opacity
    /// rather than hiding.
    visible: HashSet<PathBuf>,
    /// False until the first walk lands. The Explorer must not dim every row
    /// while the index is still being built.
    indexed: bool,
    pub tabs: Vec<Tab>,
    pub selected: usize,
    /// The commit subject. `DESIGN.md` keeps the composer multiline and focused
    /// on direct text entry, with no extra chrome inside the card.
    pub commit_input: Entity<InputState>,
    pub pushing: bool,
    next_id: usize,
}

impl Workspace {
    /// Opens the workspace without touching the tree beyond the root listing.
    ///
    /// The file index and the Git snapshot both walk the whole workspace, so
    /// `Shell` builds them off the main thread and hands them back through
    /// `apply_scan`.
    pub fn open(root: PathBuf, window: &mut Window, cx: &mut App) -> Self {
        let root = git::repo_root(&root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string());

        let commit_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .soft_wrap(true)
                .placeholder("Commit subject")
        });
        let mut workspace = Self {
            tree: FileTree::new(root.clone()),
            git: GitSnapshot::default(),
            index: Vec::new(),
            visible: HashSet::new(),
            indexed: false,
            tabs: Vec::new(),
            selected: 0,
            commit_input,
            pushing: false,
            next_id: 0,
            root,
            name,
        };
        workspace.open_terminal(window, cx);
        workspace
    }

    /// True when the path is Git-ignored, so the Explorer can keep it visible
    /// but quiet.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.indexed && !self.visible.contains(path)
    }

    fn next_id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    pub fn selected_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.selected)
    }

    pub fn refresh_git(&mut self) {
        self.git = git::snapshot(&self.root);
    }

    pub fn reindex(&mut self) {
        self.set_index(file_index::build(&self.root));
    }

    /// Applies a scan built off the main thread.
    pub fn apply_scan(&mut self, files: Vec<IndexedFile>, git: GitSnapshot) {
        self.git = git;
        self.set_index(files);
    }

    fn set_index(&mut self, files: Vec<IndexedFile>) {
        self.visible = files
            .iter()
            .flat_map(|file| file.absolute.ancestors().map(Path::to_path_buf))
            .collect();
        self.index = files;
        self.indexed = true;
        self.tree.refresh();
    }

    pub fn open_terminal(&mut self, window: &mut Window, cx: &mut App) {
        match TerminalView::open(self.root.clone(), window, cx) {
            Ok(view) => {
                let id = self.next_id();
                self.tabs.push(Tab {
                    id,
                    title: "zsh".into(),
                    kind: TabKind::Terminal(view),
                    preview: false,
                });
                self.selected = self.tabs.len() - 1;
            }
            Err(err) => {
                eprintln!("terminal failed to start: {err}");
            }
        }
    }

    /// Opens a file. `preview` replaces the current preview tab, matching the
    /// Explorer single-click rule in DESIGN.md.
    pub fn open_file(&mut self, path: PathBuf, preview: bool, cx: &mut App) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.file_path() == Some(path.as_path()))
        {
            self.selected = index;
            if !preview {
                self.tabs[index].preview = false;
            }
            return;
        }

        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let previewable = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("markdown")
        );
        let editor = EditorView::open(path.clone(), cx);
        let preview_view = previewable.then(|| MarkdownView::open(path.clone(), cx));
        let id = self.next_id();
        let tab = Tab {
            id,
            title,
            kind: TabKind::File {
                path,
                editor,
                mode: if previewable {
                    FileMode::Preview
                } else {
                    FileMode::Source
                },
                preview_view,
            },
            preview,
        };

        if preview {
            if let Some(index) = self.tabs.iter().position(|t| t.preview) {
                self.tabs[index] = tab;
                self.selected = index;
                return;
            }
        }
        self.tabs.push(tab);
        self.selected = self.tabs.len() - 1;
    }

    pub fn open_diff(&mut self, path: String, staged: bool, untracked: bool) {
        let text = git::diff(&self.root, &path, staged, untracked);
        if let Some(index) = self.tabs.iter().position(|tab| {
            matches!(&tab.kind, TabKind::Diff { path: p, staged: s, .. } if p == &path && *s == staged)
        }) {
            self.tabs[index].kind = TabKind::Diff {
                path,
                staged,
                text,
            };
            self.selected = index;
            return;
        }
        let title = format!(
            "{} (diff)",
            Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone())
        );
        let id = self.next_id();
        self.tabs.push(Tab {
            id,
            title,
            kind: TabKind::Diff { path, staged, text },
            preview: false,
        });
        self.selected = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // DESIGN.md: the last terminal stays open.
        let terminals = self.tabs.iter().filter(|t| t.is_terminal()).count();
        if self.tabs[index].is_terminal() && terminals <= 1 {
            return;
        }
        self.tabs.remove(index);
        if self.selected >= self.tabs.len() {
            self.selected = self.tabs.len().saturating_sub(1);
        }
    }

    pub fn toggle_mode(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.selected)
            && let TabKind::File {
                mode, preview_view, ..
            } = &mut tab.kind
            && preview_view.is_some()
        {
            *mode = match mode {
                FileMode::Source => FileMode::Preview,
                FileMode::Preview => FileMode::Source,
            };
        }
    }
}
