# TASK-001 Outcome

## Outcome

Status: DONE

Changed:
- Added `refresh_workspace`, `open_file`, and `get_workspace_state` to the current MCP action path.
- Reused workspace scans, permanent tabs, line navigation, and clean-file reloads.
- Preserved cursor position during reload and checked scan ownership before applying results.
- Updated [DESIGN.md](../../../../DESIGN.md) and [README.md](../../../../README.md). No dependencies were added.

Contract:
- The server exposes five tools through the existing authenticated local endpoint.
- Refresh preserves dirty buffers, reports skipped paths, and schedules the current scan.
- State reports cached Git data and running or queued scans. It does not fetch or scan.
- File opening validates canonical workspace boundaries and the 2 MB text limit before changing tabs.
- Existing pull guards, token checks, and workspace instance IDs remain in use.

Verified:
- `./scripts/test.sh`: 77 passed, zero failed, using the pinned toolchain.
- `./scripts/build.sh`: passed and rebuilt the native bundle. Existing dependency warnings remain.
- HTTP checks: five tools listed; invalid paths, escaped symlinks, missing files, binary files, and oversized text rejected.
- HTTP checks: zero, negative, fractional, and string line arguments rejected without changing workspace state.
- HTTP checks: unknown IDs, missing tokens, and wrong tokens rejected.
- Existing pull: wrong branch and dirty buffers rejected; valid upstream pull succeeded.
- Delayed fixture pull: refresh rejected while Git was busy. The temporary Git setting was removed.
- Claude Code: all three new tools called successfully with authenticated per-process MCP configuration.
- Claude verification removed inherited `CLAUDE_CODE_COORDINATOR_MODE` only from its child environment to expose tools directly.
- Codex: all three new tools called successfully with authenticated per-process MCP configuration.
- Native app: refreshed disk text appeared in the editor, Explorer entries appeared, and Git showed changes.
- Repeated refresh requests settled with `refresh_pending: false`; source uses the existing single-scan coalescing path.
- Native app: dirty text survived refresh and repeated opening; the skipped path matched the unsaved path.
- Native app: Markdown opened in source mode at line 3; a line beyond EOF clamped to line 5.
- Native app: a cursor at line 3, byte 2 survived reload; typing and saving placed the marker at that position.
- Native app: repeated opens and an inside-root symlink reused one file tab.
- State checks covered terminal tabs, a non-Git workspace, null upstream, and unsaved files.
- Native close/reopen: old `EntityId(2v1)` rejected; the reopened workspace received `EntityId(24v1)`.
- Source review: scan completion checks both root and workspace entity ID before applying results.
- `git diff --check`: passed. No application panic appeared in the native logs.
- Restored all 11 original workspaces and confirmed five live tools after restart.
- Temporary fixtures moved to Trash. Client configuration was per-process; no saved registrations changed.

Evidence:
- Build and test logs: `/tmp/artifex-workspace-tools-build.log`, `/tmp/artifex-workspace-tools-tests.log`.
- Native screenshots: `/tmp/artifex-tools-screen.png`, `/tmp/artifex-tools-dirty.png`.
- Client logs remain with the recoverable `artifex-tools-fvoukbpv` fixture in Trash.

Limitations:
- GitNexus impact calls failed because its server and database storage versions differ. Source references were reviewed directly.
- Scan ownership was verified in source; no delayed scan was forced across a close/reopen race.
- No commit or push was made.
