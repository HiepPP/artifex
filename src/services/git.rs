//! Git state.
//!
//! Reads go through `gix`. Every mutation shells out to `git`, which keeps the
//! POC away from `gix`'s still-moving write APIs and matches how the Swift app
//! stages and commits.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    pub fn short(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "U",
            Self::Conflicted => "!",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub staged: bool,
}

/// One entry in the recent-commit list.
#[derive(Clone, Debug)]
pub struct Commit {
    pub short_hash: String,
    pub subject: String,
    pub author: String,
    /// Seconds since the epoch, so the relative label is computed at render.
    pub seconds: i64,
}

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub is_repo: bool,
    pub branch: String,
    pub head_short: String,
    pub staged: Vec<Change>,
    pub unstaged: Vec<Change>,
    pub commits: Vec<Commit>,
}

impl GitSnapshot {
    pub fn changed_count(&self) -> usize {
        let mut paths: Vec<&str> = self
            .staged
            .iter()
            .chain(self.unstaged.iter())
            .map(|c| c.path.as_str())
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths.len()
    }
}

/// Reads branch, HEAD and the working-tree status through `gix`.
pub fn snapshot(root: &Path) -> GitSnapshot {
    let Ok(repo) = gix::discover(root) else {
        return GitSnapshot::default();
    };

    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string())
        .unwrap_or_else(|| "HEAD".to_string());

    let head_short = repo
        .head_id()
        .ok()
        .map(|id| id.to_hex_with_len(7).to_string())
        .unwrap_or_default();

    let (staged, unstaged) = status(&repo);
    let commits = recent_commits(&repo, 12);

    GitSnapshot {
        is_repo: true,
        branch,
        head_short,
        staged,
        unstaged,
        commits,
    }
}

/// Walks the first parents from HEAD. Read-only, like every other `gix` call
/// here; anything that writes goes through the CLI below.
fn recent_commits(repo: &gix::Repository, limit: usize) -> Vec<Commit> {
    let Ok(head) = repo.head_id() else {
        return Vec::new();
    };
    let Ok(walk) = repo.rev_walk([head]).first_parent_only().all() else {
        return Vec::new();
    };

    walk.take(limit)
        .filter_map(|info| {
            let info = info.ok()?;
            let commit = info.object().ok()?;
            let subject = commit.message().ok()?.summary().to_string();
            let author = commit.author().ok()?.name.to_string();
            let seconds = commit.time().ok()?.seconds;
            Some(Commit {
                short_hash: info.id.to_hex_with_len(7).to_string(),
                subject,
                author,
                seconds,
            })
        })
        .collect()
}

/// A compact relative label, in the style of the recent-commit rows.
pub fn relative_time(seconds: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(seconds);
    let delta = (now - seconds).max(0);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m", delta / 60),
        3600..=86_399 => format!("{}h", delta / 3600),
        86_400..=2_591_999 => format!("{}d", delta / 86_400),
        _ => format!("{}mo", delta / 2_592_000),
    }
}

fn status(repo: &gix::Repository) -> (Vec<Change>, Vec<Change>) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    let Ok(platform) = repo.status(gix::progress::Discard) else {
        return (staged, unstaged);
    };
    // The dirwalk collapses an untracked directory into one entry by default,
    // so a new folder shows up as `src` instead of the files inside it. The
    // panel stages and diffs by path, so it needs the leaves.
    let platform = platform.untracked_files(gix::status::UntrackedFiles::Files);
    let Ok(iter) = platform.into_iter(None) else {
        return (staged, unstaged);
    };

    for item in iter.flatten() {
        use gix::status::Item;
        match item {
            Item::TreeIndex(change) => {
                use gix::diff::index::Change as TreeChange;
                let (path, kind) = match &change {
                    TreeChange::Addition { location, .. } => {
                        (location.to_string(), ChangeKind::Added)
                    }
                    TreeChange::Deletion { location, .. } => {
                        (location.to_string(), ChangeKind::Deleted)
                    }
                    TreeChange::Modification { location, .. } => {
                        (location.to_string(), ChangeKind::Modified)
                    }
                    TreeChange::Rewrite { location, .. } => {
                        (location.to_string(), ChangeKind::Renamed)
                    }
                };
                staged.push(Change {
                    path,
                    kind,
                    staged: true,
                });
            }
            Item::IndexWorktree(change) => {
                use gix::status::index_worktree::Item as IwItem;
                match change {
                    IwItem::Modification {
                        rela_path, status, ..
                    } => {
                        use gix::status::plumbing::index_as_worktree::EntryStatus;
                        let kind = match status {
                            EntryStatus::Conflict { .. } => ChangeKind::Conflicted,
                            EntryStatus::IntentToAdd => ChangeKind::Added,
                            _ => ChangeKind::Modified,
                        };
                        unstaged.push(Change {
                            path: rela_path.to_string(),
                            kind,
                            staged: false,
                        });
                    }
                    IwItem::DirectoryContents { entry, .. } => {
                        // `Files` mode still emits a directory for an empty one,
                        // and nothing can be staged from that.
                        use gix::dir::entry::Kind;
                        if matches!(entry.disk_kind, Some(Kind::Directory) | None) {
                            continue;
                        }
                        unstaged.push(Change {
                            path: entry.rela_path.to_string(),
                            kind: ChangeKind::Untracked,
                            staged: false,
                        });
                    }
                    IwItem::Rewrite { dirwalk_entry, .. } => {
                        unstaged.push(Change {
                            path: dirwalk_entry.rela_path.to_string(),
                            kind: ChangeKind::Renamed,
                            staged: false,
                        });
                    }
                }
            }
        }
    }

    staged.sort_by(|a, b| a.path.cmp(&b.path));
    unstaged.sort_by(|a, b| a.path.cmp(&b.path));
    (staged, unstaged)
}

/// Every write goes through the CLI.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

pub fn stage(root: &Path, path: &str) -> Result<(), String> {
    git(root, &["add", "--", path]).map(|_| ())
}

pub fn unstage(root: &Path, path: &str) -> Result<(), String> {
    git(root, &["restore", "--staged", "--", path]).map(|_| ())
}

pub fn stage_all(root: &Path) -> Result<(), String> {
    git(root, &["add", "-A"]).map(|_| ())
}

/// Stage everything, commit with `subject`, then push the current branch.
///
/// `DESIGN.md` folds these into one visible Push action. The POC keeps the
/// same shape and reports the stage it failed at.
pub fn commit_and_push(root: &Path, subject: &str) -> Result<String, String> {
    if subject.trim().is_empty() {
        return Err("commit subject is empty".to_string());
    }
    stage_all(root)?;
    git(root, &["commit", "-m", subject]).map_err(|err| format!("commit failed: {err}"))?;
    match git(root, &["push"]) {
        Ok(_) => Ok("committed and pushed".to_string()),
        Err(err) => Ok(format!("committed; push failed: {}", err.trim())),
    }
}

/// Diff for one path. Staged rows read the index, unstaged rows read the
/// working tree; untracked files have no diff so their content is shown as an
/// all-additions block.
pub fn diff(root: &Path, path: &str, staged: bool, untracked: bool) -> String {
    if untracked {
        let full = root.join(path);
        return std::fs::read_to_string(&full)
            .map(|text| {
                text.lines()
                    .map(|line| format!("+{line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
    }
    let mut args = vec!["diff", "--no-color"];
    if staged {
        args.push("--staged");
    }
    args.push("--");
    args.push(path);
    git(root, &args).unwrap_or_else(|err| err)
}

/// Workspace root for a path, or the path itself when it is not a repository.
pub fn repo_root(path: &Path) -> PathBuf {
    gix::discover(path)
        .ok()
        .and_then(|repo| repo.workdir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| path.to_path_buf())
}
