//! Deterministic, non-UI coverage.
//!
//! Everything here runs without a window, so `cargo test` stays fast and does
//! not need a display.

use std::fs;

use crate::services::{file_index, fs_tree, highlight, material_icons, search, watch};
use crate::terminal::{
    TermSize, keys, resize_term_preserving_selection, search as terminal_search,
};
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
fn terminal_search_uses_smart_case() {
    let lines = vec![(-1, "Alpha alpha".to_string()), (0, "ALPHA".to_string())];
    let insensitive = terminal_search::find_in_lines(&lines, "alpha");
    assert_eq!(insensitive.len(), 3);
    let sensitive = terminal_search::find_in_lines(&lines, "Alpha");
    assert_eq!(sensitive.len(), 1);
    assert_eq!(sensitive[0].line, -1);
}

#[test]
fn terminal_plain_links_keep_exact_cell_ranges() {
    use terminal_search::{PlainLink, plain_link_at};

    let url = plain_link_at("see (https://example.com).", 8).expect("URL must be detected");
    assert_eq!(url.start, 5);
    assert_eq!(url.end, 24);
    assert!(matches!(url.target, PlainLink::Url(_)));

    let path = plain_link_at("error src/main.rs:12:4", 9).expect("path must be detected");
    assert!(matches!(path.target, PlainLink::Path(_)));
}

#[test]
fn terminal_resize_reflows_primary_screen_and_keeps_selection() {
    use alacritty_terminal::event::{Event, EventListener};
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line, Point, Side};
    use alacritty_terminal::selection::{Selection, SelectionType};
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi;

    #[derive(Clone, Copy)]
    struct Listener;
    impl EventListener for Listener {
        fn send_event(&self, _: Event) {}
    }

    let mut term = Term::new(Config::default(), &TermSize::for_test(8, 3), Listener);
    let mut parser: ansi::Processor = ansi::Processor::new();
    parser.advance(&mut term, b"abcdefghij");
    let mut selection = Selection::new(
        SelectionType::Simple,
        Point::new(Line(0), Column(0)),
        Side::Left,
    );
    selection.update(Point::new(Line(1), Column(1)), Side::Right);
    term.selection = Some(selection);
    assert_eq!(term.selection_to_string().as_deref(), Some("abcdefghij"));

    resize_term_preserving_selection(&mut term, TermSize::for_test(5, 3));
    assert_eq!(term.columns(), 5);
    assert_eq!(term.selection_to_string().as_deref(), Some("abcdefghij"));

    parser.advance(&mut term, b"\x1b[?1049h");
    term.resize(TermSize::for_test(6, 4));
    assert_eq!(term.columns(), 6);
    assert_eq!(term.screen_lines(), 4);
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
        highlight::Lang::for_path(Path::new("a/b.py")),
        highlight::Lang::Python
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.jsx")),
        highlight::Lang::JavaScript
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.ts")),
        highlight::Lang::TypeScript
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.tsx")),
        highlight::Lang::Tsx
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.json")),
        highlight::Lang::Json
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.toml")),
        highlight::Lang::Toml
    );
    assert_eq!(
        highlight::Lang::for_path(Path::new("a/b.txt")),
        highlight::Lang::None
    );
}

#[test]
fn every_grammar_query_compiles_and_colours() {
    use highlight::{Highlighter, Lang};
    let colors = Colors::for_test();
    // A tiny snippet per grammar that must produce at least one coloured span.
    let cases = [
        (Lang::Python, "def f(x):\n    return x\n"),
        (Lang::JavaScript, "const x = 1;\n"),
        (Lang::TypeScript, "const x: number = 1;\n"),
        (Lang::Tsx, "const x = <a>hi</a>;\n"),
        (Lang::Json, "{\"a\": 1}\n"),
        (Lang::Toml, "a = 1\n"),
        (Lang::Css, "a { color: red; }\n"),
        (Lang::Html, "<p>hi</p>\n"),
        (Lang::Bash, "echo hi\n"),
        (Lang::Yaml, "a: 1\n"),
    ];
    for (lang, source) in cases {
        let mut highlighter = Highlighter::new(lang);
        highlighter.parse(source);
        let starts = highlight::line_starts(source);
        let spans = highlighter.spans_in(source, 0..source.len(), &starts, &colors);
        assert!(
            spans.values().any(|row| !row.is_empty()),
            "{lang:?} must produce at least one coloured span"
        );
    }
}

#[test]
fn reparsing_after_an_edit_never_underflows() {
    use highlight::{Highlighter, Lang};
    let colors = Colors::for_test();
    let mut highlighter = Highlighter::new(Lang::Rust);

    // Highlight one version, then an edit that shifts every later byte. Reusing
    // the stale tree without an edit record made `push_node` subtract with
    // overflow; a full reparse keeps the tree and `line_starts` in step.
    let v1 = "fn main() {\n    let x = 1;\n}\n";
    highlighter.parse(v1);
    let starts1 = highlight::line_starts(v1);
    let _ = highlighter.spans_in(v1, 0..v1.len(), &starts1, &colors);

    let v2 = "fn main_renamed() {\n    let x = 1;\n}\n";
    highlighter.parse(v2);
    let starts2 = highlight::line_starts(v2);
    let spans = highlighter.spans_in(v2, 0..v2.len(), &starts2, &colors);
    assert!(spans.values().any(|row| !row.is_empty()), "the edited buffer still highlights");
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
    assert_eq!(
        keys::encode(&key("shift-up"), TermMode::empty()),
        Some(b"\x1b[1;2A".to_vec())
    );
    assert_eq!(
        keys::encode(&key("ctrl-left"), TermMode::empty()),
        Some(b"\x1b[1;5D".to_vec())
    );
    assert_eq!(
        keys::encode(&key("f1"), TermMode::empty()),
        Some(b"\x1bOP".to_vec())
    );
    assert_eq!(
        keys::encode(&key("shift-f12"), TermMode::empty()),
        Some(b"\x1b[24;2~".to_vec())
    );
    // Printable characters belong to the input handler, not the key path.
    assert_eq!(keys::encode(&key("a"), TermMode::empty()), None);
    assert_eq!(keys::encode(&key("cmd-s"), TermMode::empty()), None);
}

#[test]
fn paste_payload_wraps_and_sanitizes() {
    use crate::terminal::paste_payload;

    // Plain paste: newlines become carriage returns so each line submits the
    // way typed input does. CRLF collapses to a single CR, not two.
    assert_eq!(paste_payload("one\ntwo", false), "one\rtwo");
    assert_eq!(paste_payload("one\r\ntwo", false), "one\rtwo");

    // Bracketed paste wraps the run so the shell reads it as inert data.
    assert_eq!(paste_payload("ls -la", true), "\x1b[200~ls -la\x1b[201~");

    // A clipboard carrying the end marker cannot break out and run commands:
    // the marker is stripped before wrapping.
    assert_eq!(
        paste_payload("x\x1b[201~rm -rf /", true),
        "\x1b[200~xrm -rf /\x1b[201~"
    );

    // Cmd-V must not also encode to a PTY byte, or paste would double-write.
    use alacritty_terminal::term::TermMode;
    use gpui::Keystroke;
    assert_eq!(
        keys::encode(&Keystroke::parse("cmd-v").unwrap(), TermMode::empty()),
        None
    );
}

#[test]
fn terminal_mouse_reports_use_sgr_coordinates_and_modifiers() {
    use alacritty_terminal::term::TermMode;
    use gpui::Modifiers;

    assert_eq!(
        crate::terminal::mouse_report(
            TermMode::SGR_MOUSE,
            4,
            2,
            0,
            false,
            false,
            Modifiers::default(),
        ),
        b"\x1b[<0;5;3M"
    );
    assert_eq!(
        crate::terminal::mouse_report(
            TermMode::SGR_MOUSE,
            4,
            2,
            0,
            true,
            false,
            Modifiers {
                control: true,
                ..Modifiers::default()
            },
        ),
        b"\x1b[<16;5;3m"
    );
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

#[test]
fn session_round_trips_through_disk() {
    use crate::services::session::{self, FileTabState, SessionState, WorkspaceState};
    let dir = std::env::temp_dir().join(format!("artifex-session-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("session.json");

    let state = SessionState::new(
        1,
        vec![WorkspaceState {
            root: "/tmp/a".into(),
            selected: Some("/tmp/a/README.md".into()),
            files: vec![FileTabState {
                path: "/tmp/a/README.md".into(),
                preview: true,
            }],
        }],
    );
    session::save_to(&state, &path);
    let loaded = session::load_from(&path).expect("state loads back");
    assert!(loaded == state);

    // A second save overwrites in place.
    let next = SessionState::new(0, Vec::new());
    session::save_to(&next, &path);
    assert!(session::load_from(&path).expect("overwrite loads") == next);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn session_load_rejects_missing_corrupt_and_future_files() {
    use crate::services::session::{self, SessionState};
    let dir = std::env::temp_dir().join(format!("artifex-session-bad-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);

    assert!(session::load_from(&dir.join("absent.json")).is_none());

    let corrupt = dir.join("corrupt.json");
    fs::write(&corrupt, "{ not json").unwrap();
    assert!(session::load_from(&corrupt).is_none());

    let future = dir.join("future.json");
    let mut state = SessionState::new(0, Vec::new());
    state.version = 99;
    fs::write(&future, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(session::load_from(&future).is_none());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn session_legacy_preferences_load_for_settings_migration() {
    use crate::services::session::{self, SidebarTabState};
    let dir = std::env::temp_dir().join(format!("artifex-session-win-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);

    let old = dir.join("old.json");
    fs::write(
        &old,
        r#"{
            "version": 1,
            "active": 0,
            "shows_sidebar": false,
            "shows_inspector": true,
            "sidebar_tab": "Git",
            "zoom": 1.4,
            "dark": true,
            "workspaces": []
        }"#,
    )
    .unwrap();
    let loaded = session::load_from(&old).expect("old file loads");
    assert!(!loaded.legacy_shows_sidebar);
    assert!(loaded.legacy_shows_inspector);
    assert!(loaded.sidebar_tab == SidebarTabState::Git);
    assert!(loaded.legacy_zoom == 1.4);
    assert!(loaded.legacy_dark == Some(true));

    let rewritten = dir.join("rewritten.json");
    session::save_to(&loaded, &rewritten);
    let text = fs::read_to_string(rewritten).expect("rewritten session exists");
    assert!(!text.contains("shows_sidebar"));
    assert!(!text.contains("shows_inspector"));
    assert!(!text.contains("\"zoom\""));
    assert!(!text.contains("\"dark\""));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn settings_round_trip_normalizes_zoom_and_rejects_future_versions() {
    use crate::services::settings::{self, SettingsState};
    let dir = std::env::temp_dir().join(format!("artifex-settings-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("settings.json");

    let mut state = SettingsState::default();
    state.shows_sidebar = false;
    state.shows_inspector = true;
    state.content_zoom = 9.0;
    state.ui_zoom = 9.0;
    state.dark = Some(true);
    state.word_wrap = true;
    settings::save_to(&state, &path);

    let loaded = settings::load_from(&path).expect("settings load back");
    assert!(!loaded.shows_sidebar);
    assert!(loaded.shows_inspector);
    assert!(loaded.content_zoom == 2.0);
    assert!(loaded.ui_zoom == 1.4);
    assert!(loaded.dark == Some(true));
    assert!(loaded.word_wrap);

    fs::write(
        &path,
        r#"{"version":1,"shows_sidebar":true,"content_zoom":1.2}"#,
    )
    .unwrap();
    let upgraded = settings::load_from(&path).expect("older settings get defaults");
    assert!(upgraded.content_zoom == 1.2);
    assert!(upgraded.ui_zoom == 1.0);

    state.version = 99;
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    assert!(settings::load_from(&path).is_none());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn find_in_lines_is_case_insensitive_and_reports_ranges() {
    use crate::services::search::find_in_lines;
    let lines: Vec<String> = vec![
        "let Foo = foo();".into(),
        "// no hit here".into(),
        "FOO".into(),
    ];
    let hits = find_in_lines(&lines, "foo");
    assert_eq!(hits.len(), 3);
    assert_eq!((hits[0].row, hits[0].start, hits[0].end), (0, 4, 7));
    assert_eq!((hits[1].row, hits[1].start), (0, 10));
    assert_eq!((hits[2].row, hits[2].start, hits[2].end), (2, 0, 3));
    assert!(find_in_lines(&lines, "").is_empty());
    assert!(find_in_lines(&lines, "absent").is_empty());
}

#[test]
fn change_tree_nests_and_compacts_directories() {
    use crate::services::git::{Change, ChangeKind, change_tree};
    let change = |path: &str| Change {
        path: path.into(),
        kind: ChangeKind::Modified,
        staged: false,
    };
    let changes = vec![
        change("src/app/shell.rs"),
        change("src/app/panels.rs"),
        change("README.md"),
        change("src/services/git.rs"),
    ];
    let rows = change_tree(&changes);
    let flat: Vec<(usize, &str, bool)> = rows
        .iter()
        .map(|r| (r.depth, r.label.as_str(), r.change.is_some()))
        .collect();
    // `src` splits into two children so it stays one row; `app` and
    // `services` have no siblings but hold files, so no further compaction.
    assert_eq!(
        flat,
        vec![
            (0, "src", false),
            (1, "app", false),
            (2, "panels.rs", true),
            (2, "shell.rs", true),
            (1, "services", false),
            (2, "git.rs", true),
            (0, "README.md", true),
        ]
    );
    // A lone deep file compacts its whole chain into one directory row.
    let rows = change_tree(&[change("a/b/c/d.rs")]);
    let flat: Vec<(usize, &str)> = rows.iter().map(|r| (r.depth, r.label.as_str())).collect();
    assert_eq!(flat, vec![(0, "a/b/c"), (1, "d.rs")]);
}

#[test]
fn parse_diff_numbers_lines_and_drops_metadata() {
    use crate::services::git::{DiffRow, parse_diff};
    let text = "diff --git a/f.rs b/f.rs\nindex 111..222 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -3,3 +3,4 @@ fn main() {\n ctx one\n-removed\n+added\n+added two\n ctx two\n\\ No newline at end of file";
    let rows = parse_diff(text, 100);
    assert_eq!(
        rows,
        vec![
            DiffRow::Hunk {
                range: "-3,3 +3,4".into(),
                context: "fn main() {".into()
            },
            DiffRow::Ctx { old: 3, new: 3, text: "ctx one".into() },
            DiffRow::Del { old: 4, text: "removed".into() },
            DiffRow::Add { new: 4, text: "added".into() },
            DiffRow::Add { new: 5, text: "added two".into() },
            DiffRow::Ctx { old: 5, new: 6, text: "ctx two".into() },
        ]
    );
    // Fabricated untracked diff: no header, numbering starts at one.
    let rows = parse_diff("+first\n+second", 100);
    assert_eq!(
        rows,
        vec![
            DiffRow::Add { new: 1, text: "first".into() },
            DiffRow::Add { new: 2, text: "second".into() },
        ]
    );
    // Limit is a hard bound.
    assert_eq!(parse_diff("+a\n+b\n+c", 2).len(), 2);
}

#[test]
fn material_icons_resolve_by_name_extension_and_default() {
    // Extension maps to the themed glyph, as an embedded resource path.
    let rust = material_icons::file_icon("main.rs", false);
    assert_eq!(rust, "material-icons/icons/rust.svg");
    // A plain extension maps to its glyph.
    assert_eq!(
        material_icons::file_icon("notes.md", false),
        "material-icons/icons/markdown.svg"
    );
    // An exact file name wins over the extension: README carries its own glyph.
    assert_eq!(
        material_icons::file_icon("README.md", false),
        "material-icons/icons/readme.svg"
    );
    // An unknown extension drops to the default file glyph.
    assert_eq!(
        material_icons::file_icon("mystery.zzzzz", false),
        "material-icons/icons/file.svg"
    );
    // Folders split on open state; an unknown name uses the appearance default.
    assert_eq!(
        material_icons::folder_icon("no-such-folder", false, false),
        "material-icons/icons/folder.svg"
    );
    assert_eq!(
        material_icons::folder_icon("no-such-folder", true, false),
        "material-icons/icons/folder-open.svg"
    );
    // A named folder resolves to its own themed glyph, not the default.
    assert_ne!(
        material_icons::folder_icon("src", false, false),
        "material-icons/icons/folder.svg"
    );
    // Every resolved path is an embedded resource the asset source can load.
    assert!(material_icons::MaterialAssets::get("icons/rust.svg").is_some());
}

#[test]
fn editor_col_and_byte_convert_across_multibyte_characters() {
    use crate::app::editor::{byte_to_col, col_to_byte};

    // "tiếng": t(1) i(1) ế(3) n(1) g(1). Byte boundaries: 0,1,2,5,6,7.
    let line = "tiếng";
    assert_eq!(col_to_byte(line, 0), 0);
    assert_eq!(col_to_byte(line, 2), 2, "caret after 'ti' sits before the 3-byte 'ế'");
    assert_eq!(col_to_byte(line, 3), 5, "caret after 'ế' skips its 3 bytes");
    assert_eq!(col_to_byte(line, 5), line.len(), "end of line");
    assert_eq!(col_to_byte(line, 99), line.len(), "past the end clamps to the end");

    assert_eq!(byte_to_col(line, 0), 0);
    assert_eq!(byte_to_col(line, 2), 2);
    assert_eq!(byte_to_col(line, 5), 3);
    assert_eq!(byte_to_col(line, line.len()), 5);
}

#[test]
fn editor_x_maps_to_the_nearest_column() {
    use crate::app::editor::x_to_col;
    use gpui::px;

    let w = px(10.);
    assert_eq!(x_to_col(px(0.), w, 8), 0);
    assert_eq!(x_to_col(px(14.), w, 8), 1, "1.4 rounds down");
    assert_eq!(x_to_col(px(16.), w, 8), 2, "1.6 rounds up");
    assert_eq!(x_to_col(px(-5.), w, 8), 0, "left of the text clamps to 0");
    assert_eq!(x_to_col(px(999.), w, 8), 8, "past the last column clamps to cols");
    assert_eq!(x_to_col(px(5.), px(0.), 8), 0, "zero width never divides by zero");
}

#[test]
fn editor_y_maps_to_the_row_under_the_pointer() {
    use crate::app::editor::y_to_row;
    use gpui::px;

    let h = px(20.);
    assert_eq!(y_to_row(px(0.), h, 5, 100), 5, "top of the first visible row");
    assert_eq!(y_to_row(px(25.), h, 5, 100), 6, "one row down");
    assert_eq!(y_to_row(px(-30.), h, 5, 100), 3, "above the first visible row");
    assert_eq!(y_to_row(px(9999.), h, 5, 100), 100, "past the last row clamps to max");
    assert_eq!(y_to_row(px(50.), px(0.), 5, 100), 5, "zero height never divides by zero");
}

#[test]
fn editor_orders_selection_endpoints_low_to_high() {
    use crate::app::editor::ordered;

    assert_eq!(ordered((2, 4), (2, 1)), ((2, 1), (2, 4)), "same row orders by byte");
    assert_eq!(ordered((1, 9), (3, 0)), ((1, 9), (3, 0)), "earlier row wins");
}

#[test]
fn editor_double_click_range_covers_the_whole_identifier() {
    use crate::app::editor::token_range;

    let line = ".selected_tab() tiếng_42";
    assert_eq!(token_range(line, 0), Some(1..13), "leading punctuation");
    assert_eq!(token_range(line, 3), Some(1..13), "inside identifier");
    assert_eq!(token_range(line, 13), Some(1..13), "boundary before punctuation");
    assert_eq!(token_range(line, 19), Some(16..26), "Unicode identifier");
    assert_eq!(token_range(line, 14), None, "whitespace selects nothing");
}
