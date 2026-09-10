# TASK-001 Outcome

## Outcome

Status: DONE

Changed:
- Added the app-owned Streamable HTTP MCP server at `http://127.0.0.1:47831/mcp`.
- Added workspace discovery and workspace-targeted pull through the shared UI pull path.
- Added private bearer tokens, loopback binding, Host/Origin checks, and bounded request waits.
- Added Claude Code and Codex registration instructions, including authentication and removal.
- Updated the design contract and repository guidance. No commit or push was made.

Contract:
- Pull checks the live branch and uses the configured upstream with `--ff-only`.
- Unsaved buffers and overlapping Git operations block the request.
- Replies include before/after HEAD and explicit refresh state. Git/index refresh is asynchronous.
- Closing a workspace rejects its pending completion. Timeout or shutdown never claims a rollback.

Verified:
- `./scripts/test.sh`: 76 passed, 0 failed, with pinned Rust 1.97.1.
- `./scripts/build.sh`: passed and produced the app bundle. Existing compiler warnings remain.
- `git diff --check`: passed.
- HTTP initialization negotiated `2025-06-18`; missing authentication returned 401; foreign Origin returned 403.
- Direct MCP calls rejected unknown workspace, wrong branch, and missing required arguments.
- Native editing created an unsaved buffer; MCP rejected pull with `Save open files before pulling`.
- A delayed fixture pull rejected a second operation with `Git operation already in progress`.
- Closing the workspace during that pull returned a workspace-closed error without a stale UI update.
- Quitting during a request terminated its connection and released the listener.
- Restart accepted the same endpoint and token. `lsof` showed only `127.0.0.1:47831` listening.
- Codex CLI 0.153.2 registered the server and called `list_workspaces` then `pull_workspace` successfully.
- Fixture HEAD changed from `9d957fc7d05ba4f5cd0f237887c0210beb692802` to `799c67fff7dd66bb96c01a258cff5dbebeccd515`.
- Native UI showed the changed file and new HEAD. The other workspace stayed selected during the pull.
- Claude Code 2.1.226 accepted the local HTTP registration with environment-based authentication.
- Both temporary registrations were removed. Original app session was restored and one fresh app instance remained.
- Disposable repositories were moved to Trash. Fixture content named Claude Code was pulled by Codex, not Claude Code.

Verified after Claude Code login (2026-09-10):
- Claude Code 2.1.226 connected and called discovery, rejected a wrong branch, then pulled successfully.
- Claude Code tool-result evidence: `ab422085d4f2e960a4af7e29be1ad34e1475625a` -> `eb835d19f97f1dd5ac7851f57e262f1fda9cf683`.
- Codex CLI 0.153.2 repeated discovery, wrong-branch rejection, and pull: `eb835d19f97f1dd5ac7851f57e262f1fda9cf683` -> `217c388b0b6b0a49fbec51f31a6b7aa9e30afcc9`.
- Both clients rejected missing-token and wrong-token server configurations. Their authenticated configuration connected successfully.
- App restart released the listener. Both clients reconnected with unchanged registrations and the same token and URL.
- Claude Code reconnect pulled `217c388b0b6b0a49fbec51f31a6b7aa9e30afcc9` -> `76e59b981250a9b756f544698691b41551b60cf6`.
- Codex reconnect pulled `76e59b981250a9b756f544698691b41551b60cf6` -> `a7f209631e0b0a93ac9845c30524b39084db22db`.
- Native UI showed each client's changed text. Final visible HEAD and disk HEAD matched `a7f2096`.
- The non-selected workspace stayed selected during the first Claude Code pull.
- A delayed pull returned an explicit unknown-outcome error after 45.0 seconds, then completed and cleared busy state.
- Close/reopen during a delayed request changed `EntityId(11v3)` to `EntityId(8v5)`.
- The old request returned `Workspace closed or replaced`; the replacement stayed idle without accepting the old completion.
- Repeated `./scripts/test.sh`: 76 passed, 0 failed. Repeated `./scripts/build.sh`: passed with Rust 1.97.1.
- Restored the current user's original session, removed both QA registrations, and verified one running app instance.
- Moved disposable fixtures to Trash. No runtime source changes were needed during this verification.

Verification notes:
- The first Claude harness used `--tools ""`, which hid MCP tools. The corrected run produced real MCP tool results.
- The first reopen check finished before the picker reopened the root. A longer delay verified actual replacement before completion.
- Existing Codex MCP configuration required an isolated command-line MCP table for this CLI test; stored definitions stayed unchanged.
- Client logs are in the recoverable fixture folder `artifex-mcp-verify-d8w0w2xf` under the user's Trash.
- Commands and test harnesses remain at `/tmp/artifex-mcp-client-check.py`, `/tmp/artifex-mcp-timeout-check.py`, and `/tmp/artifex-mcp-reopen-check.py`.
- Build and test logs remain at `/tmp/artifex-mcp-build.log` and `/tmp/artifex-mcp-tests.log`.

Handoff:
- None. Implementation and required verification are complete. No Artifex commit or push was made.
