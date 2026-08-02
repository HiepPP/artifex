//! Workspace filesystem watcher.
//!
//! One `RecommendedWatcher` and one debounce thread serve every workspace.
//! macOS resolves `RecommendedWatcher` to FSEvents, which is kernel driven and
//! recursive, so an idle workspace costs no polling, no walk and no per-folder
//! watch descriptor. The debounce thread blocks on an empty channel, so an
//! untouched workspace wakes nothing at all.
//!
//! Two filters keep the expensive work rare, because a rebuild of the file
//! index walks the whole tree:
//!
//! - A path under the hard ignore list or the root `.gitignore` is dropped
//!   before it reaches the debouncer, so a `cargo build` costs nothing.
//! - A write into a file that already exists asks for a Git snapshot only. Only
//!   a create, a remove or a rename can change the file set, so only those ask
//!   for an index rebuild.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};
use parking_lot::Mutex;

use super::fs_tree::HARD_IGNORES;

/// Trailing quiet period. One editor save is a write, a rename and a chmod;
/// they have to land as one refresh rather than three.
const QUIET: Duration = Duration::from_millis(250);

/// Hard flush interval. A process that writes continuously would otherwise keep
/// resetting the quiet timer and starve the UI of every update.
const MAX_LATENCY: Duration = Duration::from_secs(1);

/// Files under `.git` that describe HEAD, the index or the refs. Everything
/// else there is object churn, which no surface reads.
const GIT_METADATA: &[&str] = &["HEAD", "index", "ORIG_HEAD", "MERGE_HEAD", "refs"];

/// What one root needs once a burst of filesystem events settles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootChange {
    pub root: PathBuf,
    /// The set of files changed, so the index and the tree are both stale.
    pub index: bool,
    /// Tracked content or Git metadata changed, so the snapshot is stale.
    pub git: bool,
}

impl RootChange {
    fn empty(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            index: false,
            git: false,
        }
    }
}

/// The refresh one path is worth.
#[derive(Clone, Copy)]
struct Effect {
    index: bool,
    git: bool,
}

struct Root {
    path: PathBuf,
    /// Root-level `.gitignore`, rebuilt whenever that file is written. Nested
    /// ignore files are not read; the walk that follows honours them, so the
    /// only cost of missing one here is a rebuild that finds nothing new.
    ignore: Gitignore,
}

impl Root {
    fn new(path: PathBuf) -> Self {
        let ignore = build_ignore(&path);
        Self { path, ignore }
    }

    /// What `relative` is worth inside this root, or `None` to drop it.
    fn effect(&self, relative: &Path, structural: bool) -> Option<Effect> {
        let mut names = relative.components().filter_map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        });
        let first = names.next()?;

        if first == ".git" {
            let head = names.next()?;
            if !GIT_METADATA.contains(&head) {
                return None;
            }
            return Some(Effect {
                index: false,
                git: true,
            });
        }

        if HARD_IGNORES.contains(&first) || names.any(|name| HARD_IGNORES.contains(&name)) {
            return None;
        }
        if self
            .ignore
            .matched_path_or_any_parents(relative, false)
            .is_ignore()
        {
            return None;
        }

        Some(Effect {
            index: structural,
            git: true,
        })
    }
}

#[derive(Default)]
struct Registry {
    roots: Vec<Root>,
}

impl Registry {
    /// The deepest registered root that contains `path`. Workspaces can nest,
    /// and the innermost one is the one whose surfaces show the file.
    fn root_index_for(&self, path: &Path) -> Option<usize> {
        self.roots
            .iter()
            .enumerate()
            .filter(|(_, root)| path.starts_with(&root.path))
            .max_by_key(|(_, root)| root.path.as_os_str().len())
            .map(|(index, _)| index)
    }

    fn fold(&mut self, event: &Event, out: &mut HashMap<PathBuf, RootChange>) {
        // A dropped-event notice means the stream is no longer a complete
        // record, so every root has to be read from disk again.
        if event.need_rescan() {
            for root in &self.roots {
                let entry = out
                    .entry(root.path.clone())
                    .or_insert_with(|| RootChange::empty(&root.path));
                entry.index = true;
                entry.git = true;
            }
            return;
        }
        // Opening or reading a file changes nothing any surface shows.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let structural = is_structural(&event.kind);

        for path in &event.paths {
            let Some(index) = self.root_index_for(path) else {
                continue;
            };
            let classified = self.roots.get(index).and_then(|root| {
                let relative = path.strip_prefix(&root.path).ok()?;
                // A rewritten ignore file changes what every later event means,
                // so it always forces a full rebuild.
                let rewrote_ignore = relative == Path::new(".gitignore");
                let effect = root.effect(relative, structural || rewrote_ignore)?;
                Some((root.path.clone(), effect, rewrote_ignore))
            });
            let Some((key, effect, rewrote_ignore)) = classified else {
                continue;
            };

            if rewrote_ignore && let Some(root) = self.roots.get_mut(index) {
                root.ignore = build_ignore(&key);
            }
            let entry = out
                .entry(key.clone())
                .or_insert_with(|| RootChange::empty(&key));
            entry.index |= effect.index;
            entry.git |= effect.git;
        }
    }
}

/// Owns the watcher, the registered roots and the debounced change stream.
pub struct WatchHub {
    /// Dropping the watcher stops the FSEvents stream, so it is held here even
    /// though nothing reads it after a root is registered.
    watcher: RecommendedWatcher,
    registry: Arc<Mutex<Registry>>,
    pub changes: async_channel::Receiver<Vec<RootChange>>,
}

impl WatchHub {
    /// `None` when the platform refuses a watcher. The application still runs;
    /// the manual refresh control stays the way to reload.
    pub fn new() -> Option<Self> {
        let (raw_tx, raw_rx) = mpsc::channel::<Event>();
        // The notify callback runs on the backend's own thread, so it does no
        // work beyond handing the event to the debouncer.
        let watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            if let Ok(event) = result {
                let _ = raw_tx.send(event);
            }
        })
        .ok()?;

        let registry = Arc::new(Mutex::new(Registry::default()));
        let (tx, changes) = async_channel::unbounded();
        let thread_registry = Arc::clone(&registry);
        std::thread::Builder::new()
            .name("rustelier-watch".into())
            .spawn(move || debounce(raw_rx, thread_registry, tx))
            .ok()?;

        Some(Self {
            watcher,
            registry,
            changes,
        })
    }

    /// Registers `root`. Returns false when the path is not worth watching or
    /// the platform refused it.
    pub fn watch(&mut self, root: &Path) -> bool {
        if !is_watchable(root) {
            return false;
        }
        {
            let registry = self.registry.lock();
            if registry.roots.iter().any(|entry| entry.path == root) {
                return true;
            }
        }
        if self.watcher.watch(root, RecursiveMode::Recursive).is_err() {
            return false;
        }
        self.registry.lock().roots.push(Root::new(root.to_path_buf()));
        true
    }
}

/// Refuses the home directory and the filesystem root.
///
/// A recursive watch on either covers caches, mail and every other application's
/// state. The event rate there is large, constant, and never about a workspace,
/// so the cheapest filter is to not open the stream at all.
fn is_watchable(root: &Path) -> bool {
    if root.parent().is_none() {
        return false;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    !matches!(home, Some(home) if home == root)
}

/// True when the event can add, remove or rename a path, which is the only
/// reason to walk the workspace again. A write into a file that already exists
/// changes Git status and nothing else.
fn is_structural(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Name(_)) => true,
        EventKind::Modify(_) => false,
        // `Any` and `Other` carry no detail, so assume the worst.
        _ => true,
    }
}

fn build_ignore(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    builder.add(root.join(".gitignore"));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

/// Collapses a burst of events into one batch per root.
///
/// The outer receive has no timeout, so an idle workspace parks this thread on
/// the channel instead of waking it on a timer.
fn debounce(
    raw: mpsc::Receiver<Event>,
    registry: Arc<Mutex<Registry>>,
    out: async_channel::Sender<Vec<RootChange>>,
) {
    let mut pending: HashMap<PathBuf, RootChange> = HashMap::new();

    loop {
        let Ok(event) = raw.recv() else {
            return;
        };
        registry.lock().fold(&event, &mut pending);
        if pending.is_empty() {
            continue;
        }

        let started = Instant::now();
        loop {
            let budget = MAX_LATENCY.saturating_sub(started.elapsed());
            if budget.is_zero() {
                break;
            }
            match raw.recv_timeout(QUIET.min(budget)) {
                Ok(event) => registry.lock().fold(&event, &mut pending),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }

        let batch: Vec<RootChange> = pending.drain().map(|(_, change)| change).collect();
        if out.send_blocking(batch).is_err() {
            return;
        }
    }
}

#[cfg(test)]
pub fn classify_for_test(root: &Path, path: &Path, kind: &EventKind) -> Option<(bool, bool)> {
    let entry = Root::new(root.to_path_buf());
    let relative = path.strip_prefix(root).ok()?;
    entry
        .effect(relative, is_structural(kind))
        .map(|effect| (effect.index, effect.git))
}
