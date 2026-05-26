# Changelog

Written by [@changesets/cli](https://github.com/changesets/changesets) — each
release's section is generated from the `.changeset/*.md` files that landed
since the last release. See [.changeset/README.md](.changeset/README.md) for
the workflow.

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
