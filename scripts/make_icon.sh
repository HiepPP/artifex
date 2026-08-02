#!/bin/bash
# Builds every macOS icon size from the single SVG master artwork.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAGICK="/opt/homebrew/bin/magick"
SVG="$ROOT/assets/icon.svg"
ICONSET="$ROOT/assets/AppIcon.iconset"
ICNS="$ROOT/assets/AppIcon.icns"

if [[ ! -x "$MAGICK" ]]; then
    echo "missing ImageMagick: $MAGICK" >&2
    exit 1
fi

mkdir -p "$ICONSET"

names=(
    icon_16x16.png
    icon_16x16@2x.png
    icon_32x32.png
    icon_32x32@2x.png
    icon_128x128.png
    icon_128x128@2x.png
    icon_256x256.png
    icon_256x256@2x.png
    icon_512x512.png
    icon_512x512@2x.png
)
sizes=(16 32 32 64 128 256 256 512 512 1024)

for index in "${!names[@]}"; do
    output="$ICONSET/${names[$index]}"
    size="${sizes[$index]}"
    "$MAGICK" -background none -density 384 "$SVG" -resize "${size}x${size}" \
        -depth 8 -define png:color-type=6 "$output"

    if [[ ! -s "$output" ]]; then
        echo "empty icon output: $output" >&2
        exit 1
    fi

    width="$(sips -g pixelWidth "$output" | awk '/pixelWidth/ {print $2}')"
    height="$(sips -g pixelHeight "$output" | awk '/pixelHeight/ {print $2}')"
    if [[ "$width" != "$size" || "$height" != "$size" ]]; then
        echo "wrong icon size: $output is ${width}x${height}, expected ${size}x${size}" >&2
        exit 1
    fi
done

iconutil -c icns "$ICONSET" -o "$ICNS"
echo "$ICNS"
