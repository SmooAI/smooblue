#!/usr/bin/env bash
# Build Smooblue.app — a distributable macOS .app bundle.
#
# Layout produced (Apple's standard):
#   dist/Smooblue.app/
#   ├── Contents/
#   │   ├── Info.plist
#   │   ├── MacOS/
#   │   │   └── Smooblue           ← the release binary
#   │   └── Resources/
#   │       └── Icon.icns          ← multi-resolution app icon
#
# Usage:
#   scripts/bundle-macos.sh                # release build + bundle
#   scripts/bundle-macos.sh --skip-build   # bundle the existing target/release binary
#
# Output: dist/Smooblue.app — drag to /Applications.
#
# Notes:
#   - Adhoc-signed only (no Apple Developer cert). First-run Gatekeeper
#     prompts the user to right-click → Open.
#   - DMG packaging is a follow-up — for now we ship the .app folder
#     directly (or `zip -ry Smooblue.app.zip Smooblue.app`).
#   - Universal (x86_64 + arm64) builds: pass --universal to cargo
#     manually and `lipo` into a fat binary; this script targets the
#     current host arch only by default.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist"
APP_NAME="Smooblue"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCES_DIR="$CONTENTS/Resources"
ICON_SRC="$REPO_ROOT/assets/icon.svg"
ICON_DST="$RESOURCES_DIR/Icon.icns"
BUNDLE_ID="ai.smoo.smooblue"
# Cargo's release binary lives in the workspace target dir.
# Resolve it via cargo metadata so we work regardless of CARGO_TARGET_DIR.
METADATA="$(cargo metadata --no-deps --format-version=1 --manifest-path "$REPO_ROOT/Cargo.toml")"
TARGET_DIR="$(printf '%s' "$METADATA" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
# The app version — CFBundleShortVersionString AND CFBundleVersion.
# Sparkle compares CFBundleVersion against the appcast's
# sparkle:version, so this MUST be the real, monotonically increasing
# release version (it was a hardcoded "0.1.0", which would have made
# every installed build look out of date forever).
VERSION="$(printf '%s' "$METADATA" | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "smooblue-app"))')"

# ── Sparkle 2 (OTA updates) ────────────────────────────────────────
# Same version SmoothFlow pins. The release tarball carries both the
# framework we embed and the sign_update / generate_keys tools the
# release workflow uses, so the two never drift. Cached per machine;
# the sha256 pin means a swapped tarball fails the build instead of
# shipping. SMOOBLUE_NO_SPARKLE=1 builds without it (the app then
# falls back to the GitHub "update available" toast).
SPARKLE_VERSION="2.9.6"
SPARKLE_SHA256="52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192"
SPARKLE_CACHE="${SMOOBLUE_SPARKLE_CACHE:-$HOME/.cache/smooblue}"
SPARKLE_DIR="$SPARKLE_CACHE/sparkle-$SPARKLE_VERSION"
# Feed + public key. The private half of SUPublicEDKey is the
# SMOOBLUE_SPARKLE_PRIVATE_KEY Actions secret (and the "Smooblue"
# account in the release manager's login keychain — generate_keys
# --account Smooblue). Losing it means no installed build can ever
# take another update; see docs/Operations/Sparkle-Updates.md.
SPARKLE_FEED_URL="https://github.com/SmooAI/smooblue/releases/latest/download/appcast.xml"
SPARKLE_PUBLIC_KEY="u2Xv8BPY6WG6l90PQIaWkMtmVkIlNlQ1LKTBltV1JkU="

ensure_sparkle() {
    if [ -d "$SPARKLE_DIR/Sparkle.framework" ]; then
        return
    fi
    mkdir -p "$SPARKLE_CACHE"
    local tarball="$SPARKLE_CACHE/Sparkle-$SPARKLE_VERSION.tar.xz"
    echo "▸ fetching Sparkle $SPARKLE_VERSION"
    curl -fsSL -o "$tarball" \
        "https://github.com/sparkle-project/Sparkle/releases/download/$SPARKLE_VERSION/Sparkle-$SPARKLE_VERSION.tar.xz"
    local got
    got="$(shasum -a 256 "$tarball" | awk '{print $1}')"
    if [ "$got" != "$SPARKLE_SHA256" ]; then
        echo "error: Sparkle tarball sha256 mismatch (got $got, want $SPARKLE_SHA256)" >&2
        rm -f "$tarball"
        exit 1
    fi
    rm -rf "$SPARKLE_DIR.tmp"
    mkdir -p "$SPARKLE_DIR.tmp"
    tar -xf "$tarball" -C "$SPARKLE_DIR.tmp"
    mv "$SPARKLE_DIR.tmp" "$SPARKLE_DIR"
}
BIN_PATH="$TARGET_DIR/release/smooblue-app"

skip_build=0
for arg in "$@"; do
    case "$arg" in
        --skip-build) skip_build=1 ;;
        *) echo "unknown flag: $arg" >&2; exit 1 ;;
    esac
done

if [ "$skip_build" -eq 0 ]; then
    echo "▸ cargo build --release -p smooblue-app"
    cargo build --release -p smooblue-app --manifest-path "$REPO_ROOT/Cargo.toml"
fi

if [ ! -x "$BIN_PATH" ]; then
    echo "error: release binary not found at $BIN_PATH (did the build succeed?)" >&2
    exit 1
fi

echo "▸ staging $APP_BUNDLE"
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

# ── Icon: SVG → multi-resolution .icns via sips + iconutil ─────────
# Apple wants a .iconset directory with specific filenames at each
# resolution (16, 32, 64, 128, 256, 512, 1024 — both @1x and @2x).
# rsvg-convert is the cleanest SVG rasterizer; fall back to sips which
# accepts SVG on recent macOS but with worse anti-aliasing.
ICONSET="$RESOURCES_DIR/Smooblue.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"

rasterize() {
    local size="$1" out="$2"
    if command -v rsvg-convert >/dev/null 2>&1; then
        rsvg-convert -w "$size" -h "$size" "$ICON_SRC" -o "$out"
    else
        # sips can rasterize SVG only via PNG → PNG, so go through a
        # high-res intermediate. Quality is acceptable for app icons.
        sips -s format png -z "$size" "$size" "$ICON_SRC" --out "$out" >/dev/null
    fi
}

for entry in \
    "16:icon_16x16.png" \
    "32:icon_16x16@2x.png" \
    "32:icon_32x32.png" \
    "64:icon_32x32@2x.png" \
    "128:icon_128x128.png" \
    "256:icon_128x128@2x.png" \
    "256:icon_256x256.png" \
    "512:icon_256x256@2x.png" \
    "512:icon_512x512.png" \
    "1024:icon_512x512@2x.png"; do
    size="${entry%%:*}"
    name="${entry##*:}"
    rasterize "$size" "$ICONSET/$name"
done

iconutil -c icns "$ICONSET" -o "$ICON_DST"
rm -rf "$ICONSET"

# ── Binary ─────────────────────────────────────────────────────────
echo "▸ copying binary"
cp "$BIN_PATH" "$MACOS_DIR/$APP_NAME"
chmod +x "$MACOS_DIR/$APP_NAME"

# ── Sparkle.framework ──────────────────────────────────────────────
# Loaded at runtime by crates/smooblue-app/src/sparkle.rs from
# Contents/Frameworks — nothing links against it. ditto (not cp -R)
# so the framework's Versions/Current symlinks survive intact;
# flattening them breaks its code signature.
SPARKLE_PLIST=""
if [ "${SMOOBLUE_NO_SPARKLE:-0}" != "1" ]; then
    ensure_sparkle
    echo "▸ embedding Sparkle $SPARKLE_VERSION"
    mkdir -p "$CONTENTS/Frameworks"
    ditto "$SPARKLE_DIR/Sparkle.framework" "$CONTENTS/Frameworks/Sparkle.framework"
    SPARKLE_PLIST="    <key>SUFeedURL</key>
    <string>$SPARKLE_FEED_URL</string>
    <key>SUPublicEDKey</key>
    <string>$SPARKLE_PUBLIC_KEY</string>
    <key>SUEnableAutomaticChecks</key>
    <true/>
    <key>SUScheduledCheckInterval</key>
    <integer>3600</integer>"
fi

# ── Info.plist ─────────────────────────────────────────────────────
# Highlights:
#   - LSMinimumSystemVersion 11.0 matches what Dioxus desktop needs
#     (uses recent WebKit APIs).
#   - NSHighResolutionCapable + NSPrincipalClass: retina + standard
#     event loop.
#   - NSAppTransportSecurity: we hit https://api.bsky.app, https://
#     {pds}.bsky.app, etc — all TLS so no ATS exceptions needed.
echo "▸ writing Info.plist"
cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
 "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleIconFile</key>
    <string>Icon</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSHumanReadableCopyright</key>
    <string>© 2026 Smoo AI · MIT licensed</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.social-networking</string>
$SPARKLE_PLIST
</dict>
</plist>
PLIST

# ── Adhoc codesign ────────────────────────────────────────────────
# Lets the binary run on the build machine without TCC complaining
# about an unsigned bundle. Release builds are then re-signed with the
# Developer ID, notarized and stapled by scripts/sign-and-notarize-macos.sh
# (release.yml does this in CI).
echo "▸ adhoc codesign (v$VERSION)"
codesign --force --deep --sign - "$APP_BUNDLE" 2>&1 | sed 's/^/  /' || true

# ── Summary ────────────────────────────────────────────────────────
size_mb=$(du -sh "$APP_BUNDLE" | awk '{print $1}')
echo ""
echo "✓ Built $APP_BUNDLE ($size_mb)"
echo "   Drag it to /Applications, or:"
echo "     open \"$APP_BUNDLE\""
echo "     ditto -c -k --sequesterRsrc --keepParent \"$APP_BUNDLE\" \"$DIST_DIR/${APP_NAME}.zip\""
