<a name="readme-top"></a>

<div align="center">
  <a href="https://smoo.ai">
    <img src="assets/icons/icon-256.png" alt="Smooblue" width="180" />
  </a>
</div>

## About SmooAI

SmooAI is an AI-powered platform for helping businesses multiply their customer, employee, and developer experience.

Learn more on [smoo.ai](https://smoo.ai).

## SmooAI Open Source

Check out other SmooAI open-source packages at [smoo.ai/open-source](https://smoo.ai/open-source).

## About Smooblue

A native, multi-column [Bluesky](https://bsky.app) desktop client for macOS and Linux. Written in Rust + [Dioxus](https://dioxuslabs.com/), backed by Bluesky's official OAuth flow (PAR + PKCE + DPoP-bound tokens).

<p align="center">
  <img src="media/smooblue-demo.gif" alt="Smooblue demo" width="720" />
</p>

<p align="center"><sub>
  GIF heavily downsampled to fit GitHub's inline limit. Full quality:
  <a href="media/smooblue-demo.mp4"><strong>▶ smooblue-demo.mp4 (1080p · 65 MB)</strong></a>
</sub></p>

---

## Install

### macOS — Homebrew (recommended)

```bash
brew tap SmooAI/tools
brew install --cask smooblue
```

Apple Silicon only today. `brew upgrade --cask smooblue` updates on every release.

### Linux — .deb (Debian / Ubuntu)

Grab the `.deb` from the [latest release](https://github.com/SmooAI/smooblue/releases/latest) and:

```bash
sudo apt install ./Smooblue_*.deb
```

apt pulls in `libwebkit2gtk-4.1` / `libgtk-3` / `libayatana-appindicator3` / `librsvg2` for you. `Smooblue` shows up in the launcher / activities overview. `sudo apt upgrade smooblue` after future releases (once you've installed once with the file).

### macOS or Linux — curl one-liner

Auto-detects platform; macOS gets the `.app`, Linux gets the tarball-extracted binary. Doesn't go through brew/apt, so updates are manual (re-run the same command):

```bash
curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
```

## What it is

A TweetDeck-style desktop client for Bluesky. Stack as many columns as you want — Home, Notifications, Discover, your saved feeds, lists, search, individual profiles, suggested-follows — and watch them all live-update side-by-side. No app passwords; sign in once via OAuth and Smooblue holds DPoP-bound tokens on disk (0600, your config dir).

Built fast, single-binary, ~11 MB native app — feels closer to a Finder window than an Electron browser tab.

## Features

**Deck**
- Multi-column horizontal scrolling deck (Home / Notifications / Discover / custom feeds / lists / search / profile / suggested follows)
- Drag-to-reorder columns
- "Your feeds" — feed generators you've authored show up first in the column picker
- Paste any feed AT-URI to add a custom column
- Trending topic chips + popular-feeds browser
- Per-column close + persistent layout across launches
- Light + dark themes (token-based, brand colors preserved)

**Posts**
- Compose, reply, repost, like, quote, delete
- Self-threading (chain replies on submit)
- Image attachments (up to 4) with auto-generated alt-text (Apple Vision OCR + LLM scene description)
- **Drag-and-drop** images or video onto the compose sheet
- Video attachments (mp4 / mov / webm)
- Rich-text facets — @mentions, #hashtags, http links auto-detected + resolved
- ⌘↵ to submit
- Draft persisted across launches

**Read**
- Thread view — click any post body to open the conversation
- Click a notification to jump to the relevant post (or profile for follows)
- "Reposted by X" / "Replying to @Y" chips on every feed card
- Tap the timestamp on any post to open it on bsky.app in your browser
- "More" → copy bsky.app permalink to clipboard
- Engagement modals (likes / reposts / quotes) — tap a count on any post
- Content-warning interstitial for labeled (NSFW / graphic / sensitive) posts

**Profile**
- Your own profile view + edit (display name, bio, avatar, banner via file picker)
- Other profiles with follow / mute / block / report
- Pinned post displayed at the top with a chip
- "Followed by ... and X others you follow" mutuals row

**Accounts & moderation**
- Multi-account switching (sign into as many as you want, flip via Settings)
- Mute & block list management in Settings → Moderation
- Report flow with bsky's canonical moderation reasons

**Vim-style keyboard navigation**
- `j` / `k` next / previous post
- `h` / `l` previous / next column
- `gg` top of column, `G` bottom
- `g` then `h` / `n` / `d` / `s` / `p` for Home / Notifications / Discover / Suggested / Profile
- Space leader → `n` new post, `/` search, `s` settings, `f` saved feeds, `?` help, `1`–`9` jump to column N
- `?` toggles the keyboard help overlay
- Esc closes the topmost modal; ⌘K opens search anywhere

**Operational**
- Self-update notifier — checks GitHub releases on launch
- Optional system-level auto-updater (launchd job, hourly) that rebuilds + reinstalls from `main`
- macOS app activation done right — Cmd+Up / BetterSnapTool / Raycast hotkeys reach Smooblue without clicking the menu bar first

## Install

### macOS (supported)

One-liner — grabs the latest release, installs to `/Applications` (or `~/Applications` if that's not writable), and opens it:

```bash
curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
```

Apple Silicon only today (`Smooblue-macos-arm64.zip`). Re-running upgrades in place. To install without launching after, set `SMOOBLUE_NO_OPEN=1`.

Or build from source:

```bash
git clone https://github.com/SmooAI/smooblue.git
cd smooblue
./scripts/bundle-macos.sh         # builds release + creates dist/Smooblue.app
cp -R dist/Smooblue.app /Applications/
xattr -dr com.apple.quarantine /Applications/Smooblue.app
open /Applications/Smooblue.app
```

Or stay current automatically (hourly rebuild + reinstall from `main`):

```bash
sed -e "s|@USER@|$USER|g" -e "s|@HOME@|$HOME|g" \
    scripts/ai.smoo.smooblue.updater.plist.template \
    > ~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist
launchctl load ~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist
```

The updater is a no-op when there are no new commits on `main` or your working tree is dirty — safe to leave running.

### Linux (x86_64)

Same one-liner as macOS — it auto-detects platform and grabs `Smooblue-linux-x86_64.tar.gz` instead:

```bash
curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
```

Installs the binary to `~/.local/bin/smooblue` and drops a `.desktop` file in `~/.local/share/applications/`. **You'll need the webkit2gtk runtime libs**; the installer prints the apt command after install. Debian/Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2
```

(Other distros: install the equivalents of `webkit2gtk-4.1`, `gtk3`, `libayatana-appindicator`, `librsvg`.)

A few macOS-specific niceties degrade gracefully on Linux:
- Apple Vision OCR for auto-alt-text → falls back to the LLM scene description.
- "Copy link" on a post uses `pbcopy` — needs a one-line patch to call `xclip` / `wl-copy` instead.

Or build from source (needs the `-dev` versions of the runtime libs above + `build-essential`):

```bash
git clone https://github.com/SmooAI/smooblue.git
cd smooblue
cargo run --release -p smooblue-app
```

### Windows (not yet)

Wry supports Windows via WebView2, so the core should build, but nobody's tried. The `safe_open` shell-out, the macOS activation hook, and the bundle script would all need a Windows arm.

## Privacy — what Smooblue sends where

| Data                      | Sent to                                  | When                                                                 |
| ------------------------- | ---------------------------------------- | -------------------------------------------------------------------- |
| Handle, password (typed)  | **Nowhere** — Bluesky handles auth       | Never; OAuth means Smooblue never sees your password                 |
| Bluesky access token      | Your PDS (which proxies to AppView)      | Every XRPC call                                                      |
| Session (DPoP key + tokens) | Local file (0600 in config dir)        | After sign-in; survives rebuilds (Keychain ACL was unreliable)        |
| Display name, handle, DID | **Smoo AI CRM** *(opt-in only)*          | Only if you tick "Stay in touch with Smoo AI" during sign-in         |

The Smoo AI CRM sync is off by default and reversible from Settings.

## Build from source

```bash
# requires Rust 1.80+
cargo run --release -p smooblue-app          # dev launch
cargo test --workspace --lib                  # unit tests (91 of them)
bash scripts/bundle-macos.sh                  # produces dist/Smooblue.app
bash scripts/build-icons.sh                   # regen PNG icons from icon.svg
```

Demo mode (no network, canned data — useful for screenshots):

```bash
SMOOBLUE_DEMO=1 cargo run -p smooblue-app
SMOOBLUE_DEMO=1 SMOOBLUE_DEMO_SCALE=large cargo run -p smooblue-app  # 500-post scale test
```

## Layout

```
smooblue/
├── crates/
│   ├── smooblue-app/      # Dioxus desktop binary + components
│   ├── smooblue-atproto/  # XRPC client (timeline, profile, notifs, feeds, ...)
│   ├── smooblue-crm/      # opt-in Smoo CRM sync
│   ├── smooblue-oauth/    # ATproto OAuth (PAR + PKCE + DPoP)
│   └── smooblue-theme/    # CSS tokens + shared sheet
├── assets/
│   ├── icons/             # generated PNG app icons (16 → 1024)
│   ├── icon.svg           # source SVG (Bluesky butterfly + smoo monogram chip)
│   └── styles.css         # smooblue-specific component CSS
├── media/
│   └── smooblue-demo.mp4  # demo recording
├── scripts/
│   ├── bundle-macos.sh
│   ├── build-icons.sh
│   ├── smooblue-update.sh
│   └── ai.smoo.smooblue.updater.plist.template
└── Cargo.toml             # Cargo workspace
```

## Roadmap

- DMs (`chat.bsky.*`)
- Pinned posts ordering inside a thread sheet
- Trending topics → live deep-link to bsky search
- Cross-platform builds (Linux / Windows) — code is portable, just needs CI

## Contributing

Issues and PRs welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Contact

Brent Rager

- [Email](mailto:brent@smoo.ai)
- [LinkedIn](https://www.linkedin.com/in/brentrager/)
- [Bluesky](https://bsky.app/profile/brentragertech.bsky.social)
- [TikTok](https://www.tiktok.com/@brentragertech)
- [Instagram](https://www.instagram.com/brentragertech/)

SmooAI on GitHub: [https://github.com/SmooAI](https://github.com/SmooAI)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## License

[MIT](LICENSE) © [Smoo AI](https://smoo.ai)

---

*Smooblue is not affiliated with Bluesky Social, PBC. "Bluesky" and the Bluesky butterfly are trademarks of Bluesky Social, PBC.*
