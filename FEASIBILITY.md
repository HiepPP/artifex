# Rustelier: Atelier on Rust and GPUI

Measured on macOS 26.5.1, Apple silicon, 10 cores, 24 GB. Rust 1.97.1.
Every number below came from this POC on this machine. The Swift figures are the
baselines supplied with the brief, measured the same way (`ps -o %cpu=`).

## Blocking Gates

| Gate | Verdict | Evidence |
|---|---|---|
| 1. Vietnamese input | **Go** | Telex composes correctly in a GPUI multi-line field. `tieengs` produced `U+0074 U+0069 U+1EBF U+006E U+0067` - pre-composed NFC, no combining marks. Tone after a multi-byte character (`viê` + `j` -> `việ`, `U+1EC7`) works. `Cmd-Z` during composition removes the in-flight run cleanly. Paste of 115 characters / 155 bytes arrives intact. Also works inside the terminal after adding a shadow document. |
| 2. Web preview | **Go, with a limitation** | A `wry` WebView renders a local HTML page in a tab, scrolls, follows a window resize (`innerWidth` 1406 -> 856), and disappears when another tab is selected. It does **not** respect the container's rounded clip: the host card is drawn with a 24 pt radius and the web content keeps square corners. |
| 3. Terminal | **Go, with limitations** | `zsh` runs under `alacritty_terminal`. 256-colour and true-colour output render, `ls -G` colours, arrow-key history and `less` navigation work, resize reaches the PTY (`tput cols` 67 / `lines` 16 after shrinking), and a running loop keeps ticking across a tab switch. Vietnamese composes and commits. The composition preview is not drawn at the caret, and there is no selection or copy. |

Gate 1 held, so the probe continued.

### Gate 1 caveat: VNI was not testable

Only Telex was an enabled input source on this machine. Enabling the macOS VNI
mode programmatically selected it, but in that state **no** composition happened
at all - not VNI rules, and not Telex rules either. Switching back to Telex made
composition work again in the same field, in the same build. The block is on the
macOS input-method side of a mode enabled mid-session, not in GPUI. VNI stays
unverified; the composition path it would use is the same `marked text` path
Telex already exercised.

## Code Written

Raw line counts, including comments and blank lines.

| Area | Lines | What it is |
|---|---:|---|
| `src/services` | 893 | File tree, file index, search, git, tree-sitter highlighting |
| `src/app/shell.rs` | 710 | Toolbar, rail, split, status bar, actions |
| `src/terminal` | 758 | PTY lifecycle, grid to elements, ANSI colours, key encoding, IME |
| `src/gates` | 667 | The three Phase 0 harnesses |
| `src/app/panels.rs` | 644 | Explorer, Git panel, inspector |
| `src/app/overlays.rs` | 595 | Quick Open, Command Palette, Search All Files |
| `src/app/markdown.rs` | 564 | Markdown parse, block rendering, outline rail |
| `src/tests.rs` | 431 | Deterministic non-UI tests |
| `src/app/editor.rs` | 418 | Buffer, cursor, virtualised rows, viewport highlighting, IME |
| `src/app/center.rs` | 344 | Tab strip, tab routing, diff view |
| `src/theme.rs` | 299 | Every `DESIGN.md` token, light and dark |
| `src/app/workspace.rs` | 259 | Live workspace: tree, git, index, tabs |
| `src/app/chrome.rs` | 170 | Shared chrome: icon button, pill tab, badge, drag strip |
| `src/main.rs` | 102 | Entry point, window placement, mode dispatch |
| **Total** | **6,862** | |

For scale, the Swift application is 40,530 source lines across 110 files, plus
14,020 test lines. The POC covers the shell and eight surfaces at "usable, not
complete" depth in about 17% of that.

## Time Spent

These are agent wall-clock times on one machine, with the dependency build
running in the background while code was written. They are not an estimate of
human engineering effort, and should not be read as one.

| Phase | Wall clock | Notes |
|---|---:|---|
| Setup: toolchain, pinning, dependency graph | 14 min | Includes a 5 min 20 s clean build of 1,030 packages |
| Phase 0: three gates, written and driven | 46 min | Includes two real defects found and fixed |
| Phase 1: rail, split, status bar | 12 min | Overlapped with Phase 2 |
| Phase 2: explorer, editor, terminal tabs, git, palettes, search, markdown | 35 min | |
| Phase 3: tokens, light and dark, pointer cursors | 8 min | Applied while writing each surface |
| Verification: tests, CPU, stability | 20 min | |
| Chrome pass against the shipped app | 95 min | Toolbar, icons, outline rail; most of it spent on one GPUI layout trap |
| **Total** | **~3 h 50 min** | |

## CPU and Memory

`ps %cpu` is the number the Swift baseline used. `cputime` is the exact average
from the CPU-time delta over the same window. `peak` counts 100% as one core.

| Scenario | Swift baseline | POC | Reading |
|---|---:|---:|---|
| 9 workspaces, idle 60 s | 0.33% | 0.50% (`ps`), 0.67% (cputime), 90 MB RSS | POC costs about 1.5x to 2x more at idle |
| 150 writes into a background workspace | not supplied | 0.43% (`ps`), 0.59% (cputime) | **Not a win.** The POC has no filesystem watcher, so a write costs nothing. The Swift number includes real watcher work |
| Scrolling a 146 KB Swift file | 0.35 core peak | 0.17 core peak | POC costs about half, but see below |

The idle sample is almost entirely `kevent` and `mach_msg` waits - nine PTY
reader threads parked plus the GPUI run loop. There is no spin loop. The extra
cost over Swift is structural, not a bug.

The scroll comparison is not like for like. The POC editor draws styled lines
with a gutter and nothing else: no soft wrap, no selection, no find bar, no
accessibility tree, no ligature shaping decisions. `NSTextView` does all of that.
Viewport-scoped highlighting is real and works - the tree-sitter query runs with
`set_byte_range` over the rows `uniform_list` asked for, and a test asserts no
row outside that window is highlighted - but half the measured saving is bought
by doing less, not by doing it faster.

## Stability

| Check | Result |
|---|---|
| `cargo build` | Clean, 0 errors |
| `cargo test` | 19 passed in 0.75 s |
| Zoom and unzoom, 4 cycles, crossing the wide/standard breakpoint | No crash, layout mode follows the width |
| Terminal to editor tab switch, 10 round trips | No crash, shell process and scrollback intact |
| Crash reports in `~/Library/Logs/DiagnosticReports` | None |
| Panics | None in any run |

## Defects Found and Fixed During the Probe

| Defect | Cause |
|---|---|
| Every terminal character typed twice (`ls` became `alas`) | The key handler encoded `key_char` while macOS also delivered the same text through the input handler. Printable text must come from one path only |
| Vietnamese never composed in the terminal | An input method needs the text before the caret. A terminal has no document, so one had to be rebuilt from the grid on every query |
| `Cmd-Shift-F` silently dead after any overlay closed | Closing an overlay dropped the entity that owned focus, so key bindings had no dispatch path back to the shell |
| Stale search results appearing after the query changed | Cancellation only takes effect between files, so the worker still delivered one more batch. Now tagged with a generation |
| A click on the first tab closed the window | The tab strip sat under the transparent title bar, directly beneath a traffic light |
| Identifiers rendered two-tone | A highlight query reports several captures over the same text. They were applied in match order, so a wide `variable` and a narrow `type` fought over the same bytes. Resolved narrowest-wins into non-overlapping runs |
| Every header pinned to the top edge in full screen | The title-bar reserve was a constant. A full-screen window has no title bar, so the reserve now comes from `Window::is_fullscreen` |
| The window could open partly off screen | `WindowBounds::centered` centres against the display's full bounds and never clamps the size. Now centred inside `visible_bounds` with a margin |
| Markdown tables and code cards vanished in long documents | `overflow_hidden` on a card whose rows are sized by wrapped text collapses it to nothing inside a scrolling column. The clip is gone; the rounded corners no longer clip, the content stays |
| The document column grew to its widest table instead of wrapping | A scrolling box is sized by its content on the cross axis. The viewport now sits in an absolute frame inside a flex-sized box, which hands it a definite width |
| Table cells ran past the right edge of their card | The column measure came from a canvas that was absolutely positioned and `size_full`. An absolute child resolves a percentage size against the padding box, so the reported width was one padding pair too wide and every definite-width cell overflowed. The canvas is now a zero-height flow child, and the card takes the same definite width the columns are cut from |
| Prose drew on top of the next block, and a long list item was cut off at the column edge | Every text block was `w_full` plus `max_w`. Text is measured against the width the parent proposes, and the maximum is applied after that, so a paragraph measured at the full column reported one line of height while painting two. Blocks now take a definite width, and each flexible text cell carries `min_w(0)` so it can shrink below one line and wrap |
| Untracked work showed as `src` and `target`, not as files | `gix`'s dirwalk collapses an untracked directory into one entry by default. Set to `UntrackedFiles::Files`, and empty directories are dropped because nothing in them can be staged |

## Defects Still Open

- Unstage fails on a repository with no `HEAD`. The error surfaces correctly in
  the status bar, which is the right behaviour for the wrong reason.
- Escape cannot close an overlay while the query field holds focus. This is not
  an Atelier bug and there is no fix in application code; see the platform note
  below. The scrim click that `DESIGN.md` specifies does close it, and that is
  what the panel footer now says.
- Quick Open keeps the previous workspace's results until the next keystroke
  after a workspace switch.
- The file index goes stale in a workspace that is not watched, which is the home
  directory and the filesystem root. `Workspace: Rebuild File Index` is the fix.
  Every other workspace now follows the disk; see README > Filesystem Watching.
- Long unbroken tokens, such as a commit hash, overflow their table cell rather
  than breaking by character.
- Markdown preview is a tree of block elements, so selection cannot cross blocks.

## What GPUI Does Not Provide

Everything in this list had to be written for the POC, and would have to be
written, maintained and hardened for a real port.

| Missing | Cost in this POC | Cost in a real port |
|---|---|---|
| Terminal | 754 lines | Much larger: selection, copy, search, links, shell integration, reflow |
| Code editor | 427 lines | Very large: `NSTextView` replacement including wrap, selection, undo, find and replace, multi-cursor |
| Marked-text plumbing for non-text surfaces | Shadow document per surface | Every surface that accepts typing needs one |
| Syntax highlighting integration | 200 lines | Injections, precedence, incremental re-parse on edit |
| Lazy file tree with disclosure | 170 lines | Plus watching, icons, drag, rename, context menus |
| Git model | 230 lines | Plus trees, discard, branches, history, image diffs |
| Markdown renderer | 356 lines | Large: `DESIGN.md` specifies one selectable native document, which GPUI cannot express at all |
| Fuzzy finder, palette, project search UI | 580 lines | Plus ranking quality and keyboard model |
| Transparent title bar inset | Manual constant plus a full-screen check | Must be handled on every surface |
| Window dragging and double-click zoom | Hand-written once `app_owns_titlebar_drag` is on | `WindowControlArea` is a no-op on macOS, so every title-bar behaviour is the caller's |
| A deliverable Escape while a text field has focus | Not solvable in app code | macOS hands every non-printing key to the input method before GPUI's keymap. When the input method reports it handled the key, `gpui_macos` returns early and the key never reaches an action, a binding or a `on_key_down` listener. Verified with an ABC layout and with Vietnamese Telex, with a global binding, a context binding, a descendant-context binding, a listener for the component kit's own `Escape` action, and a raw key logger: none of the six fired |
| Predictable sizing inside a scroll container | Absolute frame plus a measured column width | Percentage and flex children resolve against nothing in a scrolling box; three separate blocks disappeared before this was understood |
| Rounded clipping over a native child view | Not possible | The WebView ignores the clip |
| Text selection across blocks | Not possible today | No `NSTextStorage` equivalent |
| Filesystem watching | 321 lines over `notify`: debounce, ignore filter, per-root attribution, scan coalescing | Plus per-folder invalidation instead of a whole-tree walk, watch limits, network volumes, case-insensitive rename pairing |
| Persistence and restore | Not implemented | All of it |
| Accessibility | Partly present, and noisy: the run log fills with `ERROR: getApplicationProperty: called with invalid property` | Must be audited surface by surface |

## Unstable Or Unversioned APIs Used

| API | Risk |
|---|---|
| `gpui` from a Zed git revision | Not published in usable form on crates.io. No semver. Breaking changes land on the default branch continuously |
| `gpui-component` pinned by revision, but its own `gpui` dependency floats to the default branch | Regenerating `Cargo.lock` moves both silently. This POC pins the commit by hand |
| `EntityInputHandler` plus `ElementInputHandler` inside a `canvas` paint closure | The pattern is undocumented and asserts that it runs during paint |
| `TextSystem::resolve_font` and `ch_advance` | `font_id` is private; measuring one monospace cell depends on whichever accessor is public in that revision |
| `Pixels` field access | `Pixels.0` is private now. Arithmetic must go through `Mul<f32>` and `From<Pixels> for f32` |
| `gpui-wry` on `lb-wry` | A fork of `wry`, versioned separately from upstream |
| `alacritty_terminal` 0.26 | Pre-1.0. `EventLoop`, `Notifier` and `Msg::Resize` are effectively internal API |
| `gix` 0.86 status surface | Pre-1.0. `gix::status::Item`, `index_worktree::Item` and `EntryStatus` variants changed shape during this build |
| `tree-sitter` 0.26 `StreamingIterator` query API | Changed in the 0.25 to 0.26 window; grammar crates must match the ABI |

Two of the nine rows above are the whole UI framework and its component kit.

## Recommendation

**Not yet.** Do not start a rewrite now. Revisit when the numbers below change.

The reasons are measurements, not taste.

1. **There is no idle-performance case.** The stated goal for Atelier is a calm
   idle process. With the same nine workspaces the POC costs 0.50% against the
   Swift build's 0.33%, and 0.67% by exact CPU time. A rewrite that costs 1.5x
   to 2x more at idle is not paying for itself.

2. **The one win is not a like-for-like win.** Scrolling the 146 KB Swift file
   peaked at 0.17 core against 0.35. Half of that gap is viewport-scoped
   highlighting, which is a real technique. The other half is an editor with no
   wrap, no selection, no find bar and no accessibility tree. Add those and the
   gap narrows; nobody can say by how much without building them.

3. **The work is dominated by what GPUI does not have.** 6,862 lines reached
   "usable, not complete" across eight surfaces. Of those, 1,181 lines are an
   editor and a terminal that AppKit supplies for free, and both are the shallow
   versions. Matching the shipped chrome cost another 95 minutes, and most of
   that went on one layout trap rather than on design. `DESIGN.md` asks for one selectable native Markdown document; GPUI
   has no primitive that can express it. That requirement would have to be
   dropped or a text engine written.

4. **The dependency base is not something to build a product on yet.** The UI
   framework has no release, no semver and no compatibility promise, and the
   component kit tracks the framework's default branch. Two of the four
   Phase 0 defects came from undocumented framework behaviour, not from Atelier
   logic. That is a permanent tax while the pin moves.

5. **What did hold up is worth recording.** Vietnamese input, the hardest gate,
   passed on the first serious attempt in the text field, and passed in the
   terminal after a shadow document was added. The terminal, the web preview, the
   split, the palettes and project search all reached usable state quickly. GPUI
   is not the problem. The problem is that Atelier's value is concentrated in the
   two surfaces GPUI leaves entirely to the caller.

### What would change the verdict

- GPUI ships a versioned crates.io release with a compatibility policy.
- An editor element exists that covers wrap, selection, find and accessibility,
  whether from Zed or from `gpui-component`.
- A measured idle profile at or below the Swift build with nine live workspaces,
  after a filesystem watcher and persistence are added, because those are the
  parts that actually cost idle CPU.
- A decision that Markdown preview does not need one selectable document.

Until then the honest move is to keep this POC as a reference, keep shipping the
Swift application, and re-measure when the first two bullets land.
