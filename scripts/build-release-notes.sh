#!/usr/bin/env bash
# build-release-notes — assemble the GitHub-release body for vX.Y.Z.
#
# Wraps the per-version section pulled from CHANGELOG.md with
# install/upgrade/download sections so the release page is a
# self-serve "how do I get this on my machine" guide.
#
# Used by .github/workflows/release.yml when a vX.Y.Z tag fires the
# build. Also runnable locally to retroactively re-render an existing
# release: `./scripts/build-release-notes.sh 1.3.0 CHANGELOG.md > /tmp/n.md`
# then `gh release edit v1.3.0 --notes-file /tmp/n.md`.
#
# Usage: build-release-notes.sh VERSION [CHANGELOG_PATH]
#   VERSION         bare version, no "v" prefix (e.g. 1.3.0)
#   CHANGELOG_PATH  path to CHANGELOG.md (default: ./CHANGELOG.md)
set -euo pipefail

VERSION="${1:?usage: $0 VERSION [CHANGELOG_PATH]}"
CHANGELOG_PATH="${2:-CHANGELOG.md}"

# Extract the `## VERSION` section up to the next `## X.Y.Z` header.
CHANGELOG=$(awk -v ver="$VERSION" '
    $0 ~ "^## " ver "$" { found=1; next }
    found && /^## [0-9]+\./ { exit }
    found { print }
' "$CHANGELOG_PATH")

if [[ -z "${CHANGELOG// }" ]]; then
    CHANGELOG="_No changelog entries found for ${VERSION} — see [CHANGELOG.md](https://github.com/SmooAI/smooblue/blob/main/CHANGELOG.md)._"
fi

cat <<EOF
## Install

**macOS** (Homebrew — recommended):

\`\`\`sh
brew tap SmooAI/tools
brew install --cask smooblue
\`\`\`

**macOS** (manual): download \`Smooblue-macos-arm64.zip\` below, unzip, drag \`Smooblue.app\` to \`/Applications\`.

**Linux** (Debian/Ubuntu .deb — pulls runtime deps automatically):

\`\`\`sh
curl -LO https://github.com/SmooAI/smooblue/releases/download/v${VERSION}/Smooblue_${VERSION}_amd64.deb
sudo apt install ./Smooblue_${VERSION}_amd64.deb
\`\`\`

**Linux** (binary tarball): download \`Smooblue-linux-x86_64.tar.gz\` below — \`README.txt\` inside lists the runtime libs to apt-install.

## Upgrade

- **Homebrew:** \`brew upgrade --cask smooblue\`
- **Linux (.deb):** re-run the \`apt install\` above with the new file — apt handles the upgrade in place.

## What's new
${CHANGELOG}
## Downloads

| Platform | File |
| --- | --- |
| macOS (Apple Silicon) | \`Smooblue-macos-arm64.zip\` |
| Linux (.deb, amd64) | \`Smooblue_${VERSION}_amd64.deb\` |
| Linux (tarball, x86_64) | \`Smooblue-linux-x86_64.tar.gz\` |

---

[Security model](https://github.com/SmooAI/smooblue/blob/main/docs/Security/Security.md) · [Source](https://github.com/SmooAI/smooblue) · [smoo.ai](https://smoo.ai) — Smooblue is open source by [SmooAI](https://smoo.ai/open-source).
EOF
