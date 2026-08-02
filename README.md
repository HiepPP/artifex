# Rustelier

A feasibility probe: how much of Atelier can be rebuilt on Rust and GPUI, and what
it costs. The name is Rust wearing Atelier's coat. This is not a product. It
exists to answer one question with numbers.

The Swift application in `../atelier` is untouched by this project. Nothing here
writes to it. One 146 KB Swift file was copied into `fixtures/` as a scroll
benchmark input.

Read [FEASIBILITY.md](FEASIBILITY.md) for the result and the recommendation.

## Pinned Revisions

GPUI is not published on crates.io in a usable form, so it comes from the Zed
repository. The revision below is the one `gpui-component` is built and tested
against; `Cargo.lock` holds it.

| Dependency | Pin |
|---|---|
| Zed (`gpui`, `gpui_platform`, `gpui_macros`) | `1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba` |
| `gpui-component` (longbridge) | `88f102d13654fe25aa2fede076274b6b751a3704` |
| Rust toolchain | `1.97.1` (pinned in `rust-toolchain.toml`) |

`gpui` is declared without an explicit `rev` on purpose. Cargo treats
`git = "<url>"` and `git = "<url>", rev = "<sha>"` as two different sources, and
`gpui-component` declares the first form. Adding a `rev` would compile two copies
of `gpui` and every type would mismatch. The lockfile pins the commit instead:

```bash
cargo update gpui --precise 1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba
```

## Dependency Versions

| Crate | Version | Role |
|---|---|---|
| `gpui` | 0.2.2 (git) | Window, elements, layout, text, GPU renderer |
| `gpui_platform` | 0.1.0 (git) | macOS platform layer and app entry point |
| `gpui-component` | 0.5.2 (git) | Text input, resizable split, theme registry |
| `gpui-component-assets` | 0.5.1 (git) | Bundled icon and font assets |
| `gpui-wry` | 0.5.0 (git) | WebView element for GPUI |
| `lb-wry` | 0.53.3 | WebView backend (WKWebView on macOS) |
| `alacritty_terminal` | 0.26.0 | PTY, VT parser, grid |
| `tree-sitter` | 0.26.11 | Incremental parser |
| `tree-sitter-rust` | 0.24.2 | Rust grammar and highlight query |
| `tree-sitter-swift` | 0.7.3 | Swift grammar and highlight query |
| `nucleo-matcher` | 0.3.1 | Quick Open path ranking |
| `ignore` | 0.4.31 | Workspace file index walk |
| `grep-searcher` | 0.1.17 | Search All Files line scanning |
| `grep-regex` | 0.1.14 | Literal and whole-word patterns |
| `gix` | 0.86.0 | Git reads: branch, HEAD, working-tree status |
| `pulldown-cmark` | 0.13.4 | Markdown parsing |
| `rfd` | 0.17.2 | Folder picker for Add Workspace; GPUI's own prompt cannot pick the starting folder |
| `async-channel` | 2.5.0 | PTY events and search batches into GPUI tasks |
| `notify` | 8.2.0 | Recursive workspace watch; FSEvents on macOS |

The full graph is 1,030 packages. A clean debug build of all dependencies took
5 minutes 20 seconds on an M-series Mac with 10 cores.

## Build and Run

```bash
./scripts/build.sh
./scripts/test.sh
```

Both scripts put the pinned toolchain's bin directory first. A Homebrew `rustc`
earlier on `PATH` shadows the rustup toolchain even when cargo comes from rustup,
because cargo resolves its `rustc` through `PATH`; the build then fails on
`edition2024`. `rustup run` does not fix it, so the scripts set `RUSTC` too.

GPUI needs a real `.app` bundle on macOS. An unbundled binary cannot activate,
cannot own a menu bar, and cannot receive input-method events, so it cannot pass
the Vietnamese gate. `build.sh` ends by calling `scripts/bundle.sh`, which writes
`dist/Rustelier.app`; run `bundle.sh` alone to re-wrap a binary you already have.

```bash
"dist/Rustelier.app/Contents/MacOS/rustelier"          # application shell
"dist/Rustelier.app/Contents/MacOS/rustelier" gate1    # Vietnamese input
"dist/Rustelier.app/Contents/MacOS/rustelier" gate2    # embedded web preview
"dist/Rustelier.app/Contents/MacOS/rustelier" gate3    # zsh terminal
```

Running the binary from inside the bundle keeps the bundle identity and keeps
`stdout` attached, which is how the gate 1 codepoint log is captured.

## Measurement Scripts

```bash
scripts/measure_cpu.sh <pid> 60 "9 workspaces idle"
scripts/peak_cpu.sh <pid> 16 "scroll 146KB swift"
scripts/select_input_source.swift com.apple.inputmethod.VietnameseIM.VietnameseTelex
```

`measure_cpu.sh` reports both `ps %cpu` and an exact average from the CPU-time
delta. `peak_cpu.sh` reports per-second windows where 100% is one core.

## What Is Implemented

| Area | State |
|---|---|
| Workspace rail | Many live workspaces, `Cmd-1` to `Cmd-9`, `Cmd-0` opens a folder picker, changed-file badge |
| Three-pane split | Explorer or Git, center tabs, inspector; draggable, `DESIGN.md` width ranges |
| Status bar | Branch, short HEAD, changed count, layout mode, appearance, zoom |
| Explorer | Lazy tree, hard ignores, single click opens a preview tab |
| Editor | Virtualised rows, tree-sitter highlighting scoped to the visible range, `Cmd-S` |
| Terminal | `zsh` over `alacritty_terminal`, 256 and true colour, arrows, resize, IME |
| Git | Branch, status, stage, unstage, stage all, diff tabs |
| Quick Open | `Cmd-P`, fuzzy path ranking |
| Command Palette | `Cmd-Shift-P`, registered commands run the same handlers as the keys |
| Search All Files | `Cmd-Shift-F`, batched results, cancellable, bounded at 1,000 lines |
| Markdown preview | Headings, prose, lists, tables, code cards, quotes, rules |
| Filesystem watching | One FSEvents watch per root, debounced, ignore-filtered |
| Theme | Every `DESIGN.md` colour, spacing, radius and type token; light and dark |

## Filesystem Watching

Explorer, Quick Open, Search All Files and the Git panel follow the disk without
a manual refresh. `DESIGN.md` > Filesystem Freshness holds the rules; the numbers
below are what they buy, measured on the running bundle.

| Scenario | Result |
|---|---|
| Idle, watcher armed | 0.00% CPU, 63 MB RSS over 20 s |
| 3,000 files written into `target/` | 0.05% CPU, no refresh, RSS unchanged |
| 500 source files created at once | One refresh, 0.00% average CPU |
| Write into an existing file | Git snapshot only, no tree walk |

The home directory and the filesystem root are never watched. A workspace opened
there keeps the Explorer refresh control and `Workspace: Rebuild File Index` as
its only reload path.

## What Is Deliberately Absent

No AI agent panel, no MCP, no Watchtower, no Gemma sidecar, no model calls. No
persistence, no drag reorder, no editor selection or find bar, no image diffs,
no Mermaid. The POC is not aiming at feature parity.
