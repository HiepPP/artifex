# TASK-001 Add three MCP workspace tools

Group: standalone (all three tools share the MCP server and Shell files)
Class: risky

## Brief

Goal: Let agents refresh local changes, open a result, and inspect app state through MCP.

Change: Extend the existing two-tool server to five tools with minimal changes.

How:

- Update the Local MCP contract first. Keep existing discovery, pull, authentication, and lifecycle behavior.
- Add argument structs, tool schemas, and `Action` variants beside the current implementations.
- Handle them in `Shell::observe_mcp`. Reuse workspace lookup and existing helpers with only small local additions.
- Reject unknown or closed workspace IDs and invalid arguments using the current error response pattern.
- Add no dependencies, generic dispatcher, job system, operation history, or background polling service.

### refresh_workspace

- Input: `workspace_id` only.
- Reject an active Git mutation using the current workspace guard.
- Reload clean file views with the existing helper. Preserve every dirty buffer and report skipped file paths.
- Preserve the selected workspace, selected tab, and editor cursor where possible; clamp the cursor after shorter content.
- Make only a small editor helper change if cursor restoration needs one. Do not redesign editor state.
- Schedule Git and file-index refresh through `scan_workspace(index, true, true, cx)`.
- Repeated calls use existing scan coalescing. Do not start parallel scans for one workspace.
- Return `workspace_id`, `file_views`, `skipped_dirty_paths`, and `git_and_index: scheduled`.
- Use `file_views: reloaded` or `dirty_buffers_preserved`, matching the existing pull result vocabulary.
- Do not fetch, pull, save files, or switch focus.

### open_file

- Input: `workspace_id`, workspace-relative `path`, and optional positive integer `line` starting at 1.
- Resolve an existing regular file inside the canonical workspace root. Reject absolute paths and resolved paths outside it.
- Check symlink targets through canonicalization. Keep existing file limits and supported file types.
- Validate the target before changing UI state. Reject unsupported line navigation before opening a non-text target.
- Reuse workspace selection and `Workspace::open_file` with `preview: false` to avoid replacing a preview buffer.
- Reuse an existing tab. Preserve unsaved content when that file is already open.
- For `line`, show the text source and call `EditorView::reveal_line` after converting to a zero-based index.
- Clamp lines beyond the end using existing editor behavior.
- Return `workspace_id`, the resolved path, and the actual one-based line when requested.
- Select the requested workspace and file in Artifex. Do not add operating-system activation behavior.

### get_workspace_state

- Input: `workspace_id` only. Read app state without filesystem walks or network requests.
- Return `workspace_id`, `path`, `active`, and `selected_tab` with its kind and path when applicable.
- Return `unsaved_paths` and `git` containing cached `branch`, `upstream`, `head_short`, and `busy`.
- Mark Git data with `source: cached`.
- Return `refresh_pending` from existing running and queued scan flags.
- After refresh, callers can read again until `refresh_pending` is false to observe the updated Git snapshot.
- This is current workspace state, not proof of a specific pull result or a persistent operation record.
- Return null for absent tab or upstream. Do not expose buffer contents or terminal output.

Files:

- [DESIGN.md](../../../../DESIGN.md): extend the Local MCP contract.
- [src/services/mcp.rs](../../../../src/services/mcp.rs): add schemas, argument validation, and actions.
- [src/app/shell.rs](../../../../src/app/shell.rs): handle the actions with existing workspace operations.
- [src/app/workspace.rs](../../../../src/app/workspace.rs): add only small helpers needed for reload and state reporting.
- [src/app/editor.rs](../../../../src/app/editor.rs): add minimal cursor restoration only if required.
- [src/tests.rs](../../../../src/tests.rs): test meaningful argument and path validation behavior.
- [README.md](../../../../README.md): document five tools, inputs, outputs, and asynchronous refresh.

Expected result:

- Agents can refresh local edits without pulling, open a file at a line, and inspect the resulting app state.
- Both current tools retain their behavior. Dirty buffers remain unchanged.

## Verify

- Run `./scripts/test.sh`: all tests pass, including malformed arguments and path-boundary cases.
- Test an outside-root path, an escaping symlink, missing files, and invalid line values: requests fail without UI changes.
- Run `./scripts/build.sh`: the native bundle builds with the repository toolchain.
- Relaunch the rebuilt bundle and inspect process output: no panic during the checks below.
- Use authenticated Claude Code and Codex registrations: each lists five tools and successfully calls all three new tools.
- Native check: edit a file on disk, refresh, then read state until settled; editor, Explorer, and Git reflect changes.
- Native check: refresh with a dirty editor; its text stays unchanged and its path appears in the skipped list.
- Native check: refresh preserves workspace, tab, and cursor; repeated refresh requests settle without duplicate concurrent scans.
- Native check: open an existing file at a line, including Markdown source; the correct workspace, tab, and line appear.
- Native check: open the same file twice and open a dirty file; no duplicate tab or overwritten buffer appears.
- Native check: inspect state with a terminal tab, a non-Git workspace, and dirty files; optional fields remain valid.
- Lifecycle check: closed workspace IDs fail; closing and reopening does not accept an old ID or stale refresh result.
- Regression check: existing list and guarded pull work; missing or incorrect bearer tokens still fail.
- Restore temporary client registrations, fixtures, and the user's app session after testing.
- Run `git diff --check`: no whitespace errors. Review the diff for only the listed scope.
