#!/usr/bin/env bash
# Smooblue one-line installer (macOS + Linux).
#
#   curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
#
# macOS:  pulls Smooblue-macos-arm64.zip, installs Smooblue.app into
#         /Applications (or ~/Applications fallback), opens it.
# Linux:  pulls Smooblue-linux-x86_64.tar.gz, installs the binary into
#         ~/.local/bin/smooblue, drops a .desktop file in
#         ~/.local/share/applications, and prints the apt prereq line.
#
# Idempotent — re-running upgrades in place.
# Env knobs:
#   SMOOBLUE_NO_OPEN=1       install without launching
#   SMOOBLUE_INSTALL_DIR=…   override target dir (macOS only)
#   SMOOBLUE_REPO_SLUG=…     install from a fork

set -euo pipefail

REPO="${SMOOBLUE_REPO_SLUG:-SmooAI/smooblue}"

red()    { printf '\033[31m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
dim()    { printf '\033[2m%s\033[0m\n' "$*"; }

command -v curl >/dev/null || { red "✗ curl not found"; exit 1; }

# ── Platform / arch detection picks the right release asset ───────
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS-$ARCH" in
    Darwin-arm64)
        ASSET="Smooblue-macos-arm64.zip"
        PLATFORM="macos"
        ;;
    Darwin-x86_64)
        red "✗ smooblue install: only Apple Silicon (arm64) macOS is published today."
        red "  You're on x86_64. Build from source instead:"
        red "    git clone https://github.com/$REPO && cd smooblue && ./scripts/bundle-macos.sh"
        exit 1
        ;;
    Linux-x86_64)
        ASSET="Smooblue-linux-x86_64.tar.gz"
        PLATFORM="linux"
        ;;
    Linux-aarch64|Linux-arm64)
        red "✗ smooblue install: Linux x86_64 only today (no arm64 build yet)."
        red "  Build from source: git clone https://github.com/$REPO && cd smooblue && cargo build --release"
        exit 1
        ;;
    *)
        red "✗ smooblue install: unsupported platform $OS-$ARCH"
        red "  Supported: Darwin-arm64, Linux-x86_64"
        red "  Linux build instructions: https://github.com/$REPO#linux-untested-but-probably-works"
        exit 1
        ;;
esac

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

# ── Download ──────────────────────────────────────────────────────
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
echo "▸ downloading $ASSET ($TAG)…"
curl -fsSL --progress-bar "$ASSET_URL" -o "$TMPDIR/$ASSET"

# ── Per-platform install ──────────────────────────────────────────
if [[ "$PLATFORM" == "macos" ]]; then
    command -v unzip >/dev/null || { red "✗ unzip not found"; exit 1; }
    INSTALL_DIR="${SMOOBLUE_INSTALL_DIR:-/Applications}"
    if [[ ! -w "$INSTALL_DIR" ]]; then
        dim "  (/Applications isn't writable; using ~/Applications instead)"
        INSTALL_DIR="$HOME/Applications"
        mkdir -p "$INSTALL_DIR"
    fi
    APP_PATH="$INSTALL_DIR/Smooblue.app"

    echo "▸ unzipping…"
    unzip -q "$TMPDIR/$ASSET" -d "$TMPDIR"
    if [[ ! -d "$TMPDIR/Smooblue.app" ]]; then
        red "✗ downloaded zip didn't contain Smooblue.app — bailing."
        exit 1
    fi

    # Atomic-ish replace: move old aside, copy new, drop backup.
    if [[ -d "$APP_PATH" ]]; then
        BACKUP="${APP_PATH}.previous"
        rm -rf "$BACKUP"
        mv "$APP_PATH" "$BACKUP"
        UPGRADE=1
    else
        UPGRADE=0
    fi
    cp -R "$TMPDIR/Smooblue.app" "$APP_PATH"
    xattr -dr com.apple.quarantine "$APP_PATH" 2>/dev/null || true
    [[ "$UPGRADE" -eq 1 ]] && rm -rf "${APP_PATH}.previous" 2>/dev/null || true

    green "✓ installed Smooblue $TAG to $APP_PATH"
    [[ "$UPGRADE" -eq 1 ]] && dim "  upgraded from a previous install"
    if [[ "${SMOOBLUE_NO_OPEN:-}" != "1" ]]; then
        open "$APP_PATH"
    fi

elif [[ "$PLATFORM" == "linux" ]]; then
    BIN_DIR="${SMOOBLUE_INSTALL_DIR:-$HOME/.local/bin}"
    APP_DIR="$HOME/.local/share/applications"
    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
    mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"

    echo "▸ extracting…"
    tar -xzf "$TMPDIR/$ASSET" -C "$TMPDIR"
    STAGE="$TMPDIR/Smooblue-linux-x86_64"
    [[ -x "$STAGE/smooblue" ]] || { red "✗ tarball didn't contain a smooblue binary — bailing."; exit 1; }

    install -m755 "$STAGE/smooblue" "$BIN_DIR/smooblue"
    install -m644 "$STAGE/smooblue.png" "$ICON_DIR/smooblue.png"

    # Drop a .desktop entry so the app shows up in the launcher /
    # activity overview / KRunner / etc.
    cat > "$APP_DIR/smooblue.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Smooblue
Comment=Native multi-column Bluesky client
Exec=$BIN_DIR/smooblue
Icon=smooblue
Categories=Network;InstantMessaging;
Terminal=false
StartupNotify=true
EOF
    # Best-effort: refresh the desktop database so launchers pick
    # up the new entry without a logout/login cycle.
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi

    green "✓ installed Smooblue $TAG → $BIN_DIR/smooblue"
    dim "  + desktop entry: $APP_DIR/smooblue.desktop"
    if ! echo ":$PATH:" | grep -q ":$BIN_DIR:"; then
        dim "  ⚠ $BIN_DIR isn't on your PATH — add it to your shell rc:"
        dim "    export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
    echo
    dim "  Runtime prerequisites (Debian/Ubuntu — install once):"
    dim "    sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 \\"
    dim "                     libayatana-appindicator3-1 librsvg2-2"
    dim "  Other distros: install the equivalents of webkit2gtk-4.1, gtk3,"
    dim "  libayatana-appindicator, librsvg."

    if [[ "${SMOOBLUE_NO_OPEN:-}" != "1" ]]; then
        echo
        echo "▸ launching smooblue…"
        # Disown so curl-piped-to-bash returns immediately.
        nohup "$BIN_DIR/smooblue" >/dev/null 2>&1 &
        disown 2>/dev/null || true
    fi
fi
