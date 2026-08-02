# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Rustelier is a feasibility probe, not a product. It answers one question with
numbers: how much of the Swift application in `../atelier` can be rebuilt on
Rust + GPUI, and what it costs. `FEASIBILITY.md` holds the verdict ("not yet")
and the measured defect list.

Scope is fixed by `DESIGN.md` > Scope. An out-of-scope area must stay absent.
Do not add a partial agent panel, MCP surface, diagnostics writer, or
persistence layer. `../atelier` is read-only from here; nothing in this repo
writes to it.

## Commands

```bash
./scripts/build.sh              # build + icon + bundle; use this, not bare cargo build
./scripts/build.sh release
cargo test                      # non-UI tests only, no display needed
cargo test file_tree_expands_lazily     # single test by name
cargo clippy --all-targets
cargo fmt
./scripts/bundle.sh debug       # re-wrap an existing binary into dist/Rustelier.app
```

`scripts/build.sh` exists because a Homebrew `rustc` earlier on `PATH` shadows
the rustup toolchain, and the build then fails on `edition2024`. It prepends the
pinned toolchain's bin directory. Bare `cargo build` works only when `PATH` is
already clean.

### Running

GPUI needs a real `.app` bundle on macOS. An unbundled binary cannot activate,
cannot own a menu bar, and cannot receive input-method events. Run from inside
the bundle so bundle identity holds and `stdout` stays attached:

```bash
"dist/Rustelier.app/Contents/MacOS/rustelier"          # application shell
"dist/Rustelier.app/Contents/MacOS/rustelier" gate1    # Vietnamese IME probe
"dist/Rustelier.app/Contents/MacOS/rustelier" gate2    # embedded web preview
"dist/Rustelier.app/Contents/MacOS/rustelier" gate3    # zsh terminal
```

Optional second argument to the shell mode is the workspace root. Without it,
`resolve_root` falls back through `current_dir` then `$HOME`, because
LaunchServices sets the working directory to `/`.

### Measurement

```bash
scripts/measure_cpu.sh <pid> 60 "9 workspaces idle"
scripts/peak_cpu.sh <pid> 16 "scroll 146KB swift"
scripts/select_input_source.swift com.apple.inputmethod.VietnameseIM.VietnameseTelex
```

`fixtures/Large146KB.swift` is the scroll benchmark input.

## Architecture

One binary, two entry shapes, dispatched on `argv[1]` in `src/main.rs`: the
Phase 0 gates (`src/gates/`) and the application shell (`src/app/shell.rs`).

### Ownership Chain

`Shell` -> `Vec<Workspace>` -> `Vec<Tab>` -> `TabKind`.

- `Shell` ([src/app/shell.rs](src/app/shell.rs)) owns window state: panel
  visibility, sidebar tab, focus mode, zoom, appearance, overlay state, and the
  resizable split. Every action handler lives here, and every keybinding maps to
  one of them.
- `Workspace` ([src/app/workspace.rs](src/app/workspace.rs)) owns one root: file
  tree, git snapshot, file index, tabs. Unselected workspaces stay alive - their
  editors and shell processes keep running. Only the selected one renders.
- `TabKind` is `Terminal | File | Diff`. A `File` tab carries an `EditorView`
  and, for Markdown, a pre-parsed `MarkdownView`, so the document is parsed once
  per open rather than once per frame.

`Shell::scan_workspace` builds the file index and git snapshot on a background
task. Both walk the whole tree; run inline they block the `open_window`
callback and the window is never created.

### Layers

| Layer | Files | Role |
|---|---|---|
| Shell/chrome | `app/shell.rs`, `app/chrome.rs` | Rail, toolbar, status bar, actions, keybindings |
| Surfaces | `app/panels.rs`, `app/center.rs`, `app/overlays.rs` | Sidebar/inspector, tab strip, Quick Open + Palette + Search |
| Documents | `app/editor.rs`, `app/markdown.rs` | Hand-written editor, Markdown block tree |
| Terminal | `terminal/{mod,keys,colors}.rs` | PTY, grid-to-element, key encoding, ANSI colors |
| Services | `services/*.rs` | Pure logic: file index, lazy tree, git, highlight, search |
| Tokens | `theme.rs` | Every `DESIGN.md` color, metric, and type token |

`services/` and `theme.rs` are window-free, which is what makes
[src/tests.rs](src/tests.rs) runnable without a display. Put testable logic
there, not in a view.

### Why So Much Is Hand-Written

GPUI ships no terminal, no code editor, no Markdown renderer, no file tree, no
fuzzy finder. Each is POC code here. Before assuming a widget exists, check
`FEASIBILITY.md` > What GPUI Does Not Provide.

Two hand-rolled patterns worth knowing:

- Highlighting ([services/highlight.rs](src/services/highlight.rs)) parses the
  whole file once, then runs queries with an explicit byte range covering only
  the visible rows. A 146 KB file must cost the same as a 2 KB file while
  scrolling. Overlapping captures resolve to the narrowest span.
- Terminal and editor both plumb IME through `EntityInputHandler` inside a
  `canvas` paint closure. That pattern is undocumented GPUI API.

## Rules That Bite

### DESIGN.md Is The Contract

[DESIGN.md](DESIGN.md) governs this build, adapted from `atelier/DESIGN.md`.
Update the contract before implementing behavior that changes it. Its
`Source of Truth` table maps each area to its owning file.

- Use an existing token before adding a literal. Shared tokens go in
  [src/theme.rs](src/theme.rs) only.
- Ban `unwrap`, `expect`, `panic!`, and slice indexing on any view or controller
  path. Use `let ... else`, `if let`, `get`.
- Keep every filesystem walk behind the shared ignore rules and the 2 MB text
  limit.
- Cancel background work through an explicit token and tag streamed results with
  a generation. A cancelled task may still deliver one more batch.
- Do not allocate colors, fonts, or images inside a paint or measure closure.
- Keep element trees shallow. One element with more style beats two wrappers.

### GPUI Layout Is Not Flexbox

`DESIGN.md` > GPUI Layout Rules lists eight rules, each of which cost a visible
bug. The ones that recur:

- A scrolling box is sized by its content on the cross axis. Give the viewport
  an absolute frame inside a flex-sized box.
- Give a text block a definite width. `w_full` plus `max_w` measures against the
  proposed width and clamps after, so a two-line paragraph reports one line.
- Add `min_w(0)` to every flexible text cell, or it never wraps.
- Never put `overflow_hidden` on a card whose rows are sized by wrapped text
  inside a scrolling column. The card collapses to nothing.

### Dependency Pinning

`gpui` is declared **without** an explicit `rev` on purpose. Cargo treats
`git = "<url>"` and `git = "<url>", rev = "<sha>"` as different sources, and
`gpui-component` declares the first form. Adding a `rev` compiles two copies of
`gpui` and every type mismatches. `Cargo.lock` pins the commit instead:

```bash
cargo update gpui --precise 1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba
```

Regenerating `Cargo.lock` from scratch silently moves both `gpui` and
`gpui-component`'s floating `gpui` to the Zed default branch. Toolchain is
pinned to `1.97.1` in `rust-toolchain.toml`.

### Keys And Escape

`bind_keys` is called **last** during window construction. When two bindings
match at the same context depth the later registration wins, and the component
kit registers its own `escape` for its query field while the root is built.

A plain `escape` never arrives while a text field holds focus: macOS hands
non-printing keys to the input method first, and GPUI drops the key when the IME
reports it handled. `cmd-escape` is bound as a workaround, and `Shell` also
listens for the kit's own `input::Escape` action. This is a platform limit, not
an application bug.

## Verification

A clean build proves nothing about layout. `DESIGN.md` > Verification Rules is
the full checklist; the minimum for any UI change:

```bash
./scripts/build.sh
cargo test
```

Then launch the bundle and drive the changed surface. Check light and dark when
colors change. Check narrow and wide when a breakpoint or panel rule changes.
Type Vietnamese through Telex in the editor and terminal when the input path
changes. Confirm no panic in the process output.
