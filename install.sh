#!/usr/bin/env bash
# Smooblue one-line installer for macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
#
# Pulls the latest release from GitHub, unzips Smooblue.app into
# /Applications (or ~/Applications if that's not writable), strips
# the macOS quarantine xattr so Gatekeeper doesn't bar first launch,
# and opens it. Idempotent: re-running upgrades in place.

set -euo pipefail

REPO="${SMOOBLUE_REPO_SLUG:-SmooAI/smooblue}"
ASSET="${SMOOBLUE_ASSET:-Smooblue-macos-arm64.zip}"

red()    { printf '\033[31m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
dim()    { printf '\033[2m%s\033[0m\n' "$*"; }

# ── Sanity checks ──────────────────────────────────────────────────
[[ "$(uname -s)" == "Darwin" ]] || {
    red "✗ smooblue install: macOS only (you're on $(uname -s))."
    red "  Linux build instructions: https://github.com/$REPO#linux-untested-but-probably-works"
    exit 1
}
ARCH=$(uname -m)
if [[ "$ARCH" != "arm64" ]]; then
    red "✗ smooblue install: only Apple Silicon (arm64) is published today."
    red "  You're on $ARCH. Build from source instead:"
    red "    git clone https://github.com/$REPO && cd smooblue && ./scripts/bundle-macos.sh"
    exit 1
fi
command -v curl >/dev/null || { red "✗ curl not found"; exit 1; }
command -v unzip >/dev/null || { red "✗ unzip not found"; exit 1; }

# ── Pick an install dir we can actually write to ──────────────────
INSTALL_DIR="${SMOOBLUE_INSTALL_DIR:-/Applications}"
if [[ ! -w "$INSTALL_DIR" ]]; then
    dim "  (/Applications isn't writable for this user; using ~/Applications instead)"
    INSTALL_DIR="$HOME/Applications"
    mkdir -p "$INSTALL_DIR"
fi
APP_PATH="$INSTALL_DIR/Smooblue.app"

# ── Discover the latest release asset URL ─────────────────────────
echo "▸ resolving latest release on $REPO …"
ASSET_URL=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep -oE "\"browser_download_url\": *\"[^\"]+$ASSET\"" \
    | head -n 1 \
    | sed -E 's/.*"(https[^"]+)".*/\1/')
if [[ -z "$ASSET_URL" ]]; then
    red "✗ couldn't find $ASSET on the latest release."
    red "  Browse releases: https://github.com/$REPO/releases"
    exit 1
fi
TAG=$(echo "$ASSET_URL" | sed -E 's|.*/download/([^/]+)/.*|\1|')
echo "  found $TAG → $ASSET"

# ── Download + unpack ─────────────────────────────────────────────
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
echo "▸ downloading $ASSET ($TAG)…"
curl -fsSL --progress-bar "$ASSET_URL" -o "$TMPDIR/$ASSET"
echo "▸ unzipping…"
unzip -q "$TMPDIR/$ASSET" -d "$TMPDIR"

if [[ ! -d "$TMPDIR/Smooblue.app" ]]; then
    red "✗ downloaded zip didn't contain Smooblue.app — bailing."
    exit 1
fi

# ── Replace previous install (if any) ─────────────────────────────
if [[ -d "$APP_PATH" ]]; then
    # Back up first so a SIGBUS-mid-install can't leave the user
    # with a broken bundle.
    BACKUP="${APP_PATH}.previous"
    rm -rf "$BACKUP"
    mv "$APP_PATH" "$BACKUP"
    UPGRADE=1
else
    UPGRADE=0
fi
cp -R "$TMPDIR/Smooblue.app" "$APP_PATH"

# Strip the quarantine xattr macOS adds to anything downloaded so
# Gatekeeper doesn't show the right-click-to-open dance.
xattr -dr com.apple.quarantine "$APP_PATH" 2>/dev/null || true

# Drop the backup once we've confirmed the new install landed.
[[ "$UPGRADE" -eq 1 ]] && rm -rf "${APP_PATH}.previous" 2>/dev/null || true

# ── Done ──────────────────────────────────────────────────────────
green "✓ installed Smooblue $TAG to $APP_PATH"
if [[ "$UPGRADE" -eq 1 ]]; then
    dim "  upgraded from a previous install"
fi
if [[ "${SMOOBLUE_NO_OPEN:-}" != "1" ]]; then
    open "$APP_PATH"
fi
