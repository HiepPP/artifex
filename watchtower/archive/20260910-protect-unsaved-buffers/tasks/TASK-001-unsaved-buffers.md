# TASK-001 Protect unsaved buffers

Group: standalone
Class: risky

## Brief

Goal: Never silently lose an unsaved buffer through tab close, workspace close, or preview replacement.

Change: Route close requests through one guarded Shell helper. Preserve dirty previews before replacement.

How:
- Update the design contract first.
- Prompt Save / Discard / Cancel only when the requested tab or workspace contains dirty editors.
- Save writes the affected dirty editors using the current save method, then closes only after all saves succeed.
- On save failure, show the error in the existing status bar and leave the target open.
- Discard closes the requested target. Cancel and prompt dismissal leave it unchanged.
- Resolve the target by workspace entity and tab ID after the asynchronous prompt, never by a stale index.
- Prevent duplicate close prompts. Keep the last workspace and last terminal open as before.
- Route both keyboard/menu and tab close-button paths through the guard.
- Before opening another file, promote dirty preview tabs to permanent tabs. Keep clean preview replacement unchanged.
- Reuse nearby code without new dependencies or broad abstractions.

Files:
- [DESIGN.md](../../../../DESIGN.md): define the close and preview rules.
- [src/app/shell.rs](../../../../src/app/shell.rs): guard close requests and handle the answer.
- [src/app/center.rs](../../../../src/app/center.rs): route the close button through the guard.
- [src/app/workspace.rs](../../../../src/app/workspace.rs): preserve dirty preview tabs.

Expected result:
- Unsaved buffers remain available unless saved successfully or explicitly discarded.

## Verify

- `./scripts/test.sh`: existing non-UI checks pass.
- `./scripts/build.sh`: native bundle builds.
- Native tab checks: Cancel preserves buffer; Save persists text and closes; Discard closes without saving.
- Native checks exercise both keyboard/menu and close-button paths.
- Native workspace checks: Cancel preserves dirty files; Save saves all dirty files; Discard leaves disk unchanged.
- Force a save failure: the target stays open and the error is visible.
- Browse from a dirty preview: the old buffer stays in a permanent tab; clean previews still replace each other.
- Confirm clean tabs close immediately and the last terminal/workspace rules remain intact.
- Restore the original app session and temporary fixtures. `git diff --check` passes.
