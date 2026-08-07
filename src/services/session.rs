//! Session persistence: which workspaces are open and which file tabs each
//! one holds. `DESIGN.md` > Session Persistence is the contract.
//!
//! Only durable state is stored. Terminal tabs are recreated fresh (a PTY
//! cannot be serialized) and diff tabs are dropped (the Git state they showed
//! has moved on). A missing or corrupt file means a default session, never an
//! error surfaced to the user.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bumped when the schema changes shape. A file with a newer version than
/// this build understands is ignored rather than half-read.
const VERSION: u32 = 1;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub version: u32,
    pub active: usize,
    // Read-only migration fields from session files written before durable
    // preferences moved to settings.json. New session files omit them.
    #[serde(default = "default_true", rename = "shows_sidebar", skip_serializing)]
    pub legacy_shows_sidebar: bool,
    #[serde(default, rename = "shows_inspector", skip_serializing)]
    pub legacy_shows_inspector: bool,
    #[serde(default)]
    pub sidebar_tab: SidebarTabState,
    #[serde(default = "default_zoom", rename = "zoom", skip_serializing)]
    pub legacy_zoom: f32,
    #[serde(default, rename = "dark", skip_serializing)]
    pub legacy_dark: Option<bool>,
    pub workspaces: Vec<WorkspaceState>,
}

/// Mirror of the shell's `SidebarTab`, kept here so this module stays
/// window-free.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum SidebarTabState {
    #[default]
    Explorer,
    Git,
}

fn default_true() -> bool {
    true
}

fn default_zoom() -> f32 {
    1.0
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub root: PathBuf,
    /// Path of the selected tab. `None` means a terminal was selected; the
    /// restored terminal takes selection instead.
    pub selected: Option<PathBuf>,
    pub files: Vec<FileTabState>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct FileTabState {
    pub path: PathBuf,
    /// True when a previewable file (Markdown) was showing its preview.
    pub preview: bool,
}

impl SessionState {
    pub fn new(active: usize, workspaces: Vec<WorkspaceState>) -> Self {
        Self {
            version: VERSION,
            active,
            legacy_shows_sidebar: default_true(),
            legacy_shows_inspector: false,
            sidebar_tab: SidebarTabState::default(),
            legacy_zoom: default_zoom(),
            legacy_dark: None,
            workspaces,
        }
    }
}

/// `~/Library/Application Support/Artifex/session.json`.
pub fn state_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(
        home.join("Library")
            .join("Application Support")
            .join("Artifex")
            .join("session.json"),
    )
}

pub fn load() -> Option<SessionState> {
    load_from(&state_path()?)
}

pub fn load_from(path: &Path) -> Option<SessionState> {
    let text = fs::read_to_string(path).ok()?;
    let state: SessionState = serde_json::from_str(&text).ok()?;
    if state.version == VERSION {
        Some(state)
    } else {
        None
    }
}

pub fn save(state: &SessionState) {
    let Some(path) = state_path() else {
        return;
    };
    save_to(state, &path);
}

/// Atomic write: temp file in the same directory, then rename over the
/// target, so a crash mid-write never leaves a truncated session file.
pub fn save_to(state: &SessionState, path: &Path) {
    let Some(dir) = path.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(state) else {
        return;
    };
    let temp = path.with_extension("json.tmp");
    if fs::write(&temp, text).is_err() {
        return;
    }
    let _ = fs::rename(&temp, path);
}
