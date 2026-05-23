#!/usr/bin/env bash
# Rasterizes assets/icon.svg into PNGs at all the sizes Dioxus/Tauri want
# for desktop bundling. Requires `rsvg-convert` (librsvg, `brew install librsvg`).
#
# Run from apps/smooblue/:
#   ./scripts/build-icons.sh
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v rsvg-convert >/dev/null; then
    echo "rsvg-convert not found. Install: brew install librsvg" >&2
    exit 1
fi

mkdir -p assets/icons

for size in 16 32 64 128 256 512 1024; do
    out="assets/icons/icon-${size}.png"
    rsvg-convert -w "$size" -h "$size" assets/icon.svg -o "$out"
    echo "wrote $out"
done

echo
echo "Done. assets/icons/ now contains $(ls assets/icons/*.png | wc -l | tr -d ' ') PNGs."
