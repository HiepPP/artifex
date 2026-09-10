# Learn 20260910-protect-unsaved-buffers

## Summary

Discrepancy: none. The change matches the three requested buffer-protection paths.

## Per TASK

- TASK-001: match. Shared native close prompts protect tabs and workspaces. Dirty previews stay open.
- Focused native checks covered Save, Discard, Cancel, Escape, save failure, and preview replacement.

## Plan-Level

- none. No dependency, recovery feature, or unrelated refactor was added.

## Lessons

- Resolve asynchronous close targets by workspace entity and tab ID.
- A failed multi-file Save keeps the workspace open, though earlier files may already be saved.
- Wait for native UI state before asserting results. Telex can transform typed test markers.
