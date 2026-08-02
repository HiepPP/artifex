#!/bin/bash
# Builds the binary with the pinned toolchain and wraps it in the .app bundle.
#
# A Homebrew `rustc` earlier on PATH shadows the rustup toolchain even when cargo
# itself comes from rustup, because cargo resolves its `rustc` through PATH. The
# build then fails on `edition2024`. This script puts the pinned toolchain's bin
# directory first so the channel in `rust-toolchain.toml` is the one that runs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${1:-debug}"

if command -v rustup >/dev/null; then
    CHANNEL="$(awk -F'"' '/^channel/ {print $2}' "$ROOT/rust-toolchain.toml")"
    TOOLCHAIN_BIN="$(dirname "$(rustup which --toolchain "$CHANNEL" rustc)")"
    export PATH="$TOOLCHAIN_BIN:$PATH"
    export RUSTC="$TOOLCHAIN_BIN/rustc"
fi

echo "rustc: $(rustc --version)" >&2

case "$PROFILE" in
    debug) cargo build ;;
    release) cargo build --release ;;
    *) echo "unknown profile: $PROFILE (expected debug or release)" >&2; exit 1 ;;
esac

if [[ ! -f "$ROOT/assets/AppIcon.icns" ]]; then
    "$ROOT/scripts/make_icon.sh" >/dev/null
fi

"$ROOT/scripts/bundle.sh" "$PROFILE"
