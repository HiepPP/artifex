# Plan Context

## Shared Context

- The app currently exposes `list_workspaces` and `pull_workspace` through an authenticated local HTTP server.
- MCP requests use `Action`, a bounded channel, and `Shell::observe_mcp`.
- Workspace IDs identify open instances. Closing and reopening invalidates old IDs.
- Watcher scans update Git and the file index. Pull also reloads clean file views.

## Decisions

- Add exactly `refresh_workspace`, `open_file`, and `get_workspace_state`.
- Extend the existing request match and helpers. Keep the current transport, authentication, and timeout behavior.
- Use existing scan flags to report pending refresh. Do not add job IDs, queues, polling services, or dependencies.
- Keep cached Git data explicit. State reads do not fetch or scan.
- Update the design contract before implementation and the README before completion.
- Preserve unrelated code and existing tool contracts.

## Open Decisions

- None.

## References

- [Design contract](../../../DESIGN.md#local-mcp)
- [MCP server](../../../src/services/mcp.rs)
- [Shell](../../../src/app/shell.rs)
- [Workspace](../../../src/app/workspace.rs)
- [Editor](../../../src/app/editor.rs)
