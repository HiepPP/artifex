# Learn 20260910-artifex-mcp-agent-actions

## Summary

Discrepancy: none. The plan matches the delivered local MCP server and its two workspace tools.

## Per TASK

- TASK-001: match. The app owns the authenticated HTTP server, lists workspaces, and reuses guarded pull behavior. Claude Code and Codex connection, pull, reconnect, and lifecycle checks passed.

## Plan-Level

- none. All planned checks passed.

## Lessons

- Test both real clients, including authentication and reconnect after an app restart.
- Do not pass an empty tools option to Claude Code during MCP checks; it hides the tools being tested.
- A pull timeout means the outcome is unknown. It does not cancel or roll back Git.
- Use a long enough delay to close and reopen a workspace before its old pull completes.
