# Changelog

## 1.2.1

### Patch Changes

- [`fdd07b8`](https://github.com/SmooAI/smooblue/commit/fdd07b8f2e5905d57c06ef52b7835f157f2edc6c) Thanks [@brentrager](https://github.com/brentrager)! - Three fixes from the field:

  **Notifications: "interacted with you" generic phrase replaced with proper reasons.** The lexicon ships `like-via-repost` / `repost-via-repost` / `verified` / `unverified` / `subscribed-post` in addition to the original six, and the phrase mapping only knew about the originals — so likes on YOUR reposts showed up as the meaningless "X interacted with you." Now they read "X liked a post you reposted." Also unified the phrase + icon mapping into one source of truth on `NotificationGroup` so the next lexicon add only requires editing one file.

  **Compose typing lag.** Every keystroke into the post box was doing an inline `create_dir_all + fs::write` for draft persistence — on long drafts this stacked up enough to be visibly laggy. Moved the save off the render thread via `tokio::task::spawn_blocking`; the textarea now updates instantly and the draft saves asynchronously.

  **Notifications column slowness.** Three knobs: bumped poll interval from 20s → 30s (notifications churn slower than feeds and each poll allocates a chunk of memory for hydrated subject posts), dropped page size from 50 → 30 (50 was visibly laggy on busy accounts), and switched `.notif` / `.post` containment to `contain-intrinsic-size: auto …` so cards that scroll back into view use their _actual_ last-rendered size instead of falling back to the fixed estimate every time.

- [`b2ae9b7`](https://github.com/SmooAI/smooblue/commit/b2ae9b7688f58feaa72ebde1c5e66d9c16b1885c) Thanks [@brentrager](https://github.com/brentrager)! - Fix: "Quote post" fired from inside a thread (or any other sheet) now opens the compose dialog ON TOP of the thread instead of hidden behind it. Same fix applies to the FAB when fired with another sheet open. Root cause: every sheet shared the same `.modal__backdrop` z-index, so DOM order decided stacking — and compose was rendered first in `deck.rs`, putting it under everything else. Added a `.modal__backdrop--compose` modifier (z-index 60 vs the default 50) so the compose sheet always wins.

- [`76ea27f`](https://github.com/SmooAI/smooblue/commit/76ea27feb16c28adbc5e5ff0fd20c3a1544a53d3) Thanks [@brentrager](https://github.com/brentrager)! - Add a Smoo AI promo block to Settings → About — branded chip, tagline, version, and links out to smoo.ai / smoo.ai/open-source / source on GitHub / @brentragertech on Bluesky. Plus an MIT + Bluesky-trademark line at the bottom. Matches the same about-block pattern the other SmooAI open-source repos (config, logger, observability) already use in their READMEs.

  README also gets the canonical SmooAI top-of-file framing ("About SmooAI" → "SmooAI Open Source" → "About Smooblue") plus a Contact section at the bottom with email / socials / SmooAI GitHub link.

- [`7d46ecd`](https://github.com/SmooAI/smooblue/commit/7d46ecd9fdec124753ffe0ab5e7932006e07a86e) Thanks [@brentrager](https://github.com/brentrager)! - Long-thread scroll performance pass. The "flashing while scrolling a big thread" came from two compounding sources:

  1. **Single-image embeds had no reserved space.** The 2/3/4-up image grids set `aspect-ratio: 2/1` in CSS but the 1-up grid didn't, so single-image cards started at 0 height and reflowed to the decoded height the moment `loading=lazy` fired — and the cascade of reflows looked like a flash storm in WebKit. `EmbedImage` now carries the per-image `aspectRatio` from the lexicon; the render plumbs it onto the embed div as an inline `aspect-ratio` style + `width`/`height` attrs on the `<img>`. Fallback CSS reserves 16:9 when the lexicon omitted dims so legacy posts still don't flash.

  2. **Off-screen post cards were being laid out + painted on every scroll tick.** Added `content-visibility: auto` + `contain: layout style paint` (with `contain-intrinsic-size: 0 200px`) to `.post` and `.notif`. WebKit can now skip rendering off-screen cards entirely and never re-invalidate the rest of the column when one card changes. Biggest win on thread sheets with 100+ posts.

  Plus an AGENTS.md / CLAUDE.md update codifying the "land the plane" workflow: every chunk of work runs fmt → clippy → tests → drop a changeset → commit → push, in that order, before being called done.

- [`06f6021`](https://github.com/SmooAI/smooblue/commit/06f60213846517e3b6234a40c9bb69c5e692a38e) Thanks [@brentrager](https://github.com/brentrager)! - Hydrate + render the subject post for `like-via-repost` and `repost-via-repost` notifications. The reason mapping was fixed in the previous changeset but the subject-hydration code still only fetched URIs for `like` / `repost` / `quote`, so via-repost notifications had no post to show. Now they hydrate + display the post you reposted (the one that got the new engagement) with a "From your repost of @handle" caption so it's clear it's not your own post. Subscribed-post notifications get the same treatment.

Written by [@changesets/cli](https://github.com/changesets/changesets) — each
release's section is generated from the `.changeset/*.md` files that landed
since the last release. See [.changeset/README.md](.changeset/README.md) for
the workflow.

## 1.2.0

### Minor Changes

- [`f0d9008`](https://github.com/SmooAI/smooblue/commit/f0d900888412f5e745cbb438aff0a2b0ffabf6cc) Thanks [@brentrager](https://github.com/brentrager)! - Linux x86_64 release builds + one-line installer.

  The release workflow now has a second job that compiles a Linux x86_64 binary on ubuntu-latest and uploads `Smooblue-linux-x86_64.tar.gz` (binary + icon + README) as a release asset alongside the macOS .app.

  `install.sh` auto-detects platform and pulls the right asset:

  ```bash
  curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
  ```

  On Linux it installs the binary to `~/.local/bin/smooblue`, drops a `.desktop` entry into `~/.local/share/applications/`, copies the icon into the hicolor theme, refreshes the desktop database, and prints the runtime-deps apt line (webkit2gtk-4.1 / gtk-3 / libayatana-appindicator / librsvg).

### Patch Changes

- [`72ee460`](https://github.com/SmooAI/smooblue/commit/72ee4609934d3f8c95430367a9c57959db088f32) Thanks [@brentrager](https://github.com/brentrager)! - `install.sh` at the repo root: one-line installer that pulls the latest GitHub release zip, drops `Smooblue.app` into `/Applications` (or `~/Applications` if that's not writable), strips the quarantine xattr, and opens it.

  ```bash
  curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
  ```

  Idempotent — re-running upgrades in place. Apple Silicon only today (the release pipeline only ships `Smooblue-macos-arm64.zip`); x86_64 + Linux + Windows users get a clear error pointing at the build-from-source steps. `SMOOBLUE_NO_OPEN=1` to install without launching.

- [`71a53f3`](https://github.com/SmooAI/smooblue/commit/71a53f3a52afd3b74f30005f2f6503986e921570) Thanks [@brentrager](https://github.com/brentrager)! - README: split Install into per-platform sections. Adds Linux build instructions (webkit2gtk prerequisites, `cargo run --release` to launch) with honest caveats about macOS-only niceties (Apple Vision OCR, pbcopy-based copy-link, bundle-macos.sh) that degrade gracefully when missing. Notes Windows as theoretically buildable but untested.

- [`6b7cb32`](https://github.com/SmooAI/smooblue/commit/6b7cb327ebd61aab7f6284d30c820c3ae5827311) Thanks [@brentrager](https://github.com/brentrager)! - Tighten the post-action row — each icon+count is now wrapped in a `.post__action-pair` span with a 2px internal gap, while the gap between distinct groups (reply / repost / quote / like / copy) stays at 14px. Counts now read as belonging to their icons instead of floating mid-row. Reposts + quote now also show a zero count (matching reply + like) so the row stays the same width regardless of engagement state.

## 1.1.0

### Minor Changes

- UX overhaul + reliability sweep.

  **Reading**

  - In-app lightbox for images and videos (no more Preview.app context-switch). Esc / backdrop click closes.
  - Inline videos pause when scrolled out of view + resume when scrolled back in.
  - Rich text in posts — @mentions open profiles, links go to the browser (scheme-allowlisted), #hashtags open a search column.
  - Click a quoted post embed → opens the quoted post's thread (was a no-op).
  - Click a notification → opens the post (was: opens profile).
  - Inbound notification quotes (reply / mention / quote) render a full PostCard so you can like / repost / quote / reply directly from Notifications.
  - Thread sheet auto-scrolls to land on the post you clicked, even mid-thread.
  - Posts that are replies show a "Replying to @parent" chip; reposts show "Reposted by X".
  - Post timestamp links to bsky.app permalink; "more" copies the link to clipboard.
  - Stacked name + handle on post + quote heads — long display names stop bunching into the handle.

  **Browsing**

  - Column scrollback grows: top-poll merges new items at the head, "Load more" appends at the tail, capped at 2000 items / column (~6 MB).
  - Per-column fuzzy text filter (funnel icon next to the column X). 200ms debounce.
  - Sidebar nav buttons (Notifications / Discover / Suggested / Home) scroll to + flash the column if it's already in the deck.
  - Sidebar profile slot shows your avatar (resolved via getProfile on launch) with @handle tooltip.
  - "+ Add column" opens the rich picker (Your feeds + Subscribed + Lists + Trending + Popular + paste an AT-URI).
  - "Search posts" button on the profile sheet — opens a search column scoped via bsky's `from:` filter.
  - Notification cards use the head-row + full-width body layout (deck.blue convention) so the subject post has room.
  - Columns slimmer at 320px (from 350) to fit more side-by-side.

  **Auth**

  - Sessions move from Keychain to file storage (`~/Library/Application Support/ai.Smoo.smooblue/session.json`, 0600). Keychain ACLs broke on every adhoc-signed rebuild; files don't.
  - Single-flight refresh — concurrent column polls were racing the rotating refresh token. ~Every 2h users got bounced to login because the late-arriving refresh got `invalid_grant`.
  - Refresh writes to both legacy + per-DID session slots so the next-launch path doesn't pick up a stale token.
  - Multi-account switching (Settings → Accounts).

  **Compose**

  - Drag-and-drop images or video onto the compose sheet. 50 MB video size cap with a clear toast; read is offloaded to `spawn_blocking`.
  - Self-thread compose ("+ Thread" button to chain replies into one self-thread).
  - Image-post lexicon fix — `embed.images[].image` field name (was: `blob`, which the AppView 400'd).
  - Profile editor (display name / bio / avatar / banner via file picker).

  **Hardening**

  - URL scheme allowlist on every `open` call site — external embed clicks can't fire `file://`, `mailto:`, `slack://`, custom protocol handlers.
  - Defensive serde for `FeedItem.reply` / `.reason` — a weird shape on one item can't blow up feed decode.
  - 4 `use_resource` reactivity bugs fixed (Profile / Thread / SavedFeeds / Engagement sheets) where focus was captured by value and the sheet never re-fetched.

  **Operations**

  - Optional hourly auto-updater (launchd job). No-ops on dirty trees / non-main branches / running app. Logs to `~/Library/Logs/Smooblue/update.log`.
  - Native macOS app activation on launch so Cmd+Up / BetterSnapTool / Raycast hotkeys reach Smooblue without clicking the menu bar first.
  - Branch protection on main (CI status checks + linear history required).
  - 131 unit tests, all green.

  **Brand**

  - Smiley alien-butterfly icon redesign with a dark squircle background. Smoo monogram chip stamped bottom-right.

  **Docs**

  - Obsidian vault under `docs/` (Architecture / Engineering / Operations / Decisions / Projects).
  - 3 ADRs: session file vs Keychain, safe-open allowlist, publish=false workspace-wide.
  - `AGENTS.md` + `CLAUDE.md` at repo root pointing future agents at the vault + pearls workflow.

## 1.0.0 — 2026-05-25

The 1.0 cut. Smooblue ships every column type, full compose
(text / image / video / quote / thread / facets / alt-text), thread
view, profile editor, multi-account switching, moderation tooling,
vim-style keyboard navigation, light theme, and the OS-bundle +
auto-updater pipeline. macOS-only for now (the code is portable;
just needs CI wiring for other platforms).

### Highlights

- **Multi-column deck** — Home, Notifications, Discover, custom
  feeds, lists, search, profile, suggested follows. Drag to reorder.
  Persistent layout across launches.
- **Compose** — text, replies, quotes, self-threading, images (up to 4) with auto-alt-text (Apple Vision OCR + LLM scene description),
  video (mp4 / mov / webm), drag-and-drop, ⌘↵ submit, draft
  persistence.
- **Thread view** — click any post body to open the conversation;
  reactive on focus changes so drilling into a post inside the
  thread re-fetches automatically.
- **Profile** — view, edit (display name / bio / avatar / banner),
  follow / mute / block / report. Pinned post + mutuals row.
- **Multi-account switching** — sign into multiple accounts; flip
  the active one from Settings. Sessions stored in
  `~/Library/Application Support/ai.Smoo.smooblue/session-<did>.json`
  (0600), survives rebuilds (Keychain ACL was tied to the app code
  signature and broke on every adhoc rebuild).
- **Keyboard nav** — vim-style `j`/`k`/`gg`/`G`/`h`/`l`, chord
  prefix `g` (gh/gn/gd/gp/gs), Space leader for compose / search /
  settings / saved-feeds / column-jump. `?` toggles help overlay.
- **Brand mark** — butterfly-primary, smoo-monogram chip stamped
  bottom-right. Borg-cybernetic glow-up in the 1.0 cut.
- **macOS niceties** — activates as a foreground app on launch so
  system hotkey tools (BetterSnapTool, Magnet, Raycast) work without
  the menu-bar-click workaround.

### Distribution

- Single-binary `.app` bundle via `scripts/bundle-macos.sh`.
- Optional hourly auto-updater via the launchd plist template in
  `scripts/` — safe by design (no-op on dirty trees / feature
  branches / unchanged origin).
- 91 unit tests across the workspace; all green.

### Not yet

- Cross-platform builds (Linux / Windows) — code portable, CI wiring
  is the only blocker.
- DMs (`chat.bsky.*`) — separate lexicon, intentional follow-up.
- Apple Developer notarization (currently adhoc-signed; first-run
  Gatekeeper requires right-click → Open).
