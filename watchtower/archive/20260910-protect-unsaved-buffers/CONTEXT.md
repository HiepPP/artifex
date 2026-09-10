# Plan Context

## Decisions

- Limit scope to closing tabs, closing workspaces, and replacing preview tabs.
- Use the existing native prompt API and editor save method.
- Keep clean close behavior and the last-terminal/last-workspace rules.
- Do not add recovery, autosave, app-quit handling, dependencies, or unrelated refactors.

## References

- [Shell](../../../src/app/shell.rs)
- [Center](../../../src/app/center.rs)
- [Workspace](../../../src/app/workspace.rs)
- [Design](../../../DESIGN.md)
