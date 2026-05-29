---
"smooblue": patch
---

Homebrew cask now auto-strips macOS quarantine on install. Without this, macOS Sequoia's Gatekeeper refuses to launch the adhoc-signed `.app` with "Apple could not verify Smooblue is free of malware" and offers no GUI "Open Anyway" button — the only escape was a terminal `xattr` command, which defeats the point of a one-line cask install. The cask now runs `xattr -cr` in a `postflight` block so `brew install --cask smooblue` (and `brew upgrade --cask smooblue`) launch cleanly on first try. Direct .zip downloads from a GitHub release are NOT modified — those still need the manual one-liner, documented in the README's Install section + the Security doc's "What's NOT done" list. Real fix (Apple Developer ID enrollment + notarization) tracked as a follow-up; held until the $99/yr cost is justified by usage.
