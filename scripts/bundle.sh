#!/bin/bash
# Wraps the POC binary in a minimal .app bundle.
#
# GPUI needs a real bundle on macOS for the app to activate, own a menu bar and
# receive input-method events. An unbundled binary cannot pass gate 1.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="${1:-debug}"
BIN="$ROOT/target/$PROFILE/artifex"
APP="$ROOT/dist/Artifex.app"
ICON="$ROOT/assets/AppIcon.icns"

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
    <key>CFBundleName</key><string>Artifex</string>
    <key>CFBundleDisplayName</key><string>Artifex</string>
    <key>CFBundleExecutable</key><string>artifex</string>
    <key>CFBundleIdentifier</key><string>com.artifex.app</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
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

cp -f "$BIN" "$APP/Contents/MacOS/artifex"
if [[ -f "$ICON" ]]; then
    cp -f "$ICON" "$APP/Contents/Resources/AppIcon.icns"
else
    echo "warning: app icon not found: $ICON" >&2
fi
codesign --force --sign - --timestamp=none "$APP" >/dev/null 2>&1 || true

echo "$APP"
