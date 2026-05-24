#!/usr/bin/env bash
# Code-sign + notarize Smooblue.app for distribution outside the
# App Store. Skeleton wired for the Developer ID + notarytool flow —
# enrol in the Apple Developer Program first, then run this.
#
# Required env (all from Apple Developer setup):
#   APPLE_DEV_ID_CERT   — cert name as it appears in Keychain, e.g.
#                         "Developer ID Application: Smoo AI Inc (XXXXXXXXXX)"
#                         (find with: security find-identity -p codesigning -v)
#   APPLE_NOTARY_PROFILE — keychain profile created via:
#                         xcrun notarytool store-credentials APPLE_NOTARY_PROFILE \
#                             --apple-id "you@smoo.ai" \
#                             --team-id "XXXXXXXXXX" \
#                             --password "<app-specific-password>"
#
# Optional env:
#   APP_BUNDLE  — path to .app to sign (default: dist/Smooblue.app)
#   MAKE_DMG    — set =1 to also produce a notarized .dmg
#
# Usage:
#   APPLE_DEV_ID_CERT="…" APPLE_NOTARY_PROFILE="smoo-notary" \
#       scripts/sign-and-notarize-macos.sh
#
# This is structurally the same flow Apple documents:
#   sign → submit to notarytool → wait → staple → verify
# Stapling embeds the notarization ticket in the bundle so Gatekeeper
# sees it offline (no first-launch network requirement for the user).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_BUNDLE="${APP_BUNDLE:-$REPO_ROOT/dist/Smooblue.app}"
DIST_DIR="$REPO_ROOT/dist"

if [ -z "${APPLE_DEV_ID_CERT:-}" ]; then
    cat >&2 <<EOF
error: APPLE_DEV_ID_CERT not set.

To list available signing identities:
    security find-identity -p codesigning -v

You should see something like:
    1) ABC123…  "Developer ID Application: Smoo AI Inc (XXXXXXXXXX)"

Then re-run with:
    APPLE_DEV_ID_CERT="Developer ID Application: Smoo AI Inc (XXXXXXXXXX)" \\
        APPLE_NOTARY_PROFILE="smoo-notary" \\
        scripts/sign-and-notarize-macos.sh
EOF
    exit 1
fi

if [ -z "${APPLE_NOTARY_PROFILE:-}" ]; then
    cat >&2 <<EOF
error: APPLE_NOTARY_PROFILE not set.

One-time setup (stores credentials in the macOS keychain):
    xcrun notarytool store-credentials smoo-notary \\
        --apple-id "you@smoo.ai" \\
        --team-id "XXXXXXXXXX" \\
        --password "<app-specific-password from appleid.apple.com>"

Then re-run with APPLE_NOTARY_PROFILE=smoo-notary.
EOF
    exit 1
fi

if [ ! -d "$APP_BUNDLE" ]; then
    echo "error: $APP_BUNDLE not found. Run scripts/bundle-macos.sh first." >&2
    exit 1
fi

ENTITLEMENTS="$REPO_ROOT/scripts/entitlements-macos.plist"
# Hardened runtime entitlements — Apple requires this for notarization.
# We only need the bare minimum: allow JIT for WebKit (Dioxus desktop
# embeds wry which uses WKWebView, which uses JIT internally) plus
# unsigned-executable-memory for the same reason.
if [ ! -f "$ENTITLEMENTS" ]; then
    cat > "$ENTITLEMENTS" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
 "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.allow-jit</key>
    <true/>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.network.client</key>
    <true/>
</dict>
</plist>
PLIST
fi

echo "▸ codesign --deep --force --options runtime"
codesign --deep --force --options runtime \
    --entitlements "$ENTITLEMENTS" \
    --sign "$APPLE_DEV_ID_CERT" \
    --timestamp \
    "$APP_BUNDLE"

echo "▸ verifying signature"
codesign --verify --verbose=4 "$APP_BUNDLE"
spctl --assess --type execute --verbose=4 "$APP_BUNDLE" 2>&1 | sed 's/^/  /' || {
    echo "  (spctl rejected — expected pre-notarization, fine for now)"
}

# notarytool wants a .zip OR a .pkg/.dmg, not a raw .app.
ZIP_PATH="$DIST_DIR/Smooblue-notary.zip"
echo "▸ zipping for notary submission → $ZIP_PATH"
rm -f "$ZIP_PATH"
ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$ZIP_PATH"

echo "▸ submitting to Apple notary (this can take 2–15 min)"
xcrun notarytool submit "$ZIP_PATH" \
    --keychain-profile "$APPLE_NOTARY_PROFILE" \
    --wait

echo "▸ stapling ticket to the bundle"
xcrun stapler staple "$APP_BUNDLE"
xcrun stapler validate "$APP_BUNDLE"

echo "▸ final spctl check (should pass now)"
spctl --assess --type execute --verbose=4 "$APP_BUNDLE" 2>&1 | sed 's/^/  /'

# Re-zip the now-stapled bundle for distribution.
DIST_ZIP="$DIST_DIR/Smooblue.zip"
rm -f "$DIST_ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$DIST_ZIP"
rm -f "$ZIP_PATH"

if [ "${MAKE_DMG:-0}" = "1" ]; then
    DMG_PATH="$DIST_DIR/Smooblue.dmg"
    echo "▸ creating DMG → $DMG_PATH"
    rm -f "$DMG_PATH"
    hdiutil create -volname "Smooblue" -srcfolder "$APP_BUNDLE" \
        -ov -format UDZO "$DMG_PATH"
    # DMGs need their own codesign + notary round.
    codesign --force --sign "$APPLE_DEV_ID_CERT" --timestamp "$DMG_PATH"
    xcrun notarytool submit "$DMG_PATH" \
        --keychain-profile "$APPLE_NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG_PATH"
    echo "✓ DMG: $DMG_PATH"
fi

echo ""
echo "✓ Notarized: $APP_BUNDLE"
echo "✓ Ship: $DIST_ZIP"
