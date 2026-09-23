#!/usr/bin/env bash
# Developer ID-sign, notarize and staple dist/Smooblue.app, then zip it
# for release + Sparkle. Run after scripts/bundle-macos.sh. Same shape
# as SmoothFlow's build-release.sh (smooth repo) — see
# docs/Operations/Sparkle-Updates.md.
#
#   1. Sign INSIDE-OUT, never --deep: Sparkle's XPC services, Autoupdate
#      and Updater.app, then the framework, then the app. --deep re-signs
#      nested code with the outer options and breaks Sparkle's helpers.
#   2. Notarize the .app (zipped for submission) and STAPLE THE .app.
#      Sparkle installs the app, not whatever archive carried it, so the
#      ticket has to live in the .app or Gatekeeper phones home on first
#      launch (SmoothFlow shipped three releases with the ticket on the
#      DMG only — th-9c3f4e).
#   3. Re-zip the stapled app → dist/Smooblue-macos-arm64.zip: the
#      release asset, the Homebrew cask download and the Sparkle
#      enclosure are all this one file.
#
# Signing identity (first match wins):
#   SIGN_IDENTITY            e.g. "Developer ID Application: Smoo LLC (DTX9733844)"
#   (else) the first "Developer ID Application" identity in the keychain
#
# Notary auth (first match wins; SKIP_NOTARIZE=1 signs only):
#   NOTARY_KEY + NOTARY_KEY_ID + NOTARY_ISSUER   App Store Connect API key (CI)
#   NOTARY_PROFILE                               notarytool keychain profile
#
# Usage:
#   scripts/bundle-macos.sh && scripts/sign-and-notarize-macos.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_BUNDLE="${APP_BUNDLE:-$REPO_ROOT/dist/Smooblue.app}"
DIST_DIR="$REPO_ROOT/dist"
ZIP_OUT="${ZIP_OUT:-$DIST_DIR/Smooblue-macos-arm64.zip}"
ENTITLEMENTS="$REPO_ROOT/scripts/entitlements-macos.plist"

if [ ! -d "$APP_BUNDLE" ]; then
    echo "error: $APP_BUNDLE not found. Run scripts/bundle-macos.sh first." >&2
    exit 1
fi

if [ -z "${SIGN_IDENTITY:-}" ]; then
    SIGN_IDENTITY="$(security find-identity -v -p codesigning \
        | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -n1)"
fi
if [ -z "$SIGN_IDENTITY" ]; then
    echo "error: no Developer ID Application identity (set SIGN_IDENTITY)." >&2
    exit 1
fi
echo "▸ signing as: $SIGN_IDENTITY"

sign() {
    codesign --force --timestamp --options runtime --sign "$SIGN_IDENTITY" "$@"
}

# ── 1. Inside-out signing ──────────────────────────────────────────
FW="$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"
if [ -d "$FW" ]; then
    echo "▸ signing Sparkle.framework (inside-out)"
    V="$FW/Versions/B"
    # Order and flags follow Sparkle's "Code signing" docs. Downloader
    # keeps its own entitlements (it needs network client access).
    sign "$V/XPCServices/Installer.xpc"
    sign --preserve-metadata=entitlements "$V/XPCServices/Downloader.xpc"
    sign "$V/Autoupdate"
    sign "$V/Updater.app"
    sign "$FW"
fi

echo "▸ signing Smooblue.app"
sign --entitlements "$ENTITLEMENTS" "$APP_BUNDLE"

echo "▸ verifying signature"
codesign --verify --strict --deep --verbose=2 "$APP_BUNDLE"

# ── 2. Notarize + staple the .app ──────────────────────────────────
notary_args=()
if [ -n "${NOTARY_KEY:-}" ]; then
    : "${NOTARY_KEY_ID:?NOTARY_KEY_ID required with NOTARY_KEY}"
    : "${NOTARY_ISSUER:?NOTARY_ISSUER required with NOTARY_KEY}"
    notary_args=(--key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
elif [ -n "${NOTARY_PROFILE:-}" ]; then
    notary_args=(--keychain-profile "$NOTARY_PROFILE")
fi

if [ "${SKIP_NOTARIZE:-0}" = "1" ]; then
    echo "▸ SKIP_NOTARIZE=1 — signed only (Gatekeeper will still query Apple on first launch)"
elif [ ${#notary_args[@]} -eq 0 ]; then
    echo "error: no notary credentials (NOTARY_KEY/NOTARY_KEY_ID/NOTARY_ISSUER or NOTARY_PROFILE)," >&2
    echo "       or set SKIP_NOTARIZE=1 to sign only." >&2
    exit 1
else
    SUBMIT_ZIP="$DIST_DIR/Smooblue-notary-submit.zip"
    rm -f "$SUBMIT_ZIP"
    ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$SUBMIT_ZIP"
    echo "▸ submitting to Apple notary (usually 1–10 min)"
    xcrun notarytool submit "$SUBMIT_ZIP" "${notary_args[@]}" --wait --timeout 30m \
        | tee "$DIST_DIR/notary-submit.log"
    rm -f "$SUBMIT_ZIP"
    if ! grep -q "status: Accepted" "$DIST_DIR/notary-submit.log"; then
        id="$(sed -n 's/^ *id: \(.*\)$/\1/p' "$DIST_DIR/notary-submit.log" | head -n1)"
        [ -n "$id" ] && xcrun notarytool log "$id" "${notary_args[@]}" || true
        echo "error: notarization was not accepted" >&2
        exit 1
    fi
    echo "▸ stapling the .app"
    xcrun stapler staple "$APP_BUNDLE"
    xcrun stapler validate "$APP_BUNDLE"
    echo "▸ Gatekeeper assessment"
    spctl --assess --type execute --verbose=2 "$APP_BUNDLE"
fi

# ── 3. Release / Sparkle archive ───────────────────────────────────
rm -f "$ZIP_OUT"
ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$ZIP_OUT"
echo ""
echo "✓ Signed$([ "${SKIP_NOTARIZE:-0}" = "1" ] || echo ", notarized and stapled"): $APP_BUNDLE"
echo "✓ Archive: $ZIP_OUT"
