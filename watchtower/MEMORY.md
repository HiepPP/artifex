# Watchtower Memory

## Core Intent

- Keep Artifex changes small and simple. Follow the current code pattern.

## Planning Rules

- Reuse existing helpers before adding abstractions.
- Add no speculative tools, dependencies, or unrelated refactors.

## Source Anchors

- [Design contract](../DESIGN.md): define behavior before changing it.
- [MCP server](../src/services/mcp.rs): extend the existing action and response pattern.

## Verification Bias

- Use repository build and test scripts.
- Verify changed app behavior in the rebuilt native bundle.
