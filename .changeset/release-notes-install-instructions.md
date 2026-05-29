---
"smooblue": patch
---

Release notes on GitHub now lead with install + upgrade commands (Homebrew, .deb, manual) and end with an asset table — so anyone landing on a release page from an "update available" link gets a self-serve guide instead of a bare changeset list. The changelog body is unchanged; it's wrapped by a new `scripts/build-release-notes.sh` that `release.yml` calls when a tag fires. The same script can be run locally to retroactively re-render older releases (`./scripts/build-release-notes.sh 1.2.2 CHANGELOG.md > /tmp/n.md && gh release edit v1.2.2 --notes-file /tmp/n.md`).
