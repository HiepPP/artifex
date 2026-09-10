# TASK-001 Outcome

## Outcome

Status: DONE

Changed:
- Added one shared close-request guard in [Shell](../../../../src/app/shell.rs).
- Routed the tab close button, Close Tab action, and workspace close menu through that guard.
- Promoted dirty preview tabs before another file opens in [Workspace](../../../../src/app/workspace.rs).
- Updated [DESIGN.md](../../../../DESIGN.md). No dependencies or unrelated features were added.

Contract:
- Save closes only after every requested dirty editor saves successfully.
- Discard closes without saving. Cancel and Escape preserve the target and its buffer.
- A save error appears in the existing status bar and leaves the target open.
- Prompt completion resolves workspace entity and tab ID, not stale array positions.
- Only one close prompt can be active. Clean close and last-terminal/workspace rules remain unchanged.
- Dirty previews remain available in permanent tabs. Clean previews still replace each other.
- App quit, autosave, recovery, and external-file conflict handling remain outside this change.

Verified:
- `./scripts/test.sh`: 77 passed, zero failed, using the pinned toolchain.
- `./scripts/build.sh`: passed. The rebuilt bundle was relaunched for native checks.
- Native tab checks: Cancel preserved text; Save persisted text and closed; Discard left disk unchanged.
- Native close-button Cancel and keyboard `Cmd-Q` Cancel both preserved the dirty buffer.
- Native Escape dismissed the prompt without closing or saving.
- Native workspace checks: Cancel retained two dirty files; Save wrote both; Discard preserved disk contents.
- Forced tab and workspace save errors using temporary directory targets: dirty buffers stayed open.
- Preview check: dirty `a.txt` survived browsing to `b.txt`; clean `b.txt` was replaced by `c.txt`.
- Session file lists confirmed only `a.txt` and `c.txt` remained. Reopening `a.txt` showed its unsaved marker.
- Clean tab closed immediately. The last terminal and last workspace stayed open.
- `git diff --check`: passed. Native logs contain no panic.
- Restored all 11 original workspaces. Temporary fixtures moved to Trash.

Evidence:
- Test/build logs: `/tmp/artifex-unsaved-tests.log`, `/tmp/artifex-unsaved-build.log`.
- Preview screenshot: `/tmp/artifex-unsaved-preview.png`.
- Native checks used disposable fixture `artifex-unsaved-uce5h2b0`, now in Trash.

Limitations:
- GitNexus impact failed due to its existing database-version mismatch. Call sites were checked directly in source.
- A multi-file Save can save earlier files before a later save fails; the workspace stays open for recovery.
- No crash recovery or app-quit protection was added.
