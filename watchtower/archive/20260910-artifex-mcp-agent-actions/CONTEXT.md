# Plan Context

## Shared Context

- Agents should call the running Artifex app through MCP.
- First use case: an agent pushes finished work, then asks Artifex to pull that workspace's upstream.
- A pull cannot retrieve changes that exist only in an agent's local checkout.
- The scope contract now includes local MCP workspace discovery and guarded pull.
- Existing pull uses `--ff-only`, protects unsaved editors, and refreshes file views and Git state.

## Decisions

- Start with local access to the running app and explicit workspace targeting.
- Use Streamable HTTP on loopback with a stable endpoint and local bearer-token authentication.
- Support registration and real MCP calls from both Claude Code and Codex on the same machine.
- Initial tools cover workspace discovery and pull only.
- Reuse the app's Git operation guards and completion path.
- Do not add an agent panel, model calls, arbitrary shell execution, or automatic push.
- Keep unrelated changes and other scope exclusions intact.

## Open Decisions

- None. Select the Rust integration and port configuration during implementation within the agreed transport contract.

## References

- [DESIGN.md](../../../DESIGN.md): scope and Git behavior.
- [src/app/shell.rs](../../../src/app/shell.rs): pull action and completion handling.
- [src/app/workspace.rs](../../../src/app/workspace.rs): workspace identity, operation guards, and view reload.
- [src/services/git.rs](../../../src/services/git.rs): Git CLI pull operation.
