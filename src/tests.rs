//! Deterministic, non-UI coverage.
//!
//! Everything here runs without a window, so `cargo test` stays fast and does
//! not need a display.

use std::fs;

use crate::services::{file_index, fs_tree, highlight, search, watch};
use crate::terminal::keys;
use crate::theme::{Colors, LayoutMode};

fn fixture_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("artifex-test-{}", std::process::id()));
    let _ = fs::create_dir_all(dir.join("src"));
    let _ = fs::create_dir_all(dir.join("node_modules/pkg"));
    let _ = fs::create_dir_all(dir.join(".git"));
    fs::write(dir.join("src/main.rs"), "fn main() { println!(\"hi\"); }\n").unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i32) -> i32 { a + 1 }\n",
    )
    .unwrap();
    fs::write(
        dir.join("node_modules/pkg/index.js"),
        "module.exports = 1;\n",
    )
    .unwrap();
    fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(dir.join("README.md"), "# Title\n\nbody\n").unwrap();
    dir
}

#[test]
fn file_tree_hides_hard_ignored_directories() {
    let dir = fixture_dir();
    let tree = fs_tree::FileTree::new(dir.clone());
    let names: Vec<&str> = tree.rows.iter().map(|r| r.entry.name.as_str()).collect();
    assert!(names.contains(&"src"));
    assert!(names.contains(&"README.md"));
    assert!(!names.contains(&"node_modules"));
    assert!(!names.contains(&".git"));
}

#[test]
fn file_tree_expands_lazily() {
    let dir = fixture_dir();
    let mut tree = fs_tree::FileTree::new(dir.clone());
    let before = tree.rows.len();
    tree.toggle(&dir.join("src"));
    assert!(tree.rows.len() > before, "expanding must reveal children");
    assert!(tree.is_expanded(&dir.join("src")));
    tree.toggle(&dir.join("src"));
    assert_eq!(tree.rows.len(), before);
}

#[test]
fn file_index_skips_ignored_trees() {
    let dir = fixture_dir();
    let files = file_index::build(&dir);
    let relatives: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
    assert!(relatives.contains(&"src/main.rs"));
    assert!(!relatives.iter().any(|p| p.starts_with("node_modules")));
    assert!(
        files
            .iter()
            .all(|f| f.haystack == f.relative.to_lowercase())
    );
}

#[test]
fn search_streams_batches_and_can_be_cancelled() {
    let dir = fixture_dir();
    let files = file_index::build(&dir);

    let mut batches = Vec::new();
    let total = search::run(&files, "fn", false, false, search::Cancel::new(), |batch| {
        batches.push(batch)
    });
    assert!(total > 0);
    assert!(!batches.is_empty());
    assert!(batches.iter().all(|b| !b.hits.is_empty()));

    let cancel = search::Cancel::new();
    cancel.cancel();
    let mut none = Vec::new();
    let cancelled = search::run(&files, "fn", false, false, cancel, |b| none.push(b));
    assert_eq!(cancelled, 0);
    assert!(none.is_empty());
}

#[test]
fn search_respects_case_and_whole_word() {
    let dir = fixture_dir();
    let files = file_index::build(&dir);

    let sensitive = search::run(
        &files,
        "PRINTLN",
        true,
        false,
        search::Cancel::new(),
        |_| {},
    );
    assert_eq!(sensitive, 0);

    let insensitive = search::run(
        &files,
        "PRINTLN",
        false,
        false,
        search::Cancel::new(),
        |_| {},
    );
    assert!(insensitive > 0);

    let partial = search::run(&files, "mai", false, true, search::Cancel::new(), |_| {});
    assert_eq!(partial, 0, "whole word must not match a prefix");
}

#[test]
fn line_starts_track_every_newline() {
    let starts = highlight::line_starts("a\nbb\n\nccc");
    assert_eq!(starts, vec![0, 2, 5, 6]);
}

#[test]
fn highlighting_is_scoped_to_the_requested_byte_range() {
    let source = (0..400)
        .map(|i| format!("fn f{i}() -> u32 {{ {i} }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let starts = highlight::line_starts(&source);
    let colors = Colors::for_test();

    let mut highlighter = highlight::Highlighter::new(highlight::Lang::Rust);
    highlighter.parse(&source);

    let window = starts[10]..starts[20];
    let spans = highlighter.spans_in(&source, window, &starts, &colors);

    assert!(!spans.is_empty(), "the visible window must be highlighted");
    assert!(
        spans.keys().all(|row| (10..=20).contains(row)),
        "no row outside the window may be highlighted, got {:?}",
        spans.keys().collect::<Vec<_>>()
    );
}

#[test]
fn overlapping_captures_resolve_to_the_narrowest() {
    use highlight::Span;
    let colors = Colors::for_test();
    // A wide `variable` capture with a narrow `type` capture inside it.
    let resolved = highlight::resolve_overlaps_for_test(vec![
        Span {
            start: 0,
            end: 20,
            color: colors.ink,
        },
        Span {
            start: 5,
            end: 9,
            color: colors.workflow_todo,
        },
    ]);

    let ranges: Vec<(usize, usize)> = resolved.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(ranges, vec![(0, 5), (5, 9), (9, 20)]);
    assert_eq!(resolved[1].color, colors.workflow_todo, "narrowest wins");
    assert_eq!(resolved[0].color, colors.ink);
    assert_eq!(resolved[2].color, colors.ink);
}

#[test]
fn resolved_spans_never_overlap_and_merge_equal_neighbours() {
    use highlight::Span;
    let colors = Colors::for_test();
    let resolved = highlight::resolve_overlaps_for_test(vec![
        Span {
            start: 0,
            end: 10,
            color: colors.ink,
        },
        Span {
            start: 4,
            end: 12,
            color: colors.ink,
        },
        Span {
            start: 6,
            end: 7,
            color: colors.accent,
        },
    ]);

    for pair in resolved.windows(2) {
        assert!(
            pair[0].end <= pair[1].start,
            "spans must not overlap: {:?}",
            resolved
                .iter()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>()
        );
    }
    // The two ink runs around the accent byte stay separate; everything else merges.
    let ranges: Vec<(usize, usize)> = resolved.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(ranges, vec![(0, 6), (6, 7), (7, 12)]);
}

#[test]
fn real_highlighting_produces_no_overlapping_spans() {
    let source = "pub fn build(name: String) -> Option<Vec<u32>> { None }\n\
                  struct Widget { label: String }\n\
                  const LIMIT: usize = 10;\n";
    let starts = highlight::line_starts(source);
    let colors = Colors::for_test();

    let mut highlighter = highlight::Highlighter::new(highlight::Lang::Rust);
    highlighter.parse(source);
    let spans = highlighter.spans_in(source, 0..source.len(), &starts, &colors);

    assert!(!spans.is_empty());
    for (row, row_spans) in &spans {
        for pair in row_spans.windows(2) {
            assert!(
                pair[0].end <= pair[1].start,
                "row {row} has overlapping spans: {:?}",
                row_spans
                    .iter()
                    .map(|s| (s.start, s.end))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn language_is_resolved_from_the_extension() {
    use std::path::Path;
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.rs")),
        highlight::Lang::Rust
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.swift")),
        highlight::Lang::Swift
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.txt")),
        highlight::Lang::None
    );
}

#[test]
fn layout_mode_follows_the_design_breakpoints() {
    assert_eq!(LayoutMode::for_width(899.), LayoutMode::Compact);
    assert_eq!(LayoutMode::for_width(900.), LayoutMode::Standard);
    assert_eq!(LayoutMode::for_width(1279.), LayoutMode::Standard);
    assert_eq!(LayoutMode::for_width(1280.), LayoutMode::Wide);

    assert!(!LayoutMode::Compact.allows_sidebar());
    assert!(LayoutMode::Standard.allows_sidebar());
    assert!(!LayoutMode::Standard.allows_inspector());
    assert!(LayoutMode::Wide.allows_inspector());
}

#[test]
fn keys_encode_control_sequences_but_never_printable_text() {
    use alacritty_terminal::term::TermMode;
    use gpui::Keystroke;

    let key = |s: &str| Keystroke::parse(s).unwrap();

    assert_eq!(
        keys::encode(&key("enter"), TermMode::empty()),
        Some(b"\r".to_vec())
    );
    assert_eq!(
        keys::encode(&key("shift-enter"), TermMode::empty()),
        Some(b"\n".to_vec())
    );
    assert_eq!(
        keys::encode(&key("backspace"), TermMode::empty()),
        Some(b"\x7f".to_vec())
    );
    assert_eq!(
        keys::encode(&key("ctrl-c"), TermMode::empty()),
        Some(vec![3])
    );
    assert_eq!(
        keys::encode(&key("up"), TermMode::empty()),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        keys::encode(&key("up"), TermMode::APP_CURSOR),
        Some(b"\x1bOA".to_vec())
    );
    // Printable characters belong to the input handler, not the key path.
    assert_eq!(keys::encode(&key("a"), TermMode::empty()), None);
    assert_eq!(keys::encode(&key("cmd-s"), TermMode::empty()), None);
}

#[test]
fn git_snapshot_reads_this_repository() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let snapshot = crate::services::git::snapshot(root);
    assert!(snapshot.is_repo, "the POC directory is a git repository");
    assert!(!snapshot.branch.is_empty());
    // Counting is by path, so a file that is both staged and modified counts once.
    assert!(snapshot.changed_count() <= snapshot.staged.len() + snapshot.unstaged.len());
}

/// A throwaway repository with one commit, so `status` has a HEAD tree to
/// compare against. Each call gets its own directory, so nothing is deleted.
fn temp_repo(label: &str) -> std::path::PathBuf {
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!("artifex-git-{}-{label}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(&dir)
            .args(args)
            .output()
            .expect("git must be installed")
    };

    git(&["init", "-q"]);
    git(&["config", "user.email", "poc@example.com"]);
    git(&["config", "user.name", "POC"]);
    fs::write(dir.join("tracked.txt"), "one\n").unwrap();
    git(&["add", "tracked.txt"]);
    git(&["commit", "-q", "-m", "initial"]);
    dir
}

#[test]
fn untracked_files_are_listed_as_leaves_not_directories() {
    use crate::services::git;

    let dir = temp_repo("untracked");
    fs::create_dir_all(dir.join("fresh/deep")).unwrap();
    fs::write(dir.join("fresh/deep/note.txt"), "hello\n").unwrap();
    fs::write(dir.join("fresh/top.txt"), "hi\n").unwrap();

    let snapshot = git::snapshot(&dir);
    let untracked: Vec<&str> = snapshot
        .unstaged
        .iter()
        .filter(|change| change.kind == git::ChangeKind::Untracked)
        .map(|change| change.path.as_str())
        .collect();

    assert!(
        untracked.contains(&"fresh/deep/note.txt"),
        "expected the leaf path, got {untracked:?}"
    );
    assert!(
        untracked.contains(&"fresh/top.txt"),
        "expected the leaf path, got {untracked:?}"
    );
    assert!(
        !untracked
            .iter()
            .any(|path| *path == "fresh" || *path == "fresh/"),
        "a collapsed directory must not appear, got {untracked:?}"
    );
    // Every reported path must resolve to a real file the panel can stage.
    for path in &untracked {
        assert!(dir.join(path).is_file(), "{path} does not point at a file");
    }
}

#[test]
fn empty_untracked_directories_are_skipped() {
    use crate::services::git;

    let dir = temp_repo("empty-dir");
    fs::create_dir_all(dir.join("hollow")).unwrap();

    let snapshot = git::snapshot(&dir);
    let untracked: Vec<&str> = snapshot
        .unstaged
        .iter()
        .filter(|change| change.kind == git::ChangeKind::Untracked)
        .map(|change| change.path.as_str())
        .collect();

    assert!(
        untracked.is_empty(),
        "an empty directory has nothing to stage, got {untracked:?}"
    );
}

#[test]
fn markdown_parses_tables_lists_and_code() {
    use crate::app::markdown::parse_summary;

    let source = "# Title\n\n\
                  | Field | Value |\n\
                  |---|---|\n\
                  | Status | Current |\n\
                  | Updated | Today |\n\n\
                  - first\n- second\n\n\
                  ```bash\ncargo build\n```\n\n\
                  > a quote\n";
    let summary = parse_summary(source);

    assert!(summary.contains(&("heading", 1)));
    assert!(
        summary.contains(&("table", 3)),
        "the table needs a header row plus two body rows, got {summary:?}"
    );
    assert_eq!(
        summary.iter().filter(|(kind, _)| *kind == "list").count(),
        2
    );
    assert!(summary.contains(&("code", 1)));
    assert!(summary.iter().any(|(kind, _)| *kind == "quote"));
}

#[test]
fn recent_commits_are_read() {
    use crate::services::git;

    let dir = temp_repo("commits");
    let snapshot = git::snapshot(&dir);
    println!("commits: {:?}", snapshot.commits);
    assert!(!snapshot.commits.is_empty(), "the fixture has one commit");
    let head = &snapshot.commits[0];
    assert_eq!(head.subject, "initial");
    assert_eq!(head.author, "POC");
    assert_eq!(head.short_hash.len(), 7);
}

/// A root carrying one `.gitignore`, so the watcher's ignore matcher has
/// something to read.
fn watch_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("artifex-watch-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".gitignore"), "build/\n*.log\n").unwrap();
    dir
}

#[test]
fn a_write_asks_for_git_only_and_a_rename_asks_for_the_index() {
    use notify::event::{CreateKind, DataChange, EventKind, ModifyKind, RenameMode};

    let root = watch_root();
    let write = EventKind::Modify(ModifyKind::Data(DataChange::Content));
    let create = EventKind::Create(CreateKind::File);
    let rename = EventKind::Modify(ModifyKind::Name(RenameMode::Any));

    // Saving an existing file cannot change the file set, so the expensive
    // index walk must be skipped.
    assert_eq!(
        watch::classify_for_test(&root, &root.join("src/main.rs"), &write),
        Some((false, true))
    );
    assert_eq!(
        watch::classify_for_test(&root, &root.join("src/main.rs"), &create),
        Some((true, true))
    );
    assert_eq!(
        watch::classify_for_test(&root, &root.join("src/main.rs"), &rename),
        Some((true, true))
    );
}

#[test]
fn ignored_trees_never_reach_the_debouncer() {
    use notify::event::{CreateKind, EventKind};

    let root = watch_root();
    let create = EventKind::Create(CreateKind::File);
    let dropped = [
        "target/debug/artifex",
        "node_modules/pkg/index.js",
        "docs/dist/bundle.js",
        // Matched by the fixture's own .gitignore.
        "build/output.o",
        "run.log",
    ];

    for path in dropped {
        assert_eq!(
            watch::classify_for_test(&root, &root.join(path), &create),
            None,
            "{path} must be filtered before it costs a walk"
        );
    }
}

#[test]
fn git_metadata_refreshes_the_snapshot_without_a_walk() {
    use notify::event::{DataChange, EventKind, ModifyKind};

    let root = watch_root();
    let write = EventKind::Modify(ModifyKind::Data(DataChange::Content));

    for path in [".git/HEAD", ".git/index", ".git/refs/heads/main"] {
        assert_eq!(
            watch::classify_for_test(&root, &root.join(path), &write),
            Some((false, true)),
            "{path} changes git state only"
        );
    }
    // Object churn is the bulk of `.git` traffic and no surface reads it.
    assert_eq!(
        watch::classify_for_test(&root, &root.join(".git/objects/ab/cdef"), &write),
        None
    );
}

#[test]
fn terminal_size_never_collapses() {
    use crate::terminal::TermSize;
    use alacritty_terminal::grid::Dimensions;
    let size = TermSize::for_test(0, 0);
    assert!(size.columns() >= 2);
    assert!(size.screen_lines() >= 1);
}
