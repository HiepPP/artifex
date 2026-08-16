# Reading Room Design QA

## Artifacts

- Source visual truth: `app-design/reading-room-implementation-blueprint.png`
- Native implementation: `app-design/reading-room-implementation-wide.png`
- Combined comparison: `app-design/reading-room-comparison-final.png`
- Compact evidence: `app-design/reading-room-implementation-compact.png`
- Standard Raw evidence: `app-design/reading-room-implementation-standard-raw.png`
- Standard Changes evidence: `app-design/reading-room-implementation-standard-changes.png`
- User-selected reference: `app-design/gap-audit-reference.png`
- Final Wide capture: `app-design/gap-audit-after-final-wide-native.png`
- Final comparison: `app-design/gap-audit-after-final-comparison.png`
- Standard capture: `app-design/gap-audit-after-final-standard-native.png`
- Compact capture: `app-design/gap-audit-after-final-compact-native.png`
- Compact navigator: `app-design/gap-audit-after-final-compact-overlay-native.png`
- Dark Wide capture: `app-design/gap-audit-after-final-wide-dark-native.png`
- Explorer filter: `app-design/gap-audit-after-final-filter-native.png`
- Unified search: `app-design/gap-audit-after-final-search-native.png`
- Symbol reveal: `app-design/gap-audit-after-final-search-open-native.png`
- Active outline: `app-design/gap-audit-after-final-outline-scroll-native.png`
- Density audit source: `app-design/audit-2026-08-16/00-user-breadcrumb-reference.png`
- Density audit Wide: `app-design/audit-2026-08-16/03-after-window.png`
- Locator comparison: `app-design/audit-2026-08-16/05-before-after-locator.png`
- Density audit Compact: `app-design/audit-2026-08-16/06-after-compact.png`
- Density audit Standard: `app-design/audit-2026-08-16/08-standard-actions-fixed.png`

## Normalization

- Source pixels: 1512 x 1040 at 1x.
- Implementation window: 1512 x 1000 logical points at 2x native density.
- Implementation pixels before normalization: 3024 x 2000.
- Implementation pixels after normalization: 1512 x 1000.
- Combined comparison pixels: 3088 x 1104.
- The source includes annotation margins. The implementation capture contains only the native window.

## State

- Theme: light for the source comparison.
- Workspace: `artifex` on `main`.
- Document: `README.md` in Preview mode.
- Navigator: Files.
- Context rail: visible.
- UI and content zoom: 100 percent.

## Full-view Comparison

- The 230-point rail, 288-point navigator, flexible reader, and 300-point context rail match the target hierarchy.
- Top chrome uses the 52-point band and the 568-point global search control.
- The reader keeps a 720-point maximum measure and labelled Preview and Raw controls.
- The context rail includes outline, linked files, commit data, authors, Git state, and file metadata.
- Dynamic README content differs from the conceptual copy in the source. The structure remains equivalent.

## Focused Region Comparison

- Typography: H1 uses a 46-point serif face. UI text keeps the native sans face.
- Spacing: file rows use 32 points. The status bar uses 28 points.
- Colors: graphite, porcelain, terracotta, borders, and semantic Git colors match existing tokens.
- Images: the target has no product imagery. Existing Material icons remain sharp native assets.
- Copy: global search, Files, Search, Changes, Preview, Raw, and context labels match the target.
- Icons: existing Material icons remain aligned and consistent across both themes.

## Findings

- No actionable P0, P1, or P2 findings remain.
- P3: the status bar contains fewer secondary indicators than the conceptual source.
- This difference preserves the current scope and does not block repository reading.

## Comparison History

### Pass 1

- P2: Markdown H1 used the UI sans face at 29.6 points.
- Fix: changed H1 to Times New Roman at 46 points.
- P2: the outline did not show the active heading.
- Fix: derived the active heading from the Markdown list scroll position.

### Pass 2

- Evidence: `app-design/reading-room-comparison-final.png`.
- H1 now matches the serif hierarchy.
- The active outline item now uses the terracotta state.
- No P0, P1, or P2 differences remain.

## Responsiveness

- Wide: 1512 x 1000 shows navigator and context rail.
- Standard: 1200 x 850 hides context and keeps the navigator inline.
- Compact: 850 x 800 hides context and opens the navigator as an overlay.
- No persistent control overlaps or clips at these widths.

## Interactions Tested

- `Cmd-K` opens Quick Open and opens `README.md`.
- `Cmd-D` switches between Preview and Raw.
- `Cmd-E` switches between Files and Changes.
- `Cmd-Shift-R` opens and closes the compact navigator.
- Outline clicks scroll the Markdown reader.
- Dark mode renders all four regions with readable contrast.
- The app stayed alive through all native checks. No panic appeared.

## Accessibility

- Main navigation and reader modes are keyboard reachable.
- Selected and active states use fill plus text color.
- Light and dark themes retain readable contrast.
- UI zoom reset to 100 percent before comparison.
- No new motion was added.

## Implementation Checklist

- [x] Update `DESIGN.md` before behavior changes.
- [x] Run impact analysis before symbol edits.
- [x] Implement the Reading Room shell.
- [x] Build the native bundle.
- [x] Run all non-UI tests.
- [x] Relaunch and verify the native app.
- [x] Compare source and implementation in one image.
- [x] Resolve all P0, P1, and P2 findings.

## Gap Audit Round - 2026-08-16

| Area | Gap | Result |
|---|---|---|
| Shell | Light toolbar differed from the graphite target | Added graphite global chrome with a light search trigger |
| Explorer | Branch and filter shared one row | Split branch and filter into separate rows |
| Explorer | Filter opened Quick Open | Added a real inventory-backed tree filter |
| Explorer | Ignored and large files were absent from filtering | Added a separate cancellable Explorer inventory |
| Context | File identity repeated above the outline | Context now starts with `ON THIS PAGE` |
| Context | Active outline did not follow scroll | Markdown emits active-heading events to Shell |
| Reader | H2 used UI sans | H1 and H2 use the reading serif face |
| Search | Quick Open searched paths only | Added typed File, Symbol, and Commit results |
| Search | Keyboard selection could leave the viewport | Selected results now scroll into view |
| Search | Late batches could repopulate cleared queries | Added generation invalidation and cancellation checks |
| Status | Missing document and Git context | Added mode, line ending, tree state, token estimate, and zoom |
| Responsive | Compact toolbar and status could overflow | Added flexible toolbar controls and condensed status labels |
| Responsive | Compact scrim intercepted navigator clicks | Scrim now begins after the navigator |

Native verification:

- Wide 1512 x 1000 shows rail, navigator, reader, and context.
- Standard 1200 x 850 shows rail, navigator, and reader.
- Compact 850 x 800 shows rail and reader.
- Compact Files opens a 288-point navigator over a 0.12 scrim.
- Explorer filter returns `repository_search.rs`.
- `Cmd-K` returns `render_inspector` as a Symbol result.
- Return opens `panels.rs` and reveals line 1068.
- `Cmd-D` switches README between Preview and Raw.
- Scrolling README updates the active outline to `Dependency Versions`.
- Light and dark modes keep readable contrast.
- The latest bundle remained alive with no panic.

Automated verification:

- `./scripts/build.sh`: passed.
- `./scripts/test.sh`: 53 passed, 0 failed.
- `git diff --check`: passed.
- Conflict marker search: passed.
- GitNexus `detect_changes`: executed for the combined worktree.

Intentional differences:

- README copy, linked files, authors, and Git counts use real repository data.
- Bell, account, and settings controls remain absent without product behavior.
- The terminal tab remains open because the design contract requires it.
- No fake roles, avatars, badges, or upstream state were added.

## Density Audit Round - 2026-08-16

| Area | Finding | Result |
|---|---|---|
| Reader locator | A passive breadcrumb consumed 60 points | Reduced it to 36 points |
| Reader locator | The selected file title repeated the active tab | Locator now shows the parent location |
| Reader locator | Raw `>` punctuation looked like document text | Replaced it with a chevron glyph |
| Reader tabs | A 60-point strip left excessive empty space | Reduced it to 44 points |
| Navigator | The title used the same oversized 60-point header | Reduced it to 48 points |
| Responsive tabs | Tab content pushed trailing actions toward the edge | Added `min_w(0)` to the tab scroller |

Native verification:

- The root file locator reads `artifex`, chevron, `repository root`.
- The selected file name appears only in the active tab.
- Standard 1200 x 850 keeps Preview, Raw, search, and New Terminal visible.
- Compact 850 x 800 keeps the overlay and reader controls usable.
- The app stayed alive after resizing and relaunching.

Automated verification:

- `./scripts/build.sh`: passed.
- `./scripts/test.sh`: 53 passed, 0 failed.
- Existing compiler warnings remain non-blocking.

## Final Result

final result: passed
