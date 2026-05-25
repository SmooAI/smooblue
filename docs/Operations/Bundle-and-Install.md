# Bundle and Install

#operations

How the macOS `.app` gets built and installed.

---

## TL;DR

```bash
bash scripts/bundle-macos.sh
cp -R dist/Smooblue.app /Applications/
xattr -dr com.apple.quarantine /Applications/Smooblue.app
open /Applications/Smooblue.app
```

That's it. Takes ~90s on an M-series Mac for a clean release build, ~10s for an incremental.

---

## What `bundle-macos.sh` does

1. `cargo build --release -p smooblue-app` — produces `~/.cargo/shared-target/release/smooblue-app` (workspace target dir, not `./target/`)
2. Lays out the `dist/Smooblue.app` bundle:
   ```
   dist/Smooblue.app/
   ├── Contents/
   │   ├── Info.plist           ← rendered from the [bundle] block in Dioxus.toml
   │   ├── MacOS/
   │   │   └── Smooblue         ← the release binary
   │   └── Resources/
   │       └── Icon.icns        ← assembled from assets/icons/icon-*.png
   ```
3. Builds `Icon.icns` via `iconutil` from a temp iconset directory
4. Ad-hoc signs the bundle (`codesign --force --deep --sign -`) so it can run on the local machine

There's no `--skip-build` flag — the script always rebuilds. Use `cargo build --release -p smooblue-app` separately + `bash scripts/bundle-macos.sh --skip-build` if you want to skip (TODO: that flag doesn't exist yet; a future pearl).

---

## Why adhoc-signed

Apple Developer notarization needs a paid Developer Program membership + a Developer ID Application certificate + a notarization service round-trip. Worth doing for public distribution; not worth it for "the four of us run this internally." First-run experience for users today:

1. Double-click → Gatekeeper bars launch
2. Right-click → Open → "Open" → confirm
3. Subsequent launches are unrestricted

The `xattr -dr com.apple.quarantine` after `cp` shortcuts the dance for developers installing from source.

Notarization is a future pearl. See [[../Decisions/ADR-Index]] when it lands.

---

## Install paths

Default is `/Applications/Smooblue.app`. Override with `SMOOBLUE_INSTALL` if you want `~/Applications/` (user-only) instead:

```bash
SMOOBLUE_INSTALL=~/Applications/Smooblue.app bash scripts/smooblue-update.sh
```

The auto-updater respects the same env var.

---

## Icon pipeline

`assets/icon.svg` is the source. `scripts/build-icons.sh` rasterizes it via `rsvg-convert` (`brew install librsvg`) to all sizes Dioxus / macOS want:

```
assets/icons/
├── icon-16.png
├── icon-32.png
├── icon-64.png
├── icon-128.png
├── icon-256.png
├── icon-512.png
└── icon-1024.png
```

`bundle-macos.sh` assembles those into `Icon.icns`. The SVG itself includes a dark squircle background (`rx="30"` rounded corners) so the icon reads as a standalone app mark wherever it's displayed — Dock, README, GitHub social preview, favicon — without needing the OS-applied icon mask.

---

## Related

- [[Auto-Updater]]
- [[../Engineering/Engineering-Guide#Release]]
