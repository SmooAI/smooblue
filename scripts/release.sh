#!/usr/bin/env bash
# Tag the current commit `vX.Y.Z` (reading X.Y.Z from package.json),
# push the tag, and rely on .github/workflows/release.yml to build +
# upload the macOS .app bundle to the GitHub release.
#
# Run after `pnpm version` + the resulting "Release vX.Y.Z" commit
# has landed on main. Idempotent: skips if the tag already exists.
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION=$(node -p "require('./package.json').version")
TAG="v$VERSION"

if [[ -z "$VERSION" ]]; then
    echo "✗ couldn't read version from package.json"
    exit 1
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "✓ $TAG already exists — nothing to do"
    exit 0
fi

# Sanity: tag has to live on main and the current commit needs to be
# the "Release $TAG" one. We don't enforce strictly (so hotfix
# releases off feature branches can still ship) but warn loudly.
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$BRANCH" != "main" ]]; then
    echo "⚠ on branch '$BRANCH' — releases typically land on main. Continuing anyway."
fi

echo "▸ tagging $TAG on $(git rev-parse --short HEAD)"
git tag -a "$TAG" -m "Release $TAG"
git push origin "$TAG"

echo "✓ pushed $TAG"
echo "  GitHub Actions will build the .app and attach it to the release at:"
echo "  https://github.com/SmooAI/smooblue/releases/tag/$TAG"
