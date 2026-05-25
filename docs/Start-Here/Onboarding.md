# Onboarding

#start-here

Get from "fresh clone" to "Smooblue running on your Dock" in ~5 minutes.

---

## Prerequisites

- **Rust 1.80+** — `rustup toolchain install stable`
- **macOS 11.0+** — Smooblue is macOS-only today
- **librsvg** (`brew install librsvg`) — only if you'll regenerate the icon PNGs
- **ffmpeg** (`brew install ffmpeg`) — only if you'll regenerate the demo GIF in the README
- **gh** (optional) — branch protection / release management

---

## First run

```bash
git clone https://github.com/SmooAI/smooblue.git
cd smooblue

# Dev launch (rebuilds on every change; useful while iterating UI)
cargo run -p smooblue-app

# Or build + install to /Applications
bash scripts/bundle-macos.sh
cp -R dist/Smooblue.app /Applications/
xattr -dr com.apple.quarantine /Applications/Smooblue.app
open /Applications/Smooblue.app
```

When you first sign in, Bluesky opens in your default browser for OAuth. The DPoP private key + tokens land at `~/Library/Application Support/ai.Smoo.smooblue/session.json` (mode `0600`).

---

## Demo mode

For screenshots, screen recording, scale tests, and UI iteration that doesn't need the network:

```bash
SMOOBLUE_DEMO=1 cargo run -p smooblue-app
SMOOBLUE_DEMO=1 SMOOBLUE_DEMO_SCALE=large cargo run -p smooblue-app   # 500 posts/column
```

Scales: `small` (default, 14 posts), `medium` (100), `large` (500), `huge` (2000), `insane` (5000). See [[Engineering/Demo-Mode]] for details.

---

## Workspace layout

```
smooblue/
├── crates/
│   ├── smooblue-app/         # Dioxus desktop binary + UI components
│   ├── smooblue-atproto/     # XRPC client (timeline, feeds, profile, ...)
│   ├── smooblue-crm/         # opt-in Smoo CRM sync (privacy boundary)
│   ├── smooblue-oauth/       # ATproto OAuth (PAR + PKCE + DPoP)
│   └── smooblue-theme/       # CSS tokens + shared sheet
├── assets/
│   ├── icons/                # generated PNG app icons (16 → 1024)
│   ├── icon.svg              # source SVG (smiley alien butterfly)
│   └── styles.css            # smooblue-specific component CSS
├── media/
│   ├── smooblue-demo.mp4     # full-quality demo (1080p, 65 MB)
│   └── smooblue-demo.gif     # README-embedded demo (480p, 7.4 MB)
├── docs/                     # this Obsidian vault
├── scripts/
│   ├── bundle-macos.sh           # release build → dist/Smooblue.app
│   ├── build-icons.sh            # rsvg → all PNG sizes
│   ├── smooblue-update.sh        # auto-updater pull+build+reinstall
│   └── ai.smoo.smooblue.updater.plist.template
└── Cargo.toml                # Cargo workspace
```

---

## What to read next

1. [[Architecture/Architecture-Overview]] — how the pieces fit
2. [[Engineering/Engineering-Guide]] — daily workflow + commit conventions
3. [[Operations/Bundle-and-Install]] — how the `.app` gets made
4. [[Decisions/ADR-Index]] — non-obvious choices the project has already made

---

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| First-run Gatekeeper "can't open" | adhoc-signed binary | `xattr -dr com.apple.quarantine /Applications/Smooblue.app` |
| `cargo run` fails to find `xattr` / `open` | running on Linux | macOS-only today; cross-platform CI is a future pearl |
| OAuth callback never completes | port collision on the loopback listener | quit any prior dev instance; OAuth uses ephemeral ports |
| Cmd+Up / Magnet hotkey doesn't reach Smooblue | NSApp activation regression | covered by `activate_macos_app()` in `main.rs`; ensure you're on `main` |
| Session lost after rebuild | pre-1.0 Keychain-stored session | one-time re-auth; sessions now live in `~/Library/Application Support/` |
