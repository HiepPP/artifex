//! Lazy file tree.
//!
//! One flat `Vec<Row>` of visible rows, rebuilt only when a folder is toggled.
//! Children are read on first expansion, never up front, so opening a large
//! workspace costs one directory read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Directories that are never walked.
pub const HARD_IGNORES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".build",
    "dist",
    ".next",
    "DerivedData",
    ".venv",
];

/// Direct children of the home directory that macOS gates behind a privacy
/// prompt. Skipped by the index walk only when the workspace root is the home
/// directory itself, so a Dock launch does not fire one prompt per folder.
pub const TCC_PROTECTED: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Movies",
    "Library",
];

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// A visible row: one entry plus its depth.
#[derive(Clone, Debug)]
pub struct Row {
    pub entry: Entry,
    pub depth: usize,
    pub expanded: bool,
}

pub struct FileTree {
    root: PathBuf,
    children: HashMap<PathBuf, Vec<Entry>>,
    expanded: Vec<PathBuf>,
    pub rows: Vec<Row>,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root,
            children: HashMap::new(),
            expanded: Vec::new(),
            rows: Vec::new(),
        };
        tree.rebuild();
        tree
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.iter().any(|p| p == path)
    }

    pub fn toggle(&mut self, path: &Path) {
        if let Some(index) = self.expanded.iter().position(|p| p == path) {
            self.expanded.remove(index);
        } else {
            self.expanded.push(path.to_path_buf());
            self.ensure_children(path);
        }
        self.rebuild();
    }

    /// Expands every folder on the way to `path` so a reveal request lands on a
    /// visible row.
    pub fn reveal(&mut self, path: &Path) {
        let mut current = path.parent();
        while let Some(dir) = current {
            if !dir.starts_with(&self.root) && dir != self.root {
                break;
            }
            if !self.is_expanded(dir) {
                self.expanded.push(dir.to_path_buf());
                self.ensure_children(dir);
            }
            if dir == self.root {
                break;
            }
            current = dir.parent();
        }
        self.rebuild();
    }

    pub fn refresh(&mut self) {
        self.children.clear();
        let expanded = self.expanded.clone();
        for path in &expanded {
            self.ensure_children(path);
        }
        self.rebuild();
    }

    fn ensure_children(&mut self, path: &Path) {
        if self.children.contains_key(path) {
            return;
        }
        self.children.insert(path.to_path_buf(), read_dir(path));
    }

    fn rebuild(&mut self) {
        self.ensure_children(&self.root.clone());
        let mut rows = Vec::new();
        let root = self.root.clone();
        self.push_level(&root, 0, &mut rows);
        self.rows = rows;
    }

    fn push_level(&mut self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        let entries = self.children.get(dir).cloned().unwrap_or_default();
        for entry in entries {
            let expanded = entry.is_dir && self.is_expanded(&entry.path);
            let path = entry.path.clone();
            rows.push(Row {
                entry,
                depth,
                expanded,
            });
            if expanded {
                self.ensure_children(&path);
                self.push_level(&path, depth + 1, rows);
            }
        }
    }
}

fn read_dir(path: &Path) -> Vec<Entry> {
    let Ok(reader) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = reader
        .filter_map(|item| item.ok())
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().to_string();
            if HARD_IGNORES.contains(&name.as_str()) {
                return None;
            }
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some(Entry {
                path: item.path(),
                name,
                is_dir,
            })
        })
        .collect();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    entries
}
