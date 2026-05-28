#!/usr/bin/env bash
# Called by `changesets/action@v1` as its `publish` command after the
# Version PR is merged to main (which leaves no pending changesets,
# which is the signal the action uses to switch from "open a Version
# PR" mode to "publish" mode).
#
# At that point package.json already has the new version (the action
# bumped it in the Version PR). Our job is:
#   1. Read the version
#   2. Tag main with vX.Y.Z
#   3. Push the tag — but NOT with GITHUB_TOKEN, because tag pushes
#      from the default bot don't re-trigger downstream workflows
#      (GitHub anti-loop guard), and our release.yml fires on
#      tag pushes. The SMOOBLUE_RELEASE_DEPLOY_KEY secret is an
#      SSH deploy key on this repo with write access; using it
#      keeps the tag push as a "real" push that release.yml sees.
#
# Idempotent — if the tag already exists, exits 0 with nothing-to-do.
set -euo pipefail

VERSION=$(node -p "require('./package.json').version")
TAG="v$VERSION"

if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "✓ $TAG already exists — nothing to push"
    exit 0
fi

if [[ -z "${SMOOBLUE_RELEASE_DEPLOY_KEY:-}" ]]; then
    echo "✗ SMOOBLUE_RELEASE_DEPLOY_KEY not set — refusing to push tag"
    echo "  (without a deploy key, the tag push would use GITHUB_TOKEN,"
    echo "  which doesn't retrigger release.yml — the .app / .deb /"
    echo "  tarball would never get built)"
    exit 1
fi

echo "▸ setting up deploy key for tag push…"
mkdir -p ~/.ssh
printf '%s\n' "$SMOOBLUE_RELEASE_DEPLOY_KEY" > ~/.ssh/release_deploy
chmod 600 ~/.ssh/release_deploy
ssh-keyscan github.com >> ~/.ssh/known_hosts 2>/dev/null

echo "▸ tagging $TAG on $(git rev-parse --short HEAD)"
git tag -a "$TAG" -m "Release $TAG"

# Push directly to the SSH remote so GIT_SSH_COMMAND applies.
# `origin` is set up by actions/checkout with the HTTPS URL +
# GITHUB_TOKEN, which is exactly the credential we're trying to
# avoid here.
echo "▸ pushing tag via SSH deploy key"
GIT_SSH_COMMAND="ssh -i ~/.ssh/release_deploy -o IdentitiesOnly=yes -o StrictHostKeyChecking=no" \
    git push git@github.com:SmooAI/smooblue.git "$TAG"

echo "✓ pushed $TAG — release.yml will now build + upload + bump the brew tap"
