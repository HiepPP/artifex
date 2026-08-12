//! One live workspace: root, file tree, git state, index and tabs.
//!
//! Sessions stay alive when another workspace is selected. Only the selected
//! workspace renders; the rest keep their tabs, editors and shell processes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{App, AppContext as _, Entity, Window};

use crate::app::diff::DiffView;
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

/// The rendered preview a `File` tab can carry. Markdown parses once per
/// open; an HTML page loads into a native webview created lazily by the
/// shell, because building one needs a window handle.
pub enum PreviewKind {
    Markdown(Entity<MarkdownView>),
    Web(Entity<gpui_wry::WebView>),
}

pub enum TabKind {
    Terminal(Entity<TerminalView>),
    File {
        path: PathBuf,
        editor: Entity<EditorView>,
        mode: FileMode,
        preview_view: Option<PreviewKind>,
    },
    /// An image opens as a pure preview; there is nothing to edit.
    Image {
        path: PathBuf,
    },
    /// A video plays in a native webview (WKWebView owns the player UI).
    /// The view is created lazily by the shell, like an HTML preview.
    Video {
        path: PathBuf,
        view: Option<Entity<gpui_wry::WebView>>,
    },
    Diff {
        path: String,
        staged: bool,
        text: String,
        view: Entity<DiffView>,
    },
    /// An image change compares HEAD against the working tree side by side.
    /// `old` is a temp copy of the HEAD blob; a missing side means the file
    /// was added or deleted.
    ImageDiff {
        path: String,
        old: Option<PathBuf>,
        new: Option<PathBuf>,
    },
}

/// Extensions the image preview accepts. SVG stays out: it is text, and the
/// editor is the more useful surface for it.
pub fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico")
    )
}

/// Extensions the video preview accepts; WKWebView provides the player.
pub fn is_video_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "mov" | "m4v" | "webm")
    )
}

pub fn is_html_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("html" | "htm")
    )
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
            TabKind::File { path, .. } | TabKind::Image { path } | TabKind::Video { path, .. } => {
                Some(path.as_path())
            }
            _ => None,
        }
    }
}

/// Keeps one background walk per workspace at a time.
///
/// A watcher burst can ask for a rescan while the previous one is still
/// walking. Running both would double the CPU cost for one result, so a request
/// that arrives mid-walk is folded into the next one instead.
#[derive(Default)]
pub struct ScanState {
    pub running: bool,
    pub queued_index: bool,
    pub queued_git: bool,
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
    pub scan: ScanState,
    /// File navigation history. `back` holds files left behind, `forward`
    /// holds files backed out of; any plain open clears `forward`.
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
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
            scan: ScanState::default(),
            back: Vec::new(),
            forward: Vec::new(),
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

    /// Applies a scan built off the main thread. Each half is absent when the
    /// request did not ask for it, so a plain file write costs a Git snapshot
    /// and no tree walk.
    pub fn apply_scan(&mut self, files: Option<Vec<IndexedFile>>, git: Option<GitSnapshot>) {
        if let Some(git) = git {
            self.git = git;
        }
        if let Some(files) = files {
            self.set_index(files);
        }
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
        if let Some(current) = self.selected_tab().and_then(|tab| tab.file_path())
            && current != path
        {
            self.back.push(current.to_path_buf());
            self.forward.clear();
        }
        self.open_file_unrecorded(path, preview, cx);
    }

    /// Steps through the file history. `1` is forward, `-1` is back. A file
    /// deleted since it was recorded is dropped and the step continues.
    pub fn navigate(&mut self, step: isize, cx: &mut App) {
        let current = self
            .selected_tab()
            .and_then(|tab| tab.file_path().map(Path::to_path_buf));
        loop {
            let (from, to) = if step < 0 {
                (&mut self.forward, &mut self.back)
            } else {
                (&mut self.back, &mut self.forward)
            };
            // `navigate(-1)` pops `back`; the names above are the push side.
            let Some(path) = to.pop() else {
                return;
            };
            if !path.is_file() {
                continue;
            }
            if let Some(current) = current.clone() {
                from.push(current);
            }
            self.open_file_unrecorded(path, false, cx);
            return;
        }
    }

    fn open_file_unrecorded(&mut self, path: PathBuf, preview: bool, cx: &mut App) {
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
        let id = self.next_id();
        let kind = if is_image_path(&path) {
            TabKind::Image { path }
        } else if is_video_path(&path) {
            TabKind::Video { path, view: None }
        } else {
            let markdown = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("md") | Some("markdown")
            );
            let editor = EditorView::open(path.clone(), cx);
            let preview_view =
                markdown.then(|| PreviewKind::Markdown(MarkdownView::open(path.clone(), cx)));
            TabKind::File {
                path,
                editor,
                mode: if markdown {
                    FileMode::Preview
                } else {
                    FileMode::Source
                },
                preview_view,
            }
        };
        let tab = Tab {
            id,
            title,
            kind,
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

    pub fn open_diff(&mut self, path: String, staged: bool, untracked: bool, cx: &mut App) {
        // DESIGN.md > Git: a video change opens the working-tree player;
        // there is no meaningful text diff for it.
        if is_video_path(Path::new(&path)) {
            let working = self.root.join(&path);
            if working.is_file() {
                if let Some(index) = self
                    .tabs
                    .iter()
                    .position(|tab| tab.file_path() == Some(working.as_path()))
                {
                    self.selected = index;
                    return;
                }
                let title = Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                let id = self.next_id();
                self.tabs.push(Tab {
                    id,
                    title,
                    kind: TabKind::Video {
                        path: working,
                        view: None,
                    },
                    preview: false,
                });
                self.selected = self.tabs.len() - 1;
                return;
            }
        }
        // DESIGN.md > Git: an image change compares HEAD against the working
        // tree side by side instead of a text diff.
        if is_image_path(Path::new(&path)) {
            let old = (!untracked)
                .then(|| git::show_head_copy(&self.root, &path))
                .flatten();
            let working = self.root.join(&path);
            let new = working.is_file().then_some(working);
            let title = format!(
                "{} (diff)",
                Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone())
            );
            if let Some(index) = self.tabs.iter().position(
                |tab| matches!(&tab.kind, TabKind::ImageDiff { path: p, .. } if p == &path),
            ) {
                self.tabs[index].kind = TabKind::ImageDiff { path, old, new };
                self.selected = index;
                return;
            }
            let id = self.next_id();
            self.tabs.push(Tab {
                id,
                title,
                kind: TabKind::ImageDiff { path, old, new },
                preview: false,
            });
            self.selected = self.tabs.len() - 1;
            return;
        }
        let text = git::diff(&self.root, &path, staged, untracked);
        let view = DiffView::new(Path::new(&path), &text, cx);
        if let Some(index) = self.tabs.iter().position(|tab| {
            matches!(&tab.kind, TabKind::Diff { path: p, staged: s, .. } if p == &path && *s == staged)
        }) {
            self.tabs[index].kind = TabKind::Diff {
                path,
                staged,
                text,
                view,
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
            kind: TabKind::Diff {
                path,
                staged,
                text,
                view,
            },
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
                path,
                mode,
                preview_view,
                ..
            } = &mut tab.kind
            // HTML previews exist before their webview: the shell creates it
            // lazily on the first Preview render, because building a native
            // webview needs a window handle.
            && (preview_view.is_some() || is_html_path(path))
        {
            *mode = match mode {
                FileMode::Source => FileMode::Preview,
                FileMode::Preview => FileMode::Source,
            };
        }
    }

    /// Applies the global soft-wrap preference to every open file editor.
    pub fn toggle_wrap(&mut self, enabled: bool, cx: &mut App) {
        for tab in &self.tabs {
            let TabKind::File { editor, .. } = &tab.kind else {
                continue;
            };
            editor.clone().update(cx, |editor, cx| {
                editor.wrap = enabled;
                cx.notify();
            });
        }
    }
}
