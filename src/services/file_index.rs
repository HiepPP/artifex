//! Workspace file index.
//!
//! One ordered snapshot of candidate files, shared by Quick Open and Search All
//! Files. Built with `ignore`, so `.gitignore` and the hard ignore list are both
//! honoured in one walk.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use super::fs_tree::{HARD_IGNORES, TCC_PROTECTED};

/// Files above this size are never indexed or searched.
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct IndexedFile {
    pub absolute: PathBuf,
    pub relative: String,
    pub name: String,
    /// Lowercased relative path, precomputed so the ranking path never
    /// lowercases per keystroke.
    pub haystack: String,
}

pub fn build(root: &Path) -> Vec<IndexedFile> {
    let mut files = Vec::new();
    let root_is_home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| std::fs::canonicalize(home).ok())
        .is_some_and(|home| std::fs::canonicalize(root).ok() == Some(home));
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        .follow_links(false)
        .filter_entry(move |entry| {
            let Some(name) = entry.file_name().to_str() else {
                return true;
            };
            if HARD_IGNORES.contains(&name) {
                return false;
            }
            // Walking a privacy-gated folder under the home root fires one
            // macOS permission prompt per folder on a plain Dock launch.
            !(root_is_home && entry.depth() == 1 && TCC_PROTECTED.contains(&name))
        })
        .build();

    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if entry
            .metadata()
            .map(|meta| meta.len() > MAX_TEXT_BYTES)
            .unwrap_or(true)
        {
            continue;
        }
        let absolute = entry.into_path();
        let Ok(relative) = absolute.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().to_string();
        let name = absolute
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let haystack = relative.to_lowercase();
        files.push(IndexedFile {
            absolute,
            relative,
            name,
            haystack,
        });
    }

    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    files
}
