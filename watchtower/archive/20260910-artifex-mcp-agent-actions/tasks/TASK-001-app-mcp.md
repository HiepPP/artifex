# TASK-001 Expose app actions through MCP

Group: standalone
Class: risky

## Brief

Goal: Let an external agent call the running Artifex app to pull code into an open workspace.

Change: Manual app pull gains an MCP entry point with the same safety checks and visible results.

How:

- Update the scope contract first. Allow a narrow MCP server without adding an agent panel or model runtime.
- Align repository guidance that currently bans MCP with this limited scope.
- Use Streamable HTTP on loopback, served by the running Artifex app. Support Claude Code and Codex explicitly.
- Check current official MCP documentation when selecting the Rust integration.
- Keep a stable endpoint across app restarts. Document its URL, port configuration, and port-conflict errors.
- Require a local bearer token. Document token creation, private storage, rotation, and client configuration without committing secrets.
- Document registration and removal commands for both `claude mcp` and `codex mcp`, including configuration scope and authentication.
- Include copyable configuration examples for both clients and any required session restart or reconnect steps.
- Keep access limited to agents on the same machine. Remote agents and network exposure are outside this task.
- Define server startup and shutdown. Client connections must reuse the running app rather than launch another app instance.
- Expose workspace discovery with stable IDs, paths, branches, and upstreams.
- Expose a pull tool requiring workspace ID and expected branch. Reject unknown targets or branch mismatches.
- Route requests to the owning app workspace, including workspaces that are not currently selected.
- Reuse pull guards, background execution, operation identity, and file/Git refresh behavior.
- Preserve unsaved buffers and working-tree changes. Never stash, reset, rebase, switch branches, or force a merge.
- Return structured completion or failure. Include workspace, branch, before/after HEAD, and refresh outcome.
- Do not report a queued request as completed. Bound waits and distinguish timeout from confirmed cancellation.
- Reject overlapping operations. Handle closed workspaces, disconnected clients, and app shutdown without stale updates.
- Document the agent sequence: push upstream, discover workspace, call pull, inspect result.

Files:

- [DESIGN.md](../../../../DESIGN.md): MCP scope, lifecycle, tools, and behavior contract.
- [AGENTS.md](../../../../AGENTS.md) and [CLAUDE.md](../../../../CLAUDE.md): align MCP scope guidance.
- [Cargo.toml](../../../../Cargo.toml) and [Cargo.lock](../../../../Cargo.lock): dependencies if needed; preserve GPUI pins.
- [src/main.rs](../../../../src/main.rs): server lifecycle wiring if needed.
- [src/services/mod.rs](../../../../src/services/mod.rs): register the MCP service.
- [src/services/mcp.rs](../../../../src/services/mcp.rs): proposed new protocol and request bridge module.
- [src/app/shell.rs](../../../../src/app/shell.rs): workspace-targeted dispatch and completion replies.
- [src/app/workspace.rs](../../../../src/app/workspace.rs): reuse identity and operation guards; extend only if needed.
- [src/services/git.rs](../../../../src/services/git.rs): reuse pull behavior; add result metadata only if needed.
- [src/tests.rs](../../../../src/tests.rs): observable routing and Git safety tests.
- [README.md](../../../../README.md): connection setup and agent call example.

Expected result:

- Both Claude Code and Codex register Artifex, discover an open workspace, and pull its upstream through MCP.
- Artifex shows updated clean files and Git state after the pull completes.
- Invalid or unsafe requests return actionable errors without losing local work.

## Verify

- Run `./scripts/test.sh` and `./scripts/build.sh`; both must pass.
- Relaunch the built bundle. Register Artifex separately in Claude Code and Codex using the documented commands and token setup.
- In each client, verify initialization, tool discovery, valid calls, invalid arguments, and rejection of missing or invalid tokens.
- Restart Artifex and reconnect both clients with unchanged registration. Verify the endpoint remains stable and loopback-only.
- Use temporary Git fixtures with a bare remote and two clones. Push one commit from the agent clone.
- For each client, push a fresh fixture commit, then call workspace discovery and pull through that client's MCP tools.
- Verify returned HEAD, local HEAD, visible file content, and Git state agree after each client's pull.
- Record both client versions, registration commands, and call results. One client passing cannot satisfy this task.
- Target a non-selected workspace. Verify the selected workspace remains unchanged.
- Test unknown workspace, wrong branch, missing upstream, divergence, overlapping operations, and unsaved buffers.
- Verify non-overlapping working-tree edits survive and conflicting edits produce an error without data loss.
- Close a target workspace during a request. Verify no response or refresh attaches to a replacement workspace.
- Verify app shutdown releases the endpoint and pending calls terminate without false success.
- Restore temporary client registrations and app state. Remove task-owned test artifacts safely; record commands and native evidence.
