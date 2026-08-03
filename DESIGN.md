# Artifex Design System

## Document Status

| Field | Value |
|---|---|
| Status | Current implementation baseline |
| Updated | 2026-08-03 |
| Platform | macOS 26+ |
| UI stack | Rust, GPUI (Zed), gpui-component |
| Parent contract | `atelier/DESIGN.md`, Atelier baseline `02ebe5b` |

This file governs the Artifex build. It is adapted from the Atelier design
contract: the visual language, tokens, and interaction rules are the same, and
the platform layer is not. Where the two differ, this file records what
Artifex actually does and why.

Update this contract before implementing behavior that changes it.

## Scope

Artifex is a feasibility probe, not a product. It covers the shell and eight
surfaces at usable depth. The table states what the parent contract asks for and
what this build carries.

| Area | State here |
|---|---|
| Shell, rail, split, status bar | Ported |
| Explorer, editor, terminal, Git panel | Ported at usable depth |
| Quick Open, Command Palette, Search All Files | Ported |
| Markdown preview | Ported as a block tree, not one selectable document |
| Design tokens, light and dark, pointer cursor | Ported in full |
| Agent panel, MCP, Gemma sidecar, Watchtower | Out of scope. No language model is called |
| Runtime diagnostics probe, `atelier-doctor` | Out of scope |
| Layout profiles, display sizing tiers | Out of scope |
| Session and catalog persistence | Out of scope. Every session starts empty |
| Motion tokens | Declared below, not implemented. See Motion |

An out-of-scope area must stay absent. Do not add a partial agent surface or a
partial diagnostics writer to this build.

## Product Character

Artifex is a native macOS workspace tool. It should feel focused, dense, calm,
and expensive.

- Keep the center editor as the main visual surface.
- Use an executive-alloy hierarchy: smoked graphite navigation, titanium chrome,
  porcelain work surfaces, and one terracotta accent.
- Use compact controls and clear hierarchy instead of decorative chrome.
- Show state through fill, weight, opacity, and thin rules.
- Keep the editor matte. Reserve glass for navigation, compact chrome, and
  selected interactive surfaces only.
- Preserve text clarity at every window size.
- Prefer the simplest structure that carries the behavior. One state owner beats
  several synchronized wrappers.

## Interface Architecture

GPUI owns composition, layout, painting, and input. There is no second UI
framework: the surfaces AppKit owns in Atelier are drawn by GPUI here.

```text
main
`-- Root (gpui-component)
    `-- Shell
        |-- Title-bar drag strip
        |-- Toolbar: sidebar toggle, project menu, appearance, focus, inspector
        |-- Workspace rail
        |   |-- Workspace rows with Cmd-1 .. Cmd-9
        |   `-- Add Workspace
        |-- Three-pane split
        |   |-- Sidebar: Explorer or Git
        |   |-- Center: tab strip plus terminal, file, preview, or diff
        |   `-- Inspector: file metadata
        |-- Status bar
        `-- Quick Open, Command Palette, or Search All Files overlay
```

| Layer | Owns |
|---|---|
| `app/shell.rs` | Window chrome, workspaces, panels, actions, key bindings, status |
| `app/workspace.rs` | One workspace: root, file tree, tabs, Git snapshot, file index |
| `app/panels.rs`, `center.rs`, `overlays.rs` | Sidebar, center tabs, floating panels |
| `app/editor.rs`, `markdown.rs` | Text editing surface and Markdown preview |
| `terminal/` | PTY lifecycle, grid to runs, key encoding, input-method bridge |
| `services/` | File index, search, Git, syntax highlighting |
| `theme.rs` | Every token, the layout breakpoints, and the title-bar inset |

Rules:

- Keep one state owner per concern. Panel visibility and the selected sidebar
  tab belong to the shell, not to a workspace.
- Keep every mutation on the main thread. Long work goes to a task and reports
  back through the entity.
- Do not introduce a second theme source. Tokens come from `theme.rs` and are
  folded into the gpui-component theme once, at startup.

## Layout System

### Window and Modes

The minimum window size is `760 x 512` points. The window opens at `1440 x 900`,
centered inside the display's visible bounds, never against its full bounds.

The workspace rail stays at the outer-left edge in every mode. Focus mode hides
both side panels and never hides the rail.

| Mode | Container width | Sidebar | Inspector |
|---|---:|---|---|
| Compact | `< 900` | Hidden | Hidden |
| Standard | `900..<1280` | Visible | Hidden |
| Wide | `>= 1280` | Visible | Visible |

Rules:

- Derive the mode from the measured container width through
  `LayoutMode::for_width`. Never read it from a stored flag.
- Use one `h_resizable` split with thin dividers for sidebar, center, and
  inspector.
- Panel visibility is the product of the user toggle, the layout mode, and focus
  mode. A hidden panel is removed from the split, not collapsed to zero width.
- Keep the split container at `min_h(0)` with `overflow_hidden` so a tall panel
  cannot push the status bar off screen.
- Never mutate layout state from inside a layout-derived value. Defer it to the
  next runloop turn.

### Panel Widths

| Surface | Minimum | Ideal | Maximum |
|---|---:|---:|---:|
| Workspace rail | 176 | 176 | 176 |
| Workspace sidebar | 240 | 370 | 560 |
| Center | 420 | Flexible | Flexible |
| Inspector | 260 | 360 | 640 |

### Fixed Heights

| Token | Value | Use |
|---|---:|---|
| `PANEL_HEADER` | 40 | Panel headers and the sidebar tab strip |
| `SECTION_HEADER` | 36 | Section headers below panel chrome |
| `TAB_BAR` | 40 | Center tab strip |
| `STATUS_BAR` | 26 | Bottom workspace status |
| `FIELD` | 32 | Search and text fields |
| `CONTROL` | 28 | Regular controls |
| `COMPACT_CONTROL` | 24 | Inline controls |
| `ROW` | 28 | Dense list rows, sidebar tabs, center tabs |
| `RAIL_WIDTH` | 176 | Workspace rail |
| `RAIL_ITEM_HEIGHT` | 44 | Two-line workspace row |
| `RAIL_ITEM_GAP` | 4 | Space between workspace rows |
| `PROJECT_MENU_WIDTH` | 420 | Project command trigger |
| `PALETTE_WIDTH` | 640 | Quick Open and Command Palette |
| `PALETTE_HEIGHT` | 410 | Quick Open and Command Palette |
| `PALETTE_FIELD` | 52 | Overlay query field |
| `TITLE_BAR` | 28 | Reserve under the transparent title bar |
| `documentMaxWidth` | 640 | Markdown prose measure |
| `documentBleedMaxWidth` | 1180 | Markdown wide-block measure |
| `markdownOutlineWidth` | 200 | Trailing "On This Page" rail |

The title-bar reserve is not a constant at use sites. Read it through
`title_bar_inset(window)`: a full-screen window has no title bar, and reserving
28 points there pins every header against the menu bar.

## Color Tokens

Every token carries a light and a dark value and resolves once per appearance
change. Atelier defers text color to the native label colors; GPUI has no
equivalent, so this build pins `ink` and `ink_secondary` as the two label levels.

| Token | Light | Dark | Role |
|---|---|---|---|
| `chrome` | `#E7E3DD` | `#23262A` | Toolbar, headers, status, tab strip |
| `canvas` | `#DEDAD3` | `#181A1D` | Window and empty-state background |
| `sidebar` | `#EEEBE3` | `#202328` | Explorer, Git, and inspector bases |
| `panel` | `#F2F0EC` | `#292C30` | Cards and local panel content |
| `raised` | `#D4D0C9` | `#34383D` | Raised controls and palette body |
| `editor` | `#F8F7F4` | `#191B1E` | Editor, code, and terminal base |
| `tab_inactive` | `#E5E1DB` | `#25282C` | Inactive tabs |
| `border` | `#BFBAB2` | `#42474D` | Dividers and control outlines |
| `selection` | `#DED1C6` | `#4B3730` | Selected rows |
| `chrome_selection` | `#F7F3EE` | `#4D4742` | Warm glass selection for chrome tabs |
| `chrome_selection_ink` | `#2B2724` | `#F2EFEA` | Text and icons on selected chrome tabs |
| `hover` | `#D8D4CD` | `#383C41` | Hover state |
| `pressed` | `#CCC7BF` | `#44494F` | Pressed state |
| `accent` | `#A44F32` | `#D79570` | Primary emphasis and focus |
| `accent_ink` | `#FFF9F2` | `#21150F` | Text on accent fill |
| `workflow_done` | `#4E6C55` | `#7FA98A` | Completed workflow state |
| `workflow_todo` | `#8A652B` | `#CAA15B` | Pending workflow state |
| `workflow_blocked` | `#934941` | `#D17B72` | Blocked workflow state |
| `rail_top` | `#1D232B` | `#171C22` | Upper graphite rail gradient stop |
| `rail_bottom` | `#2D3B45` | `#202D35` | Lower petrol rail gradient stop |
| `rail_solid` | `#252D35` | `#1D252C` | Flat rail fallback |
| `rail_foreground` | `#F3F1EC` | `#F3F1EC` | Text and icons on the rail |
| `rail_secondary` | `#B6BEC3` | `#ADB7BD` | Rail metadata |
| `rail_selection` | `#4C565F` | `#46505A` | Active rail row fill |
| `rail_hover` | `#333C44` | `#2D373F` | Rail hover fill |
| `rail_pressed` | `#46515A` | `#404B54` | Rail pressed fill |
| `rail_border` | `#59636B` | `#4C575F` | Rail edge |
| `file_tree_foreground` | `#302E2B` | `#E8E4DE` | Explorer label color, including selection |
| `git_added` | `#356B43` | `#7FC58C` | Additions and success |
| `git_modified` | `#8A5B21` | `#D4A45D` | Modified state |
| `git_deleted` | `#A13E37` | `#E17B70` | Deletions and destructive state |
| `git_untracked` | `#286E68` | `#63C3B8` | Untracked state |
| `ink` | `#1E1C1A` | `#E9E5DF` | Primary label |
| `ink_secondary` | `#6A6560` | `#9A948C` | Secondary label |

Color rules:

- Reserve accent for focus, primary action, and active indicators.
- Reserve Git colors for file and diff meaning.
- Use the workflow colors only for non-Git workflow state.
- Use `chrome_selection` glass with `chrome_selection_ink` for selected sidebar
  and center tabs.
- Keep the rail dark in both appearances and use its own foreground tokens.
- Do not add a feature-local color when a semantic token fits.
- One active accent per surface.

## Spacing and Shape Tokens

Artifex uses an 8-point grid with a 4-point half step.

| Token | Value |
|---|---:|
| `XS` | 4 |
| `S` | 8 |
| `M` | 12 |
| `L` | 16 |
| `XL` | 24 |
| `XXL` | 32 |

| Token | Value | Use |
|---|---:|---|
| `Radius::PANEL` | 12 | Floating panels and icon wells |
| `Radius::CONTROL` | 8 | Fields, buttons, cards |
| `Radius::ROW` | 6 | Badges, rows, tab pills |

Depth rules:

- Use borders before shadows for structure. The gpui-component theme ships with
  shadows disabled on purpose.
- Use one `0.12` black scrim behind a blocking overlay.
- Allow one gradient on the workspace rail. Keep every other surface flat.
- A selected interactive surface may use one glass fill and one top-lit hairline.
  Nothing else gets glow, deep shadow, or a floating-card treatment.

## Typography

| Token | Size | Typical use |
|---|---:|---|
| `MICRO` | 11 | Shortcuts, metadata, compact badges |
| `CAPTION` | 12 | Secondary labels |
| `LABEL` | 12.5 | Tabs and compact actions |
| `BODY` | 13.5 | Main UI copy |
| `UI` | 14 | Fields and standard interface text |
| `HEADLINE` | 16 | Panel headers |
| `TITLE` | 17 | Section titles and strong empty states |
| `DISPLAY` | 24 | Large empty-state titles |
| `EDITOR` | 16 | Source editor and Markdown body |
| `TERMINAL` | 20 | Terminal |

Type rules:

- Use the system face for interface text.
- Use a serif face for empty-state titles and Markdown H1 and H2 only.
- Use JetBrains Mono for code, paths, shortcuts, counts, and technical metadata.
- Use semibold for hierarchy. Avoid broad bold text.
- Size every Markdown heading as a ratio of body size, never as a fixed point
  value with a floor.

Divergence: Atelier ships three independent text scales, code-ligature control,
and display sizing tiers. This build has none of them. Text sizes are fixed
tokens.

## Zoom

The status bar shows a zoom readout between 80% and 200%, changed by `Cmd-=` and
`Cmd--`.

Divergence: the readout is a value, not an applied scale. Nothing re-renders at
a different size yet. Either wire it to the render scale or drop the control; do
not leave a third state where some surfaces scale and others do not.

## Motion

| Token | Value | Use |
|---|---:|---|
| `quick` | 0.12s | Hover and press feedback |
| `standard` | 0.20s | Normal transitions |
| `deliberate` | 0.32s | Larger state changes |
| `selection` | Spring 0.28 response, 0.82 damping | Tab selection indicator |

Divergence: no animation is implemented. Selection changes are instant, and
panels appear and disappear without a transition.

When motion arrives, it must follow these rules:

- Keep tab selection inside fixed bounds. One indicator moves between measured
  frames; labels, icons, and close controls stay stationary.
- Never animate the outer geometry of a tab, a row, or a panel header.
- Never animate a loading state in a way that changes layout size.
- Add the animation without adding a second view tree for it.

## Component Rules

### Shared Components

Every shape below lives in `app/chrome.rs` so it stays identical across the rail,
the sidebar header, the tab strip, and the status bar.

| Component | Contract |
|---|---|
| `title_bar_drag_strip` | Full-width band of `title_bar_inset` height. Drags the window, and zooms it on double click |
| `icon_button` | 30-point square, `Radius::ROW`, hover and pressed fills, accent-free selected fill |
| `count_badge` | Monospaced count on a 14% wash of its own semantic tint |
| `pill_tab` | Header tab: icon, label, optional count. Equal share of the header, `ROW` height, glass fill and top-lit hairline when selected |
| `empty_state` | One icon well, a serif title, and a short message |
| `file_icon` | Identity color and glyph per extension. Never tinted by Git state |
| `project_menu` | 420-point project trigger, text only, middle-truncated |

Rules:

- Do not add a component for one local use.
- A component owns its own states. A caller must not re-implement hover or
  selection for it.
- Icons come from the gpui-component icon set. Atelier's Material icon theme is
  not ported; `file_icon` stands in for it and keeps the same rule that identity
  color never carries Git meaning.

### Interaction States

Every interactive control defines these states where relevant:

- Normal: clear fill, normal opacity.
- Hovered: `hover` fill.
- Pressed: `pressed` fill.
- Selected: one glass surface with stable geometry. No accent border, underline,
  or leading rule.
- Disabled: visible control at `0.45` opacity.

Geometry rules:

- Tabs, rows, and header controls keep identical outer bounds in every state.
  A selected tab carries a transparent border where an unselected one carries
  none, so the hairline cannot shift the layout.
- Hover, press, selection, and count changes never alter frame, padding,
  alignment, or baseline.

### Pointer Cursor

- Every enabled control that acts on click shows the pointing-hand cursor across
  its full hit target. In GPUI that is `cursor_pointer()` on the element that
  carries `on_click`.
- The cursor target and the hover fill must cover the same region. A row whose
  label is clickable but whose trailing space is not is a bug.
- Text entry keeps the I-beam. Split dividers keep the resize cursor.
- Audit every new interactive element before finishing a change.

## Surface Rules

### Workspace Rail

- Fixed 176-point rail with a graphite-to-petrol gradient and one hairline on its
  trailing edge. It is the only gradient in the application.
- Keep the `Workspaces` header, the scrollable rows, and a labelled
  `Add Workspace` action at the bottom.
- Each row is 44 points with 4-point gaps: the full project name on the first
  line, its `Cmd-1` .. `Cmd-9` shortcut in smaller monospaced secondary text on
  the second. Never show initials, monograms, or paths.
- Positions past nine stay reachable without a shortcut.
- Show the changed-file count as a trailing high-contrast badge when it is above
  zero. Count each path once across staged, unstaged, and untracked states.
- Mark the active workspace with label weight and one selection fill. No
  checkmark, no leading accent bar, no floating card.
- `Add Workspace` opens the native folder panel. It starts at `~/Projects`,
  and falls back to home when that folder does not exist.
  Choosing a folder already open selects that workspace instead of adding a
  duplicate.

Divergence: no drag reordering, no context menu, no persistence, and no cooling
of inactive sessions. Every workspace stays fully live for the run.

### Sidebar

- Use the `sidebar` token. Keep the content matte and slightly darker than the
  editor.
- Explorer and Git share one slot behind a 40-point header.
- The header holds the two tabs and nothing else. Each tab takes an equal share
  of the header width, so two tabs read as one segmented control split down the
  middle.
- Render the selected tab as a `Radius::ROW` pill of `chrome_selection` glass
  with a top-lit hairline, `chrome_selection_ink` label and icon.
- Show the Git change count as a trailing badge that never shifts the label.
- Keep tab geometry stable across normal, hover, pressed, selected, and
  count-change states.
- Explorer rows are 28 points: chevron, identity icon, label. Treat the whole
  row, including its trailing empty area, as one pointer-cursor hit target.
- Render Git-ignored entries at reduced opacity, and keep them visible.
- An Explorer click opens one replaceable preview tab. Opening another file from
  Explorer replaces that preview in place, so browsing never fills the strip.
- Quick Open and Search All Files open permanent tabs. Opening a file that is
  already showing as a preview promotes that tab instead of adding a second one.

### Center Tabs

- Titanium chrome for the strip, porcelain for the surface below it.
- Tab widths stay between 112 and 220 points.
- Render the selected tab as a `ROW`-height pill of `chrome_selection` glass with
  a top-lit hairline. No accent top rule, no selection border.
- Mark a preview tab with italic label text at `0.72` opacity. Do not add another
  icon.
- Place the close control at the leading edge of a closable tab. A tab that
  cannot close carries no close slot at all, so it has no empty gutter.
- Keep the final terminal tab open and non-closable.
- Keep editor actions in one trailing group after the scroller, with New Terminal
  always the far-right action.

### Editor

- Virtualised rows through `uniform_list`. Only the requested range is built.
- Syntax highlighting is scoped to the visible byte range. A highlight query
  reports overlapping captures, so resolve them narrowest-wins into
  non-overlapping runs before painting.
- `Cmd-S` saves. The status bar reports the result.
- The editor implements the GPUI input handler so an input method can compose
  Vietnamese, Japanese, and any other multi-stage text in place.

Divergence: no find bar, no word wrap toggle, no per-file settings, and no
selection across the document.

### Terminal

- One `zsh` per terminal tab over `alacritty_terminal`, inset by `Space::M`
  horizontally and `Space::S` vertically on the `editor` surface.
- Every terminal stays mounted for the life of its tab. Switching tabs changes
  visibility, never the view tree, so the process and its scrollback survive.
- Printable text arrives through the input handler only. The key encoder must
  never also emit it, or every character is typed twice.
- A terminal has no document, so rebuild the text before the caret from the grid
  for the input method. Without it, Vietnamese composition never starts.
- Preserve arrow keys, application cursor mode, and resize down to the PTY.

### Markdown Preview

- Open `.md` in Preview by default and keep Source one toggle away (`Cmd-D`).
- Hold prose on a 640-point measure. Let card blocks bleed to 1180: tables and
  fenced code cards read better wider, prose does not.
- Show a trailing "On This Page" outline when the document has at least two
  headings and the host is at least 900 points wide.
- Heading ratios are H1 `1.85`, H2 `1.45`, H3 `1.18`, H4 `1.00`, H5 and H6
  `0.92`. Draw the H1 and H2 rule as an accent lead segment followed by a
  hairline.
- Line-height ratios are prose `1.62`, list items `1.55`, table cells `1.45`,
  code lines `1.35`.
- Render a divider as a centered three-dot ornament, accent in the middle.
- Keep the accent budget small: the H3 eyebrow, the H1 and H2 rule lead, and the
  quote rule.

Divergence: Atelier renders one selectable native document so selection crosses
blocks. GPUI has no equivalent of `NSTextStorage`, so this build renders a tree
of block elements and selection is per block. Mermaid figures, images, callouts,
footnotes, and front-matter cards are not ported.

### Quick Open, Command Palette, and Search All Files

- One shared floating panel: 640 points wide, 410 tall, `PALETTE_FIELD` query
  field on the `editor` fill, `raised` results, `chrome` footer.
- `Cmd-P` opens Quick Open, `Cmd-Shift-P` the Command Palette, `Cmd-Shift-F`
  Search All Files.
- Show the file name first and a monospaced relative path second.
- Rank paths with a fuzzy matcher over the shared workspace file index.
- Search All Files streams ordered batches from a cancellable task, tags each
  batch with a generation, and rejects a batch from a superseded query.
- Cap one search at 1,000 matching lines and skip files above 2 MB.
- Support Up, Down, Return, and a click outside the panel to dismiss.
- Closing an overlay must return focus to the shell, or the next shortcut has no
  dispatch path.

Divergence: Escape cannot close an overlay while the query field holds focus.
This is a platform gap, not an application bug: GPUI's macOS window routes
non-printing keys to the input method before its own key bindings, and the input
method reports Escape as handled. The footer states the click-outside route
instead of promising a key that cannot arrive.

### Git

- Read through `gix`. Write through the `git` CLI. Never mix the two directions.
- Present repository identity as one compact card: workspace name, shortened
  path, current branch, short HEAD.
- Show Staged and Changes as separate, always-visible sections. Count untracked
  files under Changes, as leaf paths, never as a collapsed directory.
- Keep each change row at `ROW` height. Show Git status in the trailing slot at
  rest, then overlay the row action in that same slot on hover without reserving
  extra width.
- Show recent commits below changes with subject, author, relative time, and
  short hash.
- Push is one primary control: stage everything, commit the composer subject,
  then push the current branch. Report which stage failed.
- Open file diffs as center tabs, never inside the sidebar.
- Omit `diff --git`, `index`, `---`, and `+++` from a diff preview. Keep hunk
  headers, line numbers, context, additions, and deletions.
- Bound a diff preview at 20,000 rendered lines.
- Keep diff line numbers in a fixed 48-point gutter and the text monospaced.

Divergence: no branch picker, no discard, no upstream counts, no image diffs,
and no commit-message generation.

## Filesystem Freshness

Every surface reads the workspace as it is on disk. A file created, renamed, or
deleted outside the application appears without a manual refresh, and so does a
change to Git status.

The rules below exist because the refresh is a full tree walk, and the walk must
stay rare enough that an idle workspace costs nothing.

- Use one recursive watch per workspace root and one debounce thread for all of
  them. On macOS this resolves to FSEvents, which is kernel driven, so an idle
  workspace does no polling and holds no per-folder descriptor.
- Refuse to watch the home directory or the filesystem root. A recursive watch
  there covers caches and every other application's state, and none of it is
  about a workspace.
- Drop a path under the hard ignore list or the root `.gitignore` before it
  reaches the debouncer. A build must cost nothing.
- When the workspace root is the home directory, skip its privacy-gated
  children (`Desktop`, `Documents`, `Downloads`, `Pictures`, `Music`, `Movies`,
  `Library`) in the index walk. A Dock launch falls back to the home directory,
  and walking those folders fires one macOS permission prompt each. They stay
  reachable through the lazy tree, where descending is an explicit user action.
- Under `.git`, keep only `HEAD`, `index`, `ORIG_HEAD`, `MERGE_HEAD`, and
  `refs`. Object writes are the bulk of the traffic and no surface reads them.
- Separate the two refreshes. Only a create, a remove, or a rename can change
  the file set, so a plain write asks for a Git snapshot and no index walk.
- Coalesce a burst into one batch per root. One editor save is a write, a rename
  and a chmod, and they have to land as one refresh.
- Run one walk per workspace at a time. Fold a request that arrives mid-walk
  into the run that follows it.
- Never walk on the main thread in response to a watched event.
- Keep the Explorer refresh control and `Workspace: Rebuild File Index`. They are
  the fallback when the platform refuses a watcher, and the only path for a root
  that is not watched.

Divergence: nested `.gitignore` files are not read by the watcher's own filter.
The walk that follows honours them, so the only cost is a rebuild that finds
nothing new.

## GPUI Layout Rules

These rules come from defects found in this build. GPUI's layout is close to
CSS flexbox but not identical, and each rule below cost a visible bug.

- A scrolling box is sized by its content on the cross axis. Give the scrolling
  viewport an absolute frame inside a flex-sized box so it has a definite width
  to lay out against.
- Never put `overflow_hidden` on a card whose rows are sized by wrapped text
  inside a scrolling column. The card collapses to nothing. Losing the rounded
  clip is the cheaper trade.
- Measure a column with a zero-height flow child, not with an absolutely
  positioned one. An absolute child resolves a percentage size against the
  padding box, so it reports one padding pair too much.
- Give a text block a definite width. `w_full` plus `max_w` measures the text
  against the width the parent proposes and clamps afterwards, so a paragraph
  that wraps to two lines still reports the height of one.
- Add `min_w(0)` to every flexible text cell. The automatic minimum is the width
  of the text on one line, so without it the cell never wraps and its tail runs
  past the column.
- Cut table columns from the card's own width and let the last column absorb the
  rounding remainder, so the columns always add up to the space inside the
  border.
- `h_flex` centers its children on the cross axis. A row that should stretch
  needs `items_start` or `items_stretch` stated explicitly.
- Keep a definite width on any element whose children use percentages or flex.

## Keyboard Rules

| Shortcut | Action |
|---|---|
| `Cmd-1` .. `Cmd-9` | Select workspace by rail position |
| `Cmd-0` | Add Workspace, through the folder picker |
| `` Cmd-` `` | Next workspace, wrapping to the first |
| `Cmd-T` | New Terminal |
| `Cmd-P` | Quick Open |
| `Cmd-Shift-P` | Command Palette |
| `Cmd-Shift-F` | Search All Files |
| `Cmd-E` | Toggle between the Explorer and Git sidebar tabs |
| `Cmd-D` | Toggle Source and Preview for the active Markdown file |
| `Cmd-S` | Save the active file |
| `Cmd-Q` | Close the active closable center tab |
| `Cmd-Shift-R` | Toggle the sidebar |
| `Cmd-Shift-T` | Toggle the inspector |
| `Cmd-Shift-E` | Toggle focus mode |
| `Cmd-=` | Zoom in, readout only |
| `Cmd--` | Zoom out, readout only |

Rules:

- A palette command and its shortcut call the same action. Do not duplicate the
  behavior at the call site.
- Register the shell's bindings after the component kit's. When two bindings
  match at the same context depth, the later registration wins, and the kit
  claims `escape` for its own query field.
- Do not reuse a shortcut for a second action.
- Keep shortcut labels monospaced in the Command Palette.

## Accessibility Rules

- Give every icon-only control a label.
- Never convey state by color alone. Git status carries a letter, ignored files
  carry help text, and a selected tab carries a fill plus ink change.
- Keep keyboard focus stable after an overlay closes.
- Keep contrast stable on the rail by using its own foreground tokens.

Divergence: GPUI exposes no accessibility tree on macOS in this revision, so a
screen reader sees nothing. Reduce Motion, Reduce Transparency, and Increase
Contrast are not observed. Record this as a gap rather than hiding it behind a
partial implementation.

## Implementation Rules

- Use an existing token before adding a literal.
- Add a shared token only in `theme.rs`.
- Keep GPUI element trees shallow. Prefer one element with more style over two
  nested wrappers.
- Ban `unwrap`, `expect`, `panic!`, and slice indexing on any view or controller
  path. Use `let ... else`, `if let`, and `get`.
- Keep every filesystem walk behind the shared ignore rules and the 2 MB text
  limit.
- Cancel background work through an explicit token and tag streamed results with
  a generation. A cancelled task may still deliver one more batch.
- Do not allocate colors, fonts, or images inside a paint or measure closure.
- Do not add a dependency for a shape a token already carries. `rfd` is present
  because GPUI's own path prompt cannot choose the folder it opens in.

## Verification Rules

Every UI change must pass these before it is called done.

```bash
./scripts/build.sh
./scripts/test.sh
```

Use the scripts, not bare cargo. A Homebrew `rustc` earlier on `PATH` shadows
the pinned toolchain and the build fails on `edition2024`; `rustup run` does not
fix it, because cargo resolves its `rustc` through `PATH`.

GPUI needs a real `.app` bundle on macOS to activate, own a menu bar, and
receive input-method events. An unbundled binary cannot verify text input.

Native checks:

- Launch the bundle and drive the changed surface. A clean build proves nothing
  about layout.
- Check light and dark when colors change.
- Check the narrow and wide layout when a breakpoint or a panel rule changes.
- Type Vietnamese through Telex in the editor and in the terminal when the input
  path changes.
- Switch between a terminal tab and an editor tab ten times when tab mounting
  changes. The terminal process must survive.
- Hover every new clickable control and confirm the pointing hand covers its full
  hit target.
- Create, rename and delete a file outside the application when the watcher or a
  refresh path changes. The Explorer must follow without a manual refresh, and a
  build inside the workspace must not trigger one.
- Confirm the process reports no panic in its output.

## Source of Truth

| Area | Source |
|---|---|
| Product and pinned revisions | [README.md](README.md) |
| Feasibility result and defects | [FEASIBILITY.md](FEASIBILITY.md) |
| Window, bundle, and modes | [src/main.rs](src/main.rs) |
| Colors, metrics, typography, breakpoints | [src/theme.rs](src/theme.rs) |
| Shared chrome components | [src/app/chrome.rs](src/app/chrome.rs) |
| Shell, rail, split, status bar, actions | [src/app/shell.rs](src/app/shell.rs) |
| Sidebar and inspector | [src/app/panels.rs](src/app/panels.rs) |
| Center tabs | [src/app/center.rs](src/app/center.rs) |
| Overlays | [src/app/overlays.rs](src/app/overlays.rs) |
| Editor | [src/app/editor.rs](src/app/editor.rs) |
| Markdown preview | [src/app/markdown.rs](src/app/markdown.rs) |
| Terminal | [src/terminal/mod.rs](src/terminal/mod.rs) |
| Git | [src/services/git.rs](src/services/git.rs) |
| Filesystem watching | [src/services/watch.rs](src/services/watch.rs) |
| Parent design contract | `atelier/DESIGN.md` |

Update this document when a shared token, breakpoint, component contract, or
design rule changes.
