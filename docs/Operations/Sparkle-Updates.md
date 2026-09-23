# Sparkle Updates (OTA)

#operations

Smooblue updates itself with **Sparkle 2**, the same updater and the same release shape SmoothFlow uses (`SmooAI/smooth` → `docs/Architecture/SmoothFlow-macOS.md`, "Release and OTA"). Release builds are **Developer ID-signed, notarized and stapled**, so they also open without the old "Apple could not verify…" Gatekeeper dialog.

---

## What the user sees

- An hourly background check (`SUScheduledCheckInterval` 3600). When a newer release exists, Sparkle's own native dialog appears: "A new version of Smooblue is available!", the release notes, **Skip This Version / Remind Me Later / Install Update**, and an "Automatically download and install updates in the future" checkbox.
- **Smooblue → Check for Updates…** in the app menu, plus **Settings → About → Check for updates…**, for a manual check.
- **Install Update** downloads the release zip, verifies its EdDSA signature, replaces the app in place and relaunches.

Builds without Sparkle fall back to the old GitHub "Update available" toast (`crates/smooblue-app/src/updates.rs`). That covers `cargo run`, Linux, and `SMOOBLUE_NO_SPARKLE=1` bundles. The deck shows the toast only when `sparkle::is_active()` is false.

---

## How it fits together

| Piece | Where |
| --- | --- |
| Framework | `scripts/bundle-macos.sh` downloads Sparkle **2.9.6** (pinned SHA-256; cached in `~/.cache/smooblue`) and copies `Sparkle.framework` into `Contents/Frameworks` with `ditto`, which keeps its symlinks. |
| Info.plist | `SUFeedURL`, `SUPublicEDKey`, `SUEnableAutomaticChecks`, `SUScheduledCheckInterval`; `CFBundleVersion` set to the real release version |
| Runtime | `crates/smooblue-app/src/sparkle.rs` loads the framework with `NSBundle` (not linked), creates `SPUStandardUpdaterController` on the main thread, and adds the menu item. It runs under `catch_unwind` and `objc2::exception::catch`, so a failure logs instead of crashing. |
| Signing | `scripts/sign-and-notarize-macos.sh` signs inside-out: Sparkle's XPC services, Autoupdate, Updater.app, the framework, then the app. It never uses `--deep`. Then it notarizes, **staples the `.app`**, and zips `dist/Smooblue-macos-arm64.zip`. |
| Appcast | `scripts/make-appcast.py` EdDSA-signs the zip with Sparkle's `sign_update` and verifies that signature against the app's `SUPublicEDKey`. It writes a one-item `appcast.xml` whose notes are that version's CHANGELOG section in Markdown. |
| Hosting | GitHub Releases. `SUFeedURL` is `https://github.com/SmooAI/smooblue/releases/latest/download/appcast.xml`, which redirects to the newest release's `appcast.xml` asset. No S3 or CDN is needed (SmoothFlow uses `downloads.smoo.ai`). |
| CI | `.github/workflows/release.yml` (on a `vX.Y.Z` tag): import cert → sign/notarize/staple → appcast → upload the zip, then the appcast → re-download both and verify codesign, stapler, `spctl`, version, and the EdDSA signature against the shipped key. The PR smoke test (`ci.yml`) launches the bundle with `SMOOBLUE_FORCE_SPARKLE=1` and fails unless it logs `sparkle: updater started`. |

The zip is one file with three jobs: the GitHub release asset, the Homebrew cask download, and the Sparkle enclosure.

---

## Secrets (`SmooAI/smooblue` → Actions)

These are the same names as `SmooAI/smooth`'s `smoothflow-publish.yml`. GitHub secrets are per-repo and write-only, so they are set here separately.

| Secret | Value |
| --- | --- |
| `MACOS_CERT_P12` | base64 of the **Developer ID Application: Smoo LLC (DTX9733844)** identity exported as `.p12` |
| `MACOS_CERT_PASSWORD` | that `.p12`'s export password |
| `MACOS_SIGN_IDENTITY` | `Developer ID Application: Smoo LLC (DTX9733844)` |
| `NOTARY_KEY_P8` | base64 of the App Store Connect API key `AuthKey_<id>.p8` |
| `NOTARY_KEY_ID` | that key's ID |
| `NOTARY_ISSUER` | the team's issuer UUID (the same one in `smooai/apps/bigsmooth/ios/fastlane/Fastfile`) |
| `SMOOBLUE_SPARKLE_PRIVATE_KEY` | output of `generate_keys --account Smooblue -x <file>` |

Without `MACOS_CERT_P12`, releases ship the ad-hoc zip as before, with no appcast (a warning, not a failure). A signed build **without** `NOTARY_KEY_P8` fails the release on purpose: a signed but un-notarized update would ship.

Set values with `gh secret set NAME -R SmooAI/smooblue --body "$(cat file)"`. The command substitution strips the trailing newline, which byte-comparing consumers need.

### The Sparkle key: don't lose it

One EdDSA key pair is used for every Smooblue release.

- **Public half:** `SUPublicEDKey` in `bundle-macos.sh`.
- **Private half:**
  - the **"Smooblue" account in the release manager's login keychain**, created with `generate_keys --account Smooblue` on 2026-09-23
  - the `SMOOBLUE_SPARKLE_PRIVATE_KEY` secret

Every installed copy trusts only this key. If it's lost, a new key has to ship in a new build, and installed apps will refuse any update signed with it. Everyone would have to reinstall by hand. **Keep the keychain copy**, and back it up with `generate_keys --account Smooblue -x`.

---

## Cutting a release

Nothing changes: merge PRs with changesets, then the Release PR auto-merges, the tag is pushed, and `release.yml` runs. Installed apps pick up the release on their next hourly check. To confirm it's live:

```bash
curl -sL https://github.com/SmooAI/smooblue/releases/latest/download/appcast.xml | grep sparkle:version
```

---

## Signing + notarizing locally

```bash
scripts/bundle-macos.sh
NOTARY_KEY=~/.appstoreconnect/private_keys/AuthKey_XXXX.p8 NOTARY_KEY_ID=XXXX \
NOTARY_ISSUER=<issuer-uuid> scripts/sign-and-notarize-macos.sh
# or: NOTARY_PROFILE=<notarytool keychain profile>; or SKIP_NOTARIZE=1 to sign only
```

`SIGN_IDENTITY` defaults to the first "Developer ID Application" identity in the keychain.

---

## Testing an update end to end (no real account, no real feed)

This is how the flow was verified on 2026-09-23 (1.29.2 → 1.29.3, installed and relaunched):

1. `scripts/bundle-macos.sh && SKIP_NOTARIZE=1 scripts/sign-and-notarize-macos.sh`, then copy `dist/Smooblue.app` somewhere as the "old" app.
2. Make a "new" copy. Bump `CFBundleVersion` and `CFBundleShortVersionString` with `plutil -replace`, then re-sign it: `APP_BUNDLE=… ZIP_OUT=feed/Smooblue-X.zip SKIP_NOTARIZE=1 scripts/sign-and-notarize-macos.sh`.
3. `scripts/make-appcast.py --version X --zip feed/Smooblue-X.zip --url http://127.0.0.1:8765/Smooblue-X.zip --out feed/appcast.xml --app <new app>` (signs with the keychain key).
4. `python3 -m http.server 8765` in `feed/`, then `defaults write ai.smoo.smooblue SUFeedURL http://127.0.0.1:8765/appcast.xml`.
5. Launch the old app with `SMOOBLUE_DEMO=1 SMOOBLUE_FORCE_SPARKLE=1`. Sparkle offers the update; Install Update → Install and Relaunch.
6. **Clean up:** `defaults delete ai.smoo.smooblue SUFeedURL`. The user-defaults feed overrides Info.plist.

`SMOOBLUE_DISABLE_SPARKLE=1` turns the updater off, as demo mode does unless `SMOOBLUE_FORCE_SPARKLE=1` is set.

---

## Related

- [[Bundle-and-Install]] · [[Auto-Updater]] (the build-from-source launchd job for developers; Sparkle is the path for users)
- [[../Security/Security]]
