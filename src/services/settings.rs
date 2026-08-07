//! Durable user preferences. `DESIGN.md` > User Settings is the contract.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsState {
    pub version: u32,
    #[serde(default = "default_true")]
    pub shows_sidebar: bool,
    #[serde(default)]
    pub shows_inspector: bool,
    #[serde(default = "default_zoom")]
    pub content_zoom: f32,
    #[serde(default = "default_zoom")]
    pub ui_zoom: f32,
    /// `None` follows the system appearance until the first settings write.
    #[serde(default)]
    pub dark: Option<bool>,
    #[serde(default)]
    pub word_wrap: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            version: VERSION,
            shows_sidebar: true,
            shows_inspector: false,
            content_zoom: default_zoom(),
            ui_zoom: default_zoom(),
            dark: None,
            word_wrap: false,
        }
    }
}

impl SettingsState {
    fn normalized(mut self) -> Self {
        self.content_zoom = if self.content_zoom.is_finite() {
            let bounded = self.content_zoom.clamp(0.8, 2.0);
            (bounded * 10.0).round() / 10.0
        } else {
            default_zoom()
        };
        self.ui_zoom = if self.ui_zoom.is_finite() {
            let bounded = self.ui_zoom.clamp(0.8, 1.4);
            (bounded * 10.0).round() / 10.0
        } else {
            default_zoom()
        };
        self
    }
}

fn default_true() -> bool {
    true
}

fn default_zoom() -> f32 {
    1.0
}

/// `~/Library/Application Support/Artifex/settings.json`.
pub fn state_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(
        home.join("Library")
            .join("Application Support")
            .join("Artifex")
            .join("settings.json"),
    )
}

pub fn load() -> Option<SettingsState> {
    load_from(&state_path()?)
}

pub fn load_from(path: &Path) -> Option<SettingsState> {
    let text = fs::read_to_string(path).ok()?;
    let state: SettingsState = serde_json::from_str(&text).ok()?;
    (state.version == VERSION).then(|| state.normalized())
}

pub fn save(state: &SettingsState) {
    let Some(path) = state_path() else {
        return;
    };
    save_to(state, &path);
}

/// Writes through a sibling temp file so an interrupted write stays valid.
pub fn save_to(state: &SettingsState, path: &Path) {
    let Some(dir) = path.parent() else {
        return;
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(&state.clone().normalized()) else {
        return;
    };
    let temp = path.with_extension("json.tmp");
    if fs::write(&temp, text).is_err() {
        return;
    }
    let _ = fs::rename(&temp, path);
}

#[derive(Clone, Copy)]
pub struct WordWrap(pub bool);

impl Global for WordWrap {}

pub fn set_word_wrap(enabled: bool, cx: &mut App) {
    cx.set_global(WordWrap(enabled));
}

pub fn word_wrap(cx: &App) -> bool {
    cx.try_global::<WordWrap>().is_some_and(|state| state.0)
}
