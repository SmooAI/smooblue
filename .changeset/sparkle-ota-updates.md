---
"smooblue": minor
---

Smooblue now updates itself. Release builds use Sparkle 2, the same updater as SmoothFlow. It checks for new versions every hour and shows Sparkle's standard update window: the release notes, **Skip This Version / Remind Me Later / Install Update**, and an option to install updates automatically. Choosing Install downloads the update, checks its signature, and relaunches into the new version. You can also check any time from **Smooblue → Check for Updates…** or **Settings → About**.

Releases are now signed with Smoo's Apple Developer ID and notarized by Apple, so macOS opens them without the "Apple could not verify…" warning.

The app's build number also now matches the real release version; every build previously reported 0.1.0.
