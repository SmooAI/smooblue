#!/usr/bin/env bash
# smooblue-update — pull latest main, rebuild release, reinstall the .app.
#
# Run on demand (`scripts/smooblue-update.sh`) or via the launchd job at
# `~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist` (hourly).
#
# Idempotent: if `git fetch` shows no new commits on `main`, exits early
# without rebuilding. Safe to call back-to-back.
#
# Logs to /tmp/smooblue-update.log so the launchd job's history is
# inspectable without sudo.
set -euo pipefail

REPO="${SMOOBLUE_REPO:-$HOME/dev/smooai/smooblue}"
INSTALL_PATH="${SMOOBLUE_INSTALL:-/Applications/Smooblue.app}"
LOG_FILE="${SMOOBLUE_LOG:-/tmp/smooblue-update.log}"

# Pipe all output through tee so the script's stdout AND the log file
# both see what's happening. Without this, launchd sees nothing.
exec > >(tee -a "$LOG_FILE") 2>&1
echo
echo "=== $(date -u +%FT%TZ) smooblue-update start ==="

cd "$REPO"

# Guard: only update when main is checked out + clean. We never want
# to clobber a feature branch the user is working on.
current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$current_branch" != "main" ]]; then
    echo "Skipping: branch is '$current_branch', not 'main'."
    exit 0
fi
if [[ -n "$(git status --porcelain)" ]]; then
    echo "Skipping: working tree has uncommitted changes."
    exit 0
fi

git fetch --quiet origin main
local_sha="$(git rev-parse HEAD)"
remote_sha="$(git rev-parse origin/main)"

if [[ "$local_sha" == "$remote_sha" ]]; then
    # No new commits — but if the installed bundle is older than the
    # current binary in target/release/, the installed copy is stale
    # (likely because a previous run failed mid-install). Reinstall.
    installed="$INSTALL_PATH/Contents/MacOS/Smooblue"
    # `cargo metadata` reports the real target dir, which on this
    # workspace is `~/.cargo/shared-target` not `./target`. Defaulting
    # to ./target/release misses fresh builds and leaves stale apps
    # installed.
    target_dir="$(cargo metadata --no-deps --format-version 1 2>/dev/null \
        | python3 -c 'import sys,json; print(json.load(sys.stdin)["target_directory"])' \
        2>/dev/null || echo "$REPO/target")"
    fresh="$target_dir/release/smooblue-app"
    bundled="$REPO/dist/Smooblue.app/Contents/MacOS/Smooblue"
    # Compare both candidates against the installed binary — whichever
    # exists and is newer wins.
    if [[ -f "$bundled" && "$bundled" -nt "$installed" ]] \
        || [[ -f "$fresh" && "$fresh" -nt "$installed" ]]; then
        echo "Repo unchanged but a fresher build exists — reinstalling."
    else
        echo "Already up to date at ${local_sha:0:12}."
        exit 0
    fi
else
    echo "New commits: ${local_sha:0:12} → ${remote_sha:0:12}"
    git pull --quiet --rebase
fi

# Source nvm/cargo from the shell rc so PATH has rustup/cargo. launchd's
# env is otherwise the bare login defaults.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

echo "Building release bundle…"
bash scripts/bundle-macos.sh

if [[ ! -d "dist/Smooblue.app" ]]; then
    echo "ERROR: bundle script didn't produce dist/Smooblue.app — aborting install."
    exit 1
fi

# Install: replace the previous .app atomically (rsync --delete cleans
# stale resources, e.g. old icons or removed files).
echo "Installing to $INSTALL_PATH…"
mkdir -p "$(dirname "$INSTALL_PATH")"
# If a previous install exists, move it aside first so a partial
# rsync can't leave the user with a broken bundle.
if [[ -d "$INSTALL_PATH" ]]; then
    backup="${INSTALL_PATH}.previous"
    rm -rf "$backup"
    mv "$INSTALL_PATH" "$backup"
fi
cp -R "dist/Smooblue.app" "$INSTALL_PATH"

# Clear the quarantine flag macOS adds to fresh apps so Gatekeeper
# doesn't bark on every launch.
xattr -dr com.apple.quarantine "$INSTALL_PATH" 2>/dev/null || true

# Best-effort: drop the backup once the new one is in place.
rm -rf "${INSTALL_PATH}.previous" 2>/dev/null || true

echo "Installed Smooblue ${remote_sha:0:12} to $INSTALL_PATH"
echo "=== smooblue-update done ==="
