#!/bin/bash
# Runs the test suite with the pinned toolchain.
#
# Same reason as build.sh: a Homebrew `rustc` earlier on PATH shadows the rustup
# toolchain, and `cargo test` then fails on `edition2024`. `rustup run` does not
# fix it either, because cargo resolves its `rustc` through PATH rather than
# through RUSTUP_TOOLCHAIN. Setting both PATH and RUSTC is what works.
#
# Arguments are passed straight through, so a single test still works:
#   ./scripts/test.sh file_tree_expands_lazily
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if command -v rustup >/dev/null; then
    CHANNEL="$(awk -F'"' '/^channel/ {print $2}' "$ROOT/rust-toolchain.toml")"
    TOOLCHAIN_BIN="$(dirname "$(rustup which --toolchain "$CHANNEL" rustc)")"
    export PATH="$TOOLCHAIN_BIN:$PATH"
    export RUSTC="$TOOLCHAIN_BIN/rustc"
fi

cargo test "$@"
