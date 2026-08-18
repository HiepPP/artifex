# Artifex Design System

## Document Status

| Field | Value |
|---|---|
| Status | Current implementation baseline |
| Updated | 2026-08-16 |
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
| Reading Room shell, rail, split, status bar | Ported |
| Explorer, editor, terminal, Git panel | Ported at usable depth |
| Quick Open, Command Palette, Search All Files | Ported |
| Markdown preview | Ported as a block tree, not one selectable document |
| Design tokens, light and dark, pointer cursor | Ported in full |
| Agent panel, MCP, Gemma sidecar, Watchtower | Out of scope. No language model is called |
| Runtime diagnostics probe, `atelier-doctor` | Out of scope |
| Layout profiles, display sizing tiers | Out of scope |
| Session persistence | In scope: open workspaces and file tabs. See Session Persistence |
| Catalog persistence | Out of scope |
| Motion tokens | Declared below, not implemented. See Motion |

An out-of-scope area must stay absent. Do not add a partial agent surface or a
partial diagnostics writer to this build.

## Product Character

Artifex is a native macOS repository reading room. It should feel focused,
dense, calm, and expensive.

- Keep the selected file reader or preview as the main visual surface. Editing
  remains available, but it must not dominate the shell.
- Optimize the first screen for repository orientation: choose a workspace,
  browse files, search, inspect changes, read content, and verify provenance.
- Use an executive-alloy hierarchy: smoked graphite navigation and global
  toolbar, titanium local chrome, porcelain work surfaces, and one terracotta
  accent.
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
        |-- Toolbar: sidebar toggle, global search, appearance, focus, context
        |-- Workspace rail
        |   |-- Active workspace identity and branch
        |   |-- Files, Search, Changes primary navigation
        |   |-- Workspace rows with Cmd-1 .. Cmd-9
        |   `-- Add Workspace
        |-- Three-pane split
        |   |-- Navigator: Explorer or Git changes
        |   |-- Reader: breadcrumb, file controls, terminal, file, preview, or diff
        |   `-- Context rail: outline, links, Git provenance, metadata
        |-- Status bar
        `-- Quick Open, Command Palette, or Search All Files overlay
```

| Layer | Owns |
|---|---|
| `app/shell.rs` | Window chrome, workspaces, panels, actions, key bindings, status |
| `app/workspace.rs` | One workspace: root, file tree, tabs, Git snapshot, file index |
| `app/panels.rs`, `center.rs`, `overlays.rs` | Sidebar, center tabs, floating panels |
| `app/editor.rs`, `diff.rs`, `markdown.rs` | Text editing surface, diff view, Markdown preview |
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

| Mode | Container width | Navigator | Context rail |
|---|---:|---|---|
| Compact | `< 900` | Hidden; opens as an overlay | Hidden |
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
| Workspace rail | 230 | 230 | 230 |
| Workspace navigator | 288 | 288 | 420 |
| Center | 420 | Flexible | Flexible |
| Context rail | 280 | 300 | 420 |

### Fixed Heights

| Token | Value | Use |
|---|---:|---|
| `TOP_CHROME` | 52 | Title-bar reserve plus global search chrome |
| `PANEL_HEADER` | 48 | Navigator surface title |
| `READER_LOCATOR` | 36 | Compact workspace and parent-location breadcrumb |
| `SECTION_HEADER` | 40 | Branch and compact section headers below panel chrome |
| `TAB_BAR` | 44 | Reader document tabs and actions |
| `STATUS_BAR` | 28 | Bottom workspace status |
| `FIELD` | 32 | Search and text fields |
| `CONTROL` | 28 | Regular controls |
| `COMPACT_CONTROL` | 24 | Inline controls |
| `ROW` | 28 | Dense Git rows and center tabs |
| `TREE_ROW` | 32 | Explorer file and folder rows |
| `RAIL_WIDTH` | 230 | Workspace rail |
| `RAIL_ITEM_HEIGHT` | 44 | Two-line workspace row |
| `RAIL_ITEM_GAP` | 4 | Space between workspace rows |
| `PROJECT_MENU_WIDTH` | 568 | Global search trigger |
| `PALETTE_WIDTH` | 640 | Quick Open and Command Palette |
| `PALETTE_HEIGHT` | 410 | Quick Open and Command Palette |
| `PALETTE_FIELD` | 52 | Overlay query field |
| `TITLE_BAR` | 28 | Reserve under the transparent title bar |
| `documentMaxWidth` | 720 | Markdown prose measure |
| `documentBleedMaxWidth` | 880 | Markdown wide-block measure |
| `CONTEXT_WIDTH` | 300 | Wide-layout context rail |

The title-bar reserve is not a constant at use sites. Read it through
`title_bar_inset(window)`: a full-screen window has no title bar, and reserving
28 points there pins every header against the menu bar.

## Color Tokens

Every token carries a light and a dark value and resolves once per appearance
change. Atelier defers text color to the native label colors; GPUI has no
equivalent, so this build pins `ink` and `ink_secondary` as the two label levels.

| Token | Light | Dark | Role |
|---|---|---|---|
| `toolbar` | `#292F37` | `#20252C` | Global Reading Room toolbar in both appearances |
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
| `EDITOR` | 16 | Every content surface: editor, diff, terminal, Markdown body |

Type rules:

- Use the system face for interface text.
- Use a serif face for empty-state titles and Markdown H1 and H2 only.
- Use JetBrains Mono for code, paths, shortcuts, counts, and technical metadata.
- Use semibold for hierarchy. Avoid broad bold text.
- Size every Markdown heading as a ratio of body size, never as a fixed point
  value with a floor.

Divergence: Atelier ships three independent text scales, code-ligature control,
and display sizing tiers. This build has two text scales and no ligature or
display-tier controls.

## Zoom

Quick Settings exposes two independent text scales. `Content` runs from 80% to
200%. `Interface` runs from 80% to 140%. The status bar shows the Content value,
and `Cmd-=` plus `Cmd--` change Content only.

Scope: zoom scales the content surfaces - the code editor, the diff view, the
terminal, and the Markdown preview. All four share the one `Type::EDITOR` base,
so their text renders at the same size. The value rides the `EditorZoom` global;
each surface reads it every render. The editor and diff scale `Type::EDITOR` with
row height, caret, click mapping, and IME bounds following from that size. The
terminal scales the same base, re-measures its cell, and reflows the PTY grid.
The Markdown preview scales its `Type::EDITOR`-derived type and rhythm; column
widths and structural padding stay fixed, so larger text wraps sooner.

Interface scales the workspace rail, toolbar, tab strip, status bar, Explorer,
Git panel, inspector, find controls, empty states, and overlays. It also updates
the component theme so input fields follow the same scale. Fixed control heights
and panel widths do not scale. Both values persist in `settings.json` and restore
on launch. Rebuilding the light or dark theme preserves both scales.

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
| `icon_button` | 30-point square, `Radius::ROW`, hover and pressed fills, accent-free selected fill, accessible label, and visible tooltip |
| `count_badge` | Monospaced count on a 14% wash of its own semantic tint |
| `pill_tab` | Header tab: icon, label, optional count. Equal share of the header, `ROW` height, glass fill and top-lit hairline when selected |
| `empty_state` | One icon well, a serif title, and a short message |
| `file_glyph` / `folder_glyph` | Full-colour Material icon per name and extension, resolved from the ported theme. Never tinted by Git state |
| `project_menu` | 420-point project trigger, text only, middle-truncated |

Rules:

- Do not add a component for one local use.
- A component owns its own states. A caller must not re-implement hover or
  selection for it.
- Chrome icons come from the gpui-component icon set. File and folder glyphs
  come from Atelier's ported Material icon theme: a JSON manifest plus SVGs,
  embedded through `rust-embed` and resolved by `services/material_icons.rs`.
  GPUI's `img` element rasterises the SVG in full colour. The `Glyph` type
  carries either a tinted component icon or a Material resource path, and the
  identity glyph never carries Git meaning.

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

### Global Toolbar

- Keep the global toolbar at 52 points with the `toolbar` graphite token in
  light and dark appearances. Use the rail foreground tokens for its title and
  icon controls.
- Keep the repository search trigger light, 568 points wide, and visually
  centred. Search text and results use the normal ink tokens.
- The trailing action group may expose appearance, Changes, focus mode, and
  context visibility because those actions already have real application state.
  Do not add notification, account, or settings icons without a working surface.

### Workspace Rail

- Fixed 230-point rail with a graphite-to-petrol gradient and one hairline on its
  trailing edge. It is the only gradient in the application.
- Start with the active workspace identity and branch. Follow it with full-width
  `Files`, `Search`, and `Changes` navigation rows. The selected row uses the
  stable rail selection fill. `Changes` carries the changed-file count.
- `Files` selects the Explorer navigator. `Changes` selects the Git navigator.
  `Search` opens Search All Files without changing the selected navigator.
- Keep the `Workspaces` label, the scrollable workspace rows, and a labelled
  `Add Workspace` action at the bottom.
- Render every workspace exactly once in stored rail order. Selecting a
  workspace changes only the active marker and mounted content; it never removes
  or repositions a row. The `Workspaces` count includes the active workspace.
- Each row is 44 points with 4-point gaps: the full project name on the first
  line, its `Cmd-1` .. `Cmd-9` shortcut in smaller monospaced secondary text on
  the second. Never show initials, monograms, or paths.
- Positions past nine stay reachable without a shortcut.
- Show the changed-file count as a trailing high-contrast badge when it is above
  zero. Count each path once across staged, unstaged, and untracked states.
- Show non-zero per-workspace change counts with the same badge treatment.
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
- Explorer and Git changes share one 288-point navigator slot. The workspace
  rail selects which surface is mounted.
- The 48-point header shows the surface title and working actions. Explorer
  places branch in its own 40-point row, then a dedicated filter row with the
  refresh control. Git changes keeps its existing commit controls inside the
  surface.
- `Filter files...` is a real in-place filter. Typing updates visible Explorer
  rows immediately while keeping matching ancestors visible. Clearing the
  field restores the current expanded tree.
- Explorer rows are 32 points: chevron, identity icon, label. Treat the whole
  row, including its trailing empty area, as one pointer-cursor hit target.
- Render Git-ignored entries at reduced opacity, and keep them visible.
- An Explorer single click opens one replaceable preview tab. Opening another
  file with a single click replaces that preview in place, so browsing never
  fills the strip. A double click opens a permanent tab, or promotes the preview
  of that file to permanent, matching VS Code.
- Quick Open and Search All Files open permanent tabs. Opening a file that is
  already showing as a preview promotes that tab instead of adding a second one.

### Center Tabs

- Titanium chrome for the reader header, porcelain for the surface below it.
- Use a 36-point locator row followed by a 44-point tab and action row. This
  keeps orientation visible without spending 120 points on passive chrome.
- The locator shows a folder glyph, the workspace, a chevron glyph, and the
  selected file's parent location. Root files show `repository root`.
- Keep the selected file or terminal identity in the active tab only. Never
  repeat that title in the locator row.
- Use icon glyphs for breadcrumb separators. Never render raw `>` punctuation
  as interface chrome.
- Tab widths stay between 112 and 220 points.
- The tab scroller is the only flexible child and uses `min_w(0)`. It yields
  space before the trailing action group, so Preview, Raw, search, and New
  Terminal never clip at Standard or Compact widths.
- Render the selected tab as a `ROW`-height pill of `chrome_selection` glass with
  a top-lit hairline. No accent top rule, no selection border.
- Mark a preview tab with italic label text at `0.72` opacity. Do not add another
  icon.
- Double click a preview tab to promote it to a permanent tab.
- Place the close control at the leading edge of a closable tab. A tab that
  cannot close carries no close slot at all, so it has no empty gutter. A close
  click must not also select or reopen the tab.
- When the selected tab or workspace changes, move keyboard focus to its editor,
  terminal, or diff surface. Read-only previews return focus to the shell. This
  handoff also applies when closing or replacing the entity that owned focus.
- Keep the final terminal tab open and non-closable.
- Keep source/preview, wrap, search, and New Terminal in one trailing group.
  Render source/preview as labelled `Preview` and `Raw` controls for a file that
  supports both modes. New Terminal remains the far-right action.

### Context Rail

- Replace the generic inspector with a 300-point reading context rail in Wide
  mode. The rail scrolls as one surface and uses sticky visual sections.
- The context rail has no repeated file-identity header. For Markdown, its first
  visible section is `On This Page`. Each heading scrolls the document to
  its block. Then show local linked files that resolve inside the workspace.
- Show Git provenance from the live snapshot: last commit, recent authors,
  branch, short HEAD, changed state, and repository sync status when available.
- End with compact file metadata. Never repeat information already visible in
  the reader header.
- In Standard mode the rail is hidden. In Compact mode both side surfaces are
  hidden and the navigator opens as an overlay.

### Editor

- Virtualised rows through `uniform_list`. Only the requested range is built.
- Syntax highlighting is scoped to the visible byte range. A highlight query
  reports overlapping captures, so resolve them narrowest-wins into
  non-overlapping runs before painting.
- `Cmd-S` saves. The status bar reports the result.
- A click places the caret at the character under the pointer and focuses the
  editor. Map the point through the frame and monospace advance captured during
  paint plus the live scroll offset.
- A double click selects the whole identifier under the pointer. Identifier
  tokens contain letters, numbers, and underscores.
- A drag, or `Shift` with an arrow, extends one selection. `Cmd-A` selects all.
  Paint the selected columns with the `selection` fill behind the text. Typing,
  `backspace`, or newline replaces the selection first.
- `Cmd-C` and `Cmd-X` copy the selection; `Cmd-X` then deletes it. Save and find
  stay on the shell keymap.
- Wrap is off by default. A long line then overflows into a horizontal scroll,
  driven by `uniform_list` under `Unconstrained` horizontal sizing. Rows hug
  their text so the list measures the widest line.
- A toolbar button and the `View: Toggle Word Wrap` command flip soft wrap on the
  selected source file. Wrap on drops virtualisation, because wrapped rows have
  varying heights that `uniform_list` cannot express, so it renders every row in
  a plain scrolling column. Wrap is opt-in, so that cost is a reader's choice.
- The editor implements the GPUI input handler so an input method can compose
  Vietnamese, Japanese, and any other multi-stage text in place.

Divergence: no per-file settings and one selection only (no multiple cursors).
In wrapped mode a click sets the caret's row exactly; its column is exact on the
first visual line and approximate on a wrapped continuation, and mouse drag
selection is off (keyboard selection still works).

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
- Match native macOS terminal selection: drag selects text, double-click selects
  one semantic token, triple-click selects a line, and Option-drag selects a
  rectangular block.
- `Cmd-C` copies the active terminal selection. `Cmd-A` selects the complete
  buffer. `Cmd-V` keeps bracketed-paste protection.
- Draw marked IME text at the terminal caret. Report the same caret rectangle
  to macOS so candidate windows follow the insertion point.
- Draw the focused terminal caret as a thin bar that blinks every 500 ms. Never
  recolor the glyph under it when cursor keys move the insertion point.
- Encode modified navigation keys, Insert, and F1 through F12 as xterm control
  sequences. Printable text still uses the input handler only.
- Forward SGR and legacy mouse reports when a terminal application requests
  them. Hold Option to force local block selection instead.
- Honor terminal clipboard requests and PTY replies. Keyboard input clears the
  active selection and returns the viewport to the live prompt.
- `Cmd-F` opens a terminal-local search field. Search the complete scrollback
  with smart case. Return and `Shift-Return` move between matches. Highlight
  every visible match and scroll the active match into view.
- Detect OSC 8 hyperlinks, web URLs, and existing workspace paths. Underline a
  detected link on hover. `Cmd-click` opens web links externally and file links
  in the owning workspace tab.
- Load zsh shell integration without replacing user startup files. Track prompt,
  command start, command finish, exit status, and current working directory.
  Use that metadata for command navigation and file-link resolution.
- Remove `CLAUDE_CODE_CHILD_SESSION` and host-only `NO_COLOR` from each terminal
  shell environment. A Claude TUI there must behave like one opened from VS Code.
- Reflow soft-wrapped primary-screen lines after a column resize. Preserve hard
  line breaks, scrollback, selection anchors, and alternate-screen dimensions.

### Markdown Preview

- Open `.md` in Preview by default and keep Source one toggle away (`Cmd-D`).
- Hold prose on a 720-point measure. Let card blocks bleed to 880: tables and
  fenced code cards read better wider, prose does not.
- Set document viewport horizontal padding from the window mode: 48 points in
  Wide, 32 in Standard, and 24 in Compact. Let the scroll viewport fill the
  reader from top to bottom. Heading and block rhythm own vertical spacing.
  Keep structural padding fixed while editor zoom scales type and its derived
  vertical rhythm.
- Publish headings and local links to the shell context rail. The Markdown view
  owns parsing and scroll targets; the shell owns the surrounding rail.
- Heading ratios are H1 `2.875`, H2 `1.75`, H3 `1.1875`, H4 `1.00`, H5 and H6
  `0.9375`. H1 and H2 use the document serif face. H1 resolves to 46 points at
  default zoom. Draw the H1 and H2 rule as an accent lead segment followed by a
  hairline.
- Resolve the default heading sizes to H1 46, H2 28, H3 19, H4 16, and H5-H6
  15 points. Use semibold weight throughout. Give H1 and H2 56 points before
  and 20 points after. Give H3-H6 32 points before and 12 points after. This
  larger section rhythm separates long reading passages without adding cards.
  H1 starts the document without an extra top gap.
- Set prose at 16 points with a `1.62` line-height and a 20-point block gap.
  Links use the accent and underline. Emphasis stays italic; strong stays
  semibold. Inline code uses the mono face on the low-contrast selection fill.
- Give list content a 24-point indent and an eight-point marker gap. Keep list
  line-height at `1.55`. Task items use a square checkbox; checked items use the
  completed workflow colour and secondary struck text.
- Give quotes a three-point accent rule, a 16-point text inset, and `1.62`
  line-height. Keep quote text italic and secondary, without a card fill.
- Render a divider as a centered three-dot ornament with 28 points of vertical
  gap, accent in the middle.
- Set code cards at 14.5-point mono type with `1.35` line-height. Keep the
  language header, line numbers, 12-point body inset, eight-point radius, and
  the low-contrast panel fill. Parse a supported fence language once on reload
  and reuse the shared syntax palette while rendering visible code cards.
- Keep long code lines on one line. Show a horizontal scrollbar only when
  content exceeds the card width.
- Place a copy control at the right edge of the code header. After copying,
  show check feedback for about two seconds.
- Set table cells at 14.5 points with `1.45` line-height and 12-point insets.
  Use a raised semibold header, subtle alternating rows, a one-point border, and
  an eight-point radius.
- In the context outline, mark the active heading with semibold text and the
  terracotta accent. Do not add a second reader identity header.
- Clicking an outline item aligns that heading with the top of the reader,
  even when it is already visible.
- Keep the accent budget small: the H1 and H2 rule lead, the quote rule, and the
  divider centre. Light and dark appearances use the shared `editor`, `panel`,
  `raised`, `border`, `ink`, `ink_secondary`, `accent`, and workflow tokens.

Divergence: Atelier renders one selectable native document so selection crosses
blocks. GPUI has no equivalent of `NSTextStorage`, so this build renders a tree
of block elements and selection is per block. Mermaid figures, images, callouts,
footnotes, and front-matter cards are not ported.

### Quick Open, Command Palette, and Search All Files

- One shared floating panel: 640 points wide, 410 tall, `PALETTE_FIELD` query
  field on the `editor` fill, `raised` results, `chrome` footer.
- The global search trigger is 568 points wide and reads `Search files, symbols,
  commits...`. Clicking it opens unified repository search.
- `Cmd-K` and `Cmd-P` open Quick Open, `Cmd-Shift-P` the Command Palette, and
  `Cmd-Shift-F` Search All Files.
- Show the file name first and a monospaced relative path second.
- Rank paths with a fuzzy matcher over the shared workspace file index.
- Unified repository search returns typed File, Symbol, and Commit results.
  Symbol results come from declarations in indexed text files. Commit results
  come from the live Git snapshot. Never label a filename as a symbol.
- Move the active result with Up and Down. Return opens the active item. File
  and symbol results open their source file; symbol results also reveal their
  line. Commit results open the Git navigator and identify the matching commit.
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
- Display each change section as a directory tree, not a flat list. Compact a
  single-child directory chain into one row (`src/app`). Directories sort
  before files, both alphabetically. Directory rows carry no Git status and no
  action; they are always expanded, never collapsible. File rows show the file
  name only - the tree carries the directory.
- Show recent commits below changes with subject, author, relative time, and
  short hash.
- Push is one primary control: stage everything, commit the composer subject,
  then push the current branch. Report which stage failed.
- Open file diffs as center tabs, never inside the sidebar.
- Omit raw git metadata from a diff preview: `diff --git`, `index`, `---`,
  `+++`, mode lines, rename/similarity lines, and `\ No newline` markers.
  Keep hunk headers, line numbers, context, additions, and deletions.
- Bound a diff preview at 20,000 rendered lines.
- A text diff is a read-only, editor-grade surface. The reader drag-selects
  code and copies it with `Cmd-C`; `Cmd-A` selects all. Copy yields the code
  alone, without line numbers or `+`/`-` signs.
- Show real file line numbers in two fixed 40-point gutters (old, new), then a
  16-point sign column, all monospaced. An added row fills only the new
  gutter, a deleted row only the old one.
- Rows never wrap. A long line drives a horizontal scrollbar, like the editor.
- Tint the full row background for additions and deletions at low opacity,
  across the whole row width, and colour the sign.
- Syntax-highlight the code. Colours come from two reconstructed
  pseudo-documents - the new side is context plus additions, the old side is
  context plus deletions - each parsed once and queried per visible range like
  the editor.
- Emphasise the within-line change the way VSCode does. Pair each deleted line
  with the added line that replaced it, trim the shared prefix and suffix, and
  paint the differing span in a stronger tint.
- Render a hunk header as its own row on a raised band: the `@@` range and its
  trailing context in the accent-adjacent Git colour. Separate hunks visually
  by that band, not by blank lines.

- An image change opens side by side instead of a text diff: the HEAD blob
  (copied to a temp file) on the left, the working tree on the right, each
  under a monospaced label band. An added or deleted image shows the one
  surviving side full-view instead, with no "absent" column.
- On an HTML text diff, the Preview toggle opens the working-tree file
  rendered in the web preview.

Divergence: no branch picker, no discard, no upstream counts, and no
commit-message generation.

### Workspace Status

- Keep the status bar at 28 points. The leading group shows branch, short HEAD,
  and changed count from the live Git snapshot.
- The trailing group identifies the active surface with real state: file type
  or language, Preview or Raw mode when available, line-ending mode for text,
  working-tree clean or dirty state, content token estimate, and content zoom.
- Omit a value when the active tab cannot provide it. Never fill the status bar
  with invented metadata.
- A status item only uses a pointer cursor when it has a click action.

### File Previews

- An image file (`png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `ico`) opens as
  a pure preview tab: centred, contained, on the editor surface. There is no
  Source mode. SVG stays a text file in the editor.
- An HTML file opens in the editor with a Preview mode that renders the
  working-tree file in a native webview (`gpui_wry`, the Gate 2 stack).
  The webview is created lazily on the first Preview render, reloads on
  save, and is hidden whenever its tab is not the selected Preview tab of
  the active workspace or an overlay is open - a native child view paints
  over the GPUI canvas otherwise.
- A video file (`mp4`, `mov`, `m4v`, `webm`) opens as a player tab: a
  generated wrapper page in the webview holds a plain `<video>` on a black
  surface, so hovering shows only the native control bar - never Safari's
  standalone media document, which dims the whole frame. Same lazy creation
  and visibility rules as the HTML preview. A video
  change in the Git panel opens this player for the working-tree file;
  there is no text diff for it.
- Markdown keeps its block-tree preview. One Preview toggle (`Cmd-D`) serves
  all three.

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

The native macOS menu bar exposes the same actions as the shell keymap and
Command Palette. `src/app/menu.rs` only maps menu items to those shared actions;
it does not own duplicate behavior. The menu order is Artifex, File, Edit, View,
Go, Workspaces, Git, and Window. macOS application actions such as Hide,
Minimize, Full Screen, and Quit remain global actions.

Artifex also shows one macOS status item with the `slider.horizontal.3` symbol.
It opens a transient quick-settings panel with a fixed 300 point width. The
vertical panel exposes Content from 80% through 200%, Interface from 80% through
140%, focus mode, sidebar, inspector, word wrap, dark mode, reset text size, and
Quit Artifex. The inspector
control is unavailable outside the wide layout. Controls dispatch shared GPUI
actions and stay open for repeated changes. `Shell` remains the only state owner
and publishes the actual values back to AppKit after every render. The panel
owns only the native surface and action bridge. Its popover moves to the active
Space, even when the Artifex window is elsewhere.

| Shortcut | Action |
|---|---|
| `Cmd-1` .. `Cmd-9` | Select workspace by rail position |
| `Cmd-0`, `Cmd-O` | Add Workspace, through the folder picker |
| `` Cmd-` `` | Next workspace, wrapping to the first |
| `Cmd-T`, `Cmd-Shift-;` | New Terminal |
| `Cmd-Return` | Push: stage, commit, push |
| `Cmd-B` | Reveal the active file in the Explorer |
| `Ctrl--` / `Ctrl-=` | Navigate back / forward through opened files |
| `Cmd-F` / `Cmd-Alt-F` | Find in the active file or terminal / replace in the active file |
| `Cmd-G` / `Cmd-Shift-G` | Next / previous find match |
| `Cmd-Shift-C` | Insert `path:line` of the active file into the terminal |
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
| `Cmd-=` | Zoom in the editor text |
| `Cmd--` | Zoom out the editor text |

Rules:

- A palette command and its shortcut call the same action. Do not duplicate the
  behavior at the call site.
- Register the shell's bindings after the component kit's. When two bindings
  match at the same context depth, the later registration wins, and the kit
  claims `escape` for its own query field.
- Do not reuse a shortcut for a second action.
- Keep shortcut labels monospaced in the Command Palette.
- Divergence from the parent app: navigate forward is `Ctrl-=`, not
  `Ctrl-Shift--`. GPUI folds Shift into the produced character for
  punctuation keys, so `Ctrl-Shift--` cannot be told apart from `Ctrl--`.
- Divergence from the parent app: the `Option-Z` shortcut is not ported. Word
  wrap is available from Quick Settings, the View menu, the editor toolbar, and
  the Command Palette.

## Accessibility Rules

- Give every icon-only control a label.
- Show that label in a tooltip on pointer hover. Keep the hit target keyboard
  reachable when GPUI exposes a focus path for that surface.
- Never convey state by color alone. Git status carries a letter, ignored files
  carry help text, and a selected tab carries a fill plus ink change.
- Keep keyboard focus stable after an overlay closes.
- Keep contrast stable on the rail by using its own foreground tokens.
- Give the active outline row a fill or weight change in addition to accent
  colour. Put a dismissible scrim behind the Compact navigator overlay.

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

## User Settings

Durable preferences live in
`~/Library/Application Support/Artifex/settings.json`, independently from the
open workspace session.

- Stored: Content text scale, Interface text scale, light or dark appearance,
  sidebar visibility, inspector visibility, and global word wrap.
- Not stored: focus mode. It is a temporary concentration state and every launch
  starts with the normal panel layout.
- `Shell` owns the live values. The AppKit panel only dispatches shared actions
  and displays the snapshot published by `Shell`.
- The file is pretty JSON with a schema version. Missing, corrupt, or future
  files use safe defaults. Each text scale is clamped to its supported range.
- Writes happen only when the snapshot changes and use a sibling temp file plus
  rename. If `settings.json` is absent, the first launch migrates appearance,
  zoom, and panel visibility from the legacy fields in `session.json`.

## Session Persistence

The shell remembers which workspaces are open and which file tabs each one
holds, in `~/Library/Application Support/Artifex/session.json`.

- Stored: workspace roots in rail order, the active workspace, each
  workspace's file tabs with their source/preview mode, and the selected tab.
- Not stored: terminal tabs (a PTY cannot be serialized; each restored
  workspace opens one fresh terminal), diff tabs (the Git state they showed
  has moved on), and per-tab editor or scroll state. Global word wrap belongs
  to `settings.json`.
- The file is written on every state change, from `render`, only when the
  snapshot differs from the last write, atomically via temp file plus rename.
- Restore degrades silently. A missing or corrupt session file, a deleted
  root, or a deleted file skips the entry; with nothing left the shell falls
  back to the launch-argument root.
- The schema carries a version number. A file with an unknown version is
  ignored whole, never half-read.

## Source of Truth

| Area | Source |
|---|---|
| Product and pinned revisions | [README.md](README.md) |
| Feasibility result and defects | [FEASIBILITY.md](FEASIBILITY.md) |
| Window, bundle, and modes | [src/main.rs](src/main.rs) |
| Colors, metrics, typography, breakpoints | [src/theme.rs](src/theme.rs) |
| Shared chrome components | [src/app/chrome.rs](src/app/chrome.rs) |
| Native macOS menu | [src/app/menu.rs](src/app/menu.rs) |
| macOS quick settings | [src/app/quick_settings.rs](src/app/quick_settings.rs) |
| Shell, rail, split, status bar, actions | [src/app/shell.rs](src/app/shell.rs) |
| Sidebar and inspector | [src/app/panels.rs](src/app/panels.rs) |
| Center tabs | [src/app/center.rs](src/app/center.rs) |
| Overlays | [src/app/overlays.rs](src/app/overlays.rs) |
| Editor | [src/app/editor.rs](src/app/editor.rs) |
| Diff view | [src/app/diff.rs](src/app/diff.rs) |
| Markdown preview | [src/app/markdown.rs](src/app/markdown.rs) |
| Terminal | [src/terminal/mod.rs](src/terminal/mod.rs) |
| Git | [src/services/git.rs](src/services/git.rs) |
| Filesystem watching | [src/services/watch.rs](src/services/watch.rs) |
| User settings | [src/services/settings.rs](src/services/settings.rs) |
| Session persistence | [src/services/session.rs](src/services/session.rs) |
| Parent design contract | `atelier/DESIGN.md` |

Update this document when a shared token, breakpoint, component contract, or
design rule changes.
