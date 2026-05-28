#!/usr/bin/env bash
# Cut a release end-to-end. From a clean main with pending
# changesets in .changeset/*.md, this:
#
#   1. Consumes changesets   → bumps package.json + Cargo.toml + writes CHANGELOG.md
#   2. Commits + pushes      → "Release vX.Y.Z" on main
#   3. Tags + pushes the tag → CI builds + uploads + bumps the brew tap
#
# Idempotent and safe to re-run:
# - No pending changesets       → exits 0 with nothing-to-do.
# - Tag already exists           → exits 0 with nothing-to-do.
# - Working tree dirty           → refuses to start (would otherwise
#                                  drag unrelated changes into the
#                                  Release commit).
# - Not on main                  → warns + continues (hotfix branches).
#
# Requires GITHUB_TOKEN in env (or `gh auth token` callable) so the
# changeset version step can resolve PR titles for the
# @changesets/changelog-github plugin.
set -euo pipefail

cd "$(dirname "$0")/.."

# ── 0. preflight ──────────────────────────────────────────────────
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "✗ working tree is dirty — commit or stash before releasing"
    echo "  (so the 'Release vX.Y.Z' commit only contains the version bump)"
    git status --short
    exit 1
fi

BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$BRANCH" != "main" ]]; then
    echo "⚠ on branch '$BRANCH' — releases typically land on main. Continuing anyway."
fi

# Make sure GITHUB_TOKEN is available for the changelog-github plugin.
# Fall back to `gh auth token` so a manual export isn't required for
# the common case of a developer who's already gh-authed.
if [[ -z "${GITHUB_TOKEN:-}" ]]; then
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        export GITHUB_TOKEN="$(gh auth token)"
    else
        echo "✗ GITHUB_TOKEN not set and gh not authed — needed for the"
        echo "  @changesets/changelog-github plugin to resolve PR titles"
        exit 1
    fi
fi

# Pull first so we don't try to push a "Release" commit on top of
# remote changes we don't have yet.
echo "▸ git pull --rebase"
git pull --rebase --autostash

# ── 1. consume changesets ─────────────────────────────────────────
# A pending changeset is any .md file in .changeset/ other than the
# README (the README is the only .md the tooling installs). config.json
# is JSON so it's excluded by the glob.
PENDING=$(find .changeset -maxdepth 1 -name "*.md" -not -name "README.md" | wc -l | tr -d ' ')
if [[ "$PENDING" -eq 0 ]]; then
    echo "✓ no pending changesets — nothing to release"
    echo "  (drop one with \`pnpm changeset\` first)"
    exit 0
fi
echo "▸ consuming $PENDING pending changeset(s)…"
pnpm run version

# `pnpm run version` stages Cargo.toml + Cargo.lock but leaves
# package.json / CHANGELOG.md / .changeset/*.md unstaged. Catch all of
# them with a final `git add -A` so the Release commit is one tidy
# commit even when changeset adds new files we haven't seen before.
git add -A

VERSION=$(node -p "require('./package.json').version")
TAG="v$VERSION"
echo "  → bumped to $VERSION"

# ── 2. commit + push the version bump ─────────────────────────────
echo "▸ committing Release $TAG"
git commit -m "Release $TAG"
echo "▸ pushing branch"
git push origin "$BRANCH"

# ── 3. tag + push the tag ─────────────────────────────────────────
# Re-check the tag here even though we did it implicitly above — in
# the (unlikely) race where someone else tagged this version between
# our pull and our push, we'd rather no-op than error.
if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "✓ $TAG already exists — skipping tag push"
    exit 0
fi
echo "▸ tagging $TAG on $(git rev-parse --short HEAD)"
git tag -a "$TAG" -m "Release $TAG"
git push origin "$TAG"

echo
echo "✓ released $TAG"
echo
echo "  GitHub Actions will build .app + .deb + tarball and"
echo "  auto-bump the Homebrew tap. Watch:"
echo "    https://github.com/SmooAI/smooblue/actions"
echo "    https://github.com/SmooAI/smooblue/releases/tag/$TAG"
