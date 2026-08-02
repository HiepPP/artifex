#!/bin/bash
# Wraps the POC binary in a minimal .app bundle.
#
# GPUI needs a real bundle on macOS for the app to activate, own a menu bar and
# receive input-method events. An unbundled binary cannot pass gate 1.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${1:-debug}"
BIN="$ROOT/target/$PROFILE/rustelier"
APP="$ROOT/dist/Rustelier.app"

if [[ ! -x "$BIN" ]]; then
    echo "missing binary: $BIN" >&2
    exit 1
fi

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Rustelier</string>
    <key>CFBundleDisplayName</key><string>Rustelier</string>
    <key>CFBundleExecutable</key><string>rustelier</string>
    <key>CFBundleIdentifier</key><string>com.rustelier.app</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

cp -f "$BIN" "$APP/Contents/MacOS/rustelier"
codesign --force --sign - --timestamp=none "$APP" >/dev/null 2>&1 || true

echo "$APP"
