# Changelog

## 1.25.0

### Minor Changes

- [#67](https://github.com/SmooAI/smooblue/pull/67) [`8bca850`](https://github.com/SmooAI/smooblue/commit/8bca8502a530880111786be675e5919fd4d6876e) Thanks [@brentrager](https://github.com/brentrager)! - Columns now have a settings panel (gear in the header). It slides in below the header and persists per column:

  - **Feed filters** (post columns): Hide reposts, Hide replies, Media only, Text only — applied client-side to the feed.
  - **Notification filter** (Notifications columns): show All, just Mentions (replies/mentions/quotes), or just Reactions (likes/reposts/follows).
  - **Refresh cadence**: Auto (the per-kind default), 15s, 30s, 60s, or Off to pause live polling for that column.

## 1.24.0

### Minor Changes

- [#65](https://github.com/SmooAI/smooblue/pull/65) [`b890c77`](https://github.com/SmooAI/smooblue/commit/b890c77f1d51f5fc0f22a80b5cd07517e098e23a) Thanks [@brentrager](https://github.com/brentrager)! - The post "…" action menu no longer gets clipped. It was rendered inside the post card, which has `contain: paint` and lives in a column with `overflow: hidden` — so the menu was cut off at the card edge. It's now rendered at the deck level and positioned `fixed` at the click point, floating above every column with its actions fully visible.

  Columns now have a "jump to top" pill. When you've scrolled down, a small pill appears near the top of the column; tapping it smooth-scrolls back to the top and resets the virtual viewport, so newly-polled posts (which accumulate above your read position) become visible live again.

## 1.23.0

### Minor Changes

- [#61](https://github.com/SmooAI/smooblue/pull/61) [`f8e4df2`](https://github.com/SmooAI/smooblue/commit/f8e4df294d17aeb26f4d4149cf000fdec8efe94d) Thanks [@brentrager](https://github.com/brentrager)! - Bare URLs now linkify and embed. Typing a domain without a scheme (`smoo.ai`, `google.com`, `docs.example.io/guide`) is now detected as a link — it becomes a clickable facet on the published post and feeds the link-card preview, matching Bluesky's own composer. Previously only `http(s)://`-prefixed URLs were detected, so a bare domain published as plain text with no card.

  The post "…" overflow button is now a real action menu. It used to just copy the link. It now opens a menu: Copy link, Open in browser, and — on your own posts — Delete (removes the post and hides it immediately); on others' posts — Mute, Block, and Report.

## 1.22.0

### Minor Changes

- [#58](https://github.com/SmooAI/smooblue/pull/58) [`5a44f85`](https://github.com/SmooAI/smooblue/commit/5a44f856ce7b836e96f89d6e765a0ca229659c44) Thanks [@brentrager](https://github.com/brentrager)! - Added an opt-in UI-automation bridge for scripted/headless testing. Set `SMOOBLUE_AUTOMATION=<port>` and the app opens a local (127.0.0.1-only) socket: send a line of JavaScript, get the JSON result back. It runs against the live webview via Dioxus' `document::eval`, so a test script can query elements, click them, read text, and assert state — the primitives UI tests are built from. This is the realistic equivalent of Playwright for a wry app, which can't be driven over Chrome's CDP (WKWebView / WebKitGTK don't speak it). Off by default; bound to localhost; never touches a normal user's run. Note: on macOS the idle Cocoa event loop only services requests when the window receives input — see the module docs for the focus-nudge workaround and the Linux/CI (Xvfb) note.

- [#58](https://github.com/SmooAI/smooblue/pull/58) [`5a44f85`](https://github.com/SmooAI/smooblue/commit/5a44f856ce7b836e96f89d6e765a0ca229659c44) Thanks [@brentrager](https://github.com/brentrager)! - Posting a URL now attaches a link card. When the composer text contains a link, smooblue fetches its OpenGraph metadata (title, description, thumbnail) via CardyB — the same extractor the official Bluesky app uses — and shows a preview card under the textarea with a remove (×). On post it's published as an `app.bsky.embed.external` embed (or `recordWithMedia` when you're also quoting a post), so your followers see a real card instead of a bare URL. The card is skipped automatically when you've attached an image or video, since those own the post's single media slot.

### Patch Changes

- [#58](https://github.com/SmooAI/smooblue/pull/58) [`5a44f85`](https://github.com/SmooAI/smooblue/commit/5a44f856ce7b836e96f89d6e765a0ca229659c44) Thanks [@brentrager](https://github.com/brentrager)! - Smoother column scrolling — fixed the "wiggle" when scrolling a feed, especially while new posts stream in. The virtualized lists (feeds + notifications) assumed every row was one fixed height, so on a mixed feed (a text post ~120px next to a 4-image grid + quote ~500px+) the scrollbar math drifted from the real layout and the browser re-corrected the scroll position every few rows. They now measure each row's real height and place the virtual window + spacers from those measurements, so the content stays put. Rows fall back to the per-kind estimate until they've been measured once.

- [#58](https://github.com/SmooAI/smooblue/pull/58) [`5a44f85`](https://github.com/SmooAI/smooblue/commit/5a44f856ce7b836e96f89d6e765a0ca229659c44) Thanks [@brentrager](https://github.com/brentrager)! - The @mention autocomplete now biases toward people you actually know. Bluesky's typeahead is only lightly personalized, so it buried mutuals under big strangers who happened to prefix-match. Results are now re-ranked: mutuals first, then people you follow, then people who follow you, then strangers — and within a tier a prefix match on the handle or display name beats a mid-string match. We fetch a wider candidate set and trim after ranking so a buried mutual can still surface.

  Fixed transparent backgrounds across the compose @mention dropdown, the DM/messages sheet, and inbox rows. These used CSS custom properties (`--color-surface`, `--color-fg`, etc.) that this theme never defines, so they resolved to transparent/inherited — you could read content straight through the mention popover. They now use the real theme tokens (`--card`, `--foreground`, `--muted`, `--muted-foreground`, `--border`).

- [#58](https://github.com/SmooAI/smooblue/pull/58) [`5a44f85`](https://github.com/SmooAI/smooblue/commit/5a44f856ce7b836e96f89d6e765a0ca229659c44) Thanks [@brentrager](https://github.com/brentrager)! - Quote posts now show the quoted post's media. A quoted **video** (or link card / record-with-media) used to render as just the author's name with no content — the quote card only knew how to draw nested _images_. It now renders video players, link cards, and record-with-media the same way a top-level embed does.

  Quote **notifications** now show the post that quoted you. "X quoted your post" was hydrating your own original post instead of X's quoting post, so the actual quote (with your post nested inside it) never appeared. Reply/mention/quote now consistently surface the inbound post.

## 1.21.0

### Minor Changes

- [#53](https://github.com/SmooAI/smooblue/pull/53) [`2c5e236`](https://github.com/SmooAI/smooblue/commit/2c5e236500cbfc6fbbcaf1d9b02e79b508326930) Thanks [@brentrager](https://github.com/brentrager)! - Inbox rows are now cards that show the actual message. Each interaction renders as
  a card with the triage actions tucked into the top-right corner (revealed on
  hover) instead of a column, and the preview shows the **real text the person
  wrote** — up to three lines — pulled straight from the notification's own post
  record (no extra fetch), instead of a generic "Replied to your post" caption.
  Quick reply stays one hover-and-click away in the card's action corner.

## 1.20.0

### Minor Changes

- [#51](https://github.com/SmooAI/smooblue/pull/51) [`87326c2`](https://github.com/SmooAI/smooblue/commit/87326c2224e95cbe46b99906fc75dce036c36602) Thanks [@brentrager](https://github.com/brentrager)! - Reply to inbox posts inline. The reply button on a post row now opens a compact
  quick-reply box right there — type a plain-text reply and send it without leaving
  the column (replying also marks the item read). Need images, links, or a quote?
  Hit **Pop out ↗** and your draft carries over into the full composer. DMs keep
  their existing inline reply; row-click still opens the full thread.

## 1.19.0

### Minor Changes

- [#49](https://github.com/SmooAI/smooblue/pull/49) [`19877fa`](https://github.com/SmooAI/smooblue/commit/19877faf2b582b3ad3566ccb4f093796cb2b8db6) Thanks [@brentrager](https://github.com/brentrager)! - Inbox rows now show a clout badge: the author's follower count plus a
  followers:following ratio (e.g. `12.4k · 8.3×`). A big follower count built by
  mass-following everyone reads as a low ratio and is dimmed, so you can tell real
  reach from follow-back inflation at a glance. The follows count is fetched in the
  same profile-enrichment pass that already powers the inbox's clout-aware sort.

## 1.18.2

### Patch Changes

- [#47](https://github.com/SmooAI/smooblue/pull/47) [`235b1f1`](https://github.com/SmooAI/smooblue/commit/235b1f1816d0b6f58b8c7a7187ec3db63f4c2e4c) Thanks [@brentrager](https://github.com/brentrager)! - Live feed no longer jumps while you're scrolled into it. When a column's
  background poll pulls in new posts, the feed now grows upward into the
  scrollback — the scrollbar lengthens at the top while your current read position
  stays exactly where it is. At the very top, fresh posts still appear in view as
  before.

## 1.18.1

### Patch Changes

- [`58bd65a`](https://github.com/SmooAI/smooblue/commit/58bd65a9ab53eaa18c7edb03f1cdebe91947b221) Thanks [@brentrager](https://github.com/brentrager)! - @mention popover now pops UP above the textarea instead of down. The compose dialog has no spare room below the textarea (Post button + attachment row sit there), so the popover was clipping past the dialog footer and getting overlapped by the Post button. Up has the headroom — the dialog header is short — and the popover lifts cleanly into that space. Also bumps the popover z-index past 50 and sets `overflow: visible` on the compose sheet so a tall suggestion list isn't clipped by the upstream modal's overflow rule.

## 1.18.0

### Minor Changes

- [`daeddcf`](https://github.com/SmooAI/smooblue/commit/daeddcf46ee90c6f632f02a4e597b6627e3394c2) Thanks [@brentrager](https://github.com/brentrager)! - @mention autocomplete in the compose sheet. Typing `@` (at start-of-line or after whitespace) opens a popover beneath the textarea with up to 8 actor suggestions from `app.bsky.actor.searchActorsTypeahead`, debounced 150ms so each keystroke doesn't fire a round-trip. Arrow Up/Down navigates, Enter or Tab inserts `@handle ` (preserving any text before the mention), Esc dismisses, click also works. Previously mentions were only resolved at post-time — typing `@al` and hitting Post would silently degrade to plain text if `al` didn't resolve to anyone.

  Only fires when the cursor is at the end of an active partial (no whitespace after the `@<chars>` run). Editing inside an existing word — including the mid-word `@` in an email address — doesn't accidentally pop the popover.

## 1.17.0

### Minor Changes

- [`de4fa9a`](https://github.com/SmooAI/smooblue/commit/de4fa9a18d011b7a829032a8a8d1973c4872eb8e) Thanks [@brentrager](https://github.com/brentrager)! - Virtualize the column body — only the rows within ~3 viewports of the visible area are mounted at a time, with top/bottom spacer divs preserving the scrollbar geometry as if all rows were rendered. Eliminates both failure modes the previous render path could hit on a 2000-row scrollback: the GPU tile-cache eviction that caused multi-second total blanks after deep scrollback (the original issue), and the `content-visibility: auto` per-card render gap that showed up as "blank cards have to load" after the column had been idle. Image bytes stay in WKWebView's image cache so re-mounting a row on scroll-back paints instantly.

  Each column kind has its own estimated row height (240px posts, 110px notifications, 90px inbox, 72px messages, 96px suggestions); the 2-viewport buffer above and below the visible area absorbs the variance. Scroll-anchor on the body excludes spacer divs so top-poll prepends still keep the user's view stable.

  Also drops the `content-visibility: auto` declarations added in 1.15.3 — virtualization caps the DOM size more aggressively than cv ever did.

## 1.16.1

### Patch Changes

- [`3e60acb`](https://github.com/SmooAI/smooblue/commit/3e60acbddbf08833fea51ce5fe40994d312fe6ec) Thanks [@brentrager](https://github.com/brentrager)! - Inbox per-row mark-as-read button is now always rendered (dims + disables itself once the row is read) instead of hiding when the row was already read. Previously the affordance disappeared the moment you marked anything, so users couldn't find it after hitting "Mark all as read" once. Tooltip also flips from "Mark as read" → "Read" to make the state explicit.

## 1.16.0

### Minor Changes

- [`b865129`](https://github.com/SmooAI/smooblue/commit/b865129a6aa0d859777d4e02004bf490721174ad) Thanks [@brentrager](https://github.com/brentrager)! - Notifications column now does infinite scroll — captures the `list_notifications` cursor (previously dropped on the floor), wired into the same `is_paginated` + scroll-geometry probe path as Home/Search/Feed/List/Author. Top-poll now merges new groups at the head (and grows existing same-key groups with new items) instead of wiping the column wholesale, so paginated scrollback survives the 15s refresh. Capped at 1000 groups per column (refuse-rather-than-evict, matching Posts policy).

  Inbox column gained a per-row "Mark as read" button (visible when the item is unread) plus a "Mark all as read" header action. Per-row flips the row's read styling immediately and persists in SQLite. Header action runs a single `UPDATE` over all active items then re-reads from disk so the column reflects the change without waiting for the next 15s poll.

## 1.15.2

### Patch Changes

- [`53fc3d3`](https://github.com/SmooAI/smooblue/commit/53fc3d3f5d658c6627322eb9210d7e6d35f58943) Thanks [@brentrager](https://github.com/brentrager)! - Fix scrolling-column blankness on tall feeds. Re-added `content-visibility: auto` + `contain-intrinsic-size` on `.post`, `.notif`, `.inbox-row__wrap`, and `.convo-row` so WebKit can skip painting off-screen cards in the 2000-row scrollback. Without it, the GPU tile cache evicts painted tiles at scrollback distance and re-rasterizing the rich post DOM produced multi-second blank slots (both text and images missing) during scroll. The previous removal traded this away for a sub-100ms entry flash — that's the smaller artifact, and `contain-intrinsic-size: auto <px>` lets WebKit cache each card's last-measured size so the scrollbar stays accurate. Also flipped post/notification/quote-card avatars from `loading="lazy"` to `loading="eager"` — avatars are tiny and always wanted, and lazy decode contributed its own pop-in on fast scroll.

## 1.15.1

### Patch Changes

- [`cec80a7`](https://github.com/SmooAI/smooblue/commit/cec80a78dd56aac0563cb931fdc2cb43eb7daf36) Thanks [@brentrager](https://github.com/brentrager)! - Removed Cmd+scroll-wheel zoom. Too easy to trigger by accident while scrolling a column with the Cmd key tap-held (focus pivots, modifier-key holdovers), which yanked the whole UI mid-scroll. ⌘+/⌘-/⌘0 keyboard shortcuts + the Settings → Appearance sliders cover the same zoom surface without the footgun. Updated the in-app hint to drop the "⌘+scroll wheel" callout.

## 1.15.0

### Minor Changes

- [`cb62de7`](https://github.com/SmooAI/smooblue/commit/cb62de70247d1050418277d4eb3ed6d19c1a3dc9) Thanks [@brentrager](https://github.com/brentrager)! - **Fixed: empty Inbox column on every install.** The ingestion task that polls listNotifications + listConvos and upserts triage rows was defined since v1.11 but never actually called from anywhere — so the Inbox column has been silently empty since the feature shipped. Wired up at App-mount via `use_hook`, so the first poll fires ~5s after launch and every 30s thereafter. The fact-finding tool that surfaced this: the new `diag.log` (shipped v1.14.0) was empty after a full session, meaning zero ingestion cycles had fired. Pearl th-4eb2f1 tracks adding a regression test so this can't silently regress again.

  **Infinite scroll on columns.** Scrolling near the bottom of any paginated column (Home, Search, Feeds, Lists, AuthorFeed) now auto-fetches the next page — no need to click the "Load more" button anymore. The button still renders as a fallback. Throttled internally so a fast scroll burst pre-warms the next page without firing N concurrent loads. Triggers ~600px before the end so the next page arrives before you actually hit it.

  **⌘0 now resets the full a11y surface.** Previously ⌘0 only reset text size; column width was left at whatever you'd dragged it to. Now ⌘0 mirrors the Settings → "Reset to defaults" button — text size back to 100%, column width back to 320px. Keeps the keyboard shortcut and the visible button writing the same value so they can't drift.

## 1.14.1

### Patch Changes

- [`5a0cbd4`](https://github.com/SmooAI/smooblue/commit/5a0cbd4d2cb57de92641164e7b2450f0a3ce24b7) Thanks [@brentrager](https://github.com/brentrager)! - When a column's 15–30 s top-poll inserts new posts at the head, your scroll position now stays anchored to whatever post you were reading. Previously the new content shifted everything down by its own height and you'd lose your place — a real pain on the Home column while reading a thread mid-scroll.

  CSS-only fix: explicit `overflow-anchor: auto` on `.deck-column__body` (default per spec but stating it makes the intent reviewable + protects against accidental override) plus `overflow-anchor: none` on the trailing loading / empty / error / load-more chrome so WebKit's anchor selection stays constrained to actual content cards.

  If this doesn't fully hold for you in practice (Dioxus diffing edge cases can defeat anchor-selection), I'll follow up with a JS scroll-math compensation that captures `scrollHeight` before the merge + bumps `scrollTop` by the delta after.

## 1.14.0

### Minor Changes

- [`98e58d1`](https://github.com/SmooAI/smooblue/commit/98e58d1505737452202f80b66e12ecc40d965fc2) Thanks [@brentrager](https://github.com/brentrager)! - UI accessibility prefs (text size + column width) now persist in SQLite instead of `ui_prefs.json`. Migration v3 adds a generic `settings` k/v table to the existing SQLite store; the database file also got renamed `inbox.db` → `smooblue.db` (auto-rename on first open) since it's no longer inbox-only. One-time migration reads the legacy JSON, upserts it into SQLite, and deletes the file so manual edits to the JSON can't resurrect stale state. Follow-up pearl th-feacc8 tracks moving the other small JSON/text files (theme, columns, draft, last_handle) to the same store.

  Also: **"Reset to defaults" button** in Settings → Appearance for the a11y sliders. One click puts text size back to 100% and column width back to 320px (the values you'd type ⌘0 to get for text alone; this fixes both at once).

## 1.13.0

### Minor Changes

- [`00a4e6b`](https://github.com/SmooAI/smooblue/commit/00a4e6bcd5d6a86e9aa7f90a6d124056c1ea8d4f) Thanks [@brentrager](https://github.com/brentrager)! - **MessagesSheet visual polish.** v1.12.0 shipped the inline DM thread with placeholder styling — bubbles used CSS vars that don't exist in Smooblue's theme (`--color-surface-alt`, `--color-brand`, etc.) so they rendered transparent and you could only tell a message was from you by right-alignment. Real implementation:

  - **Convo header** — partner's avatar + display name + `@handle` at the top of the sheet (fetched once per open via `chat_get_convo`, member-list filtered to the non-self party).
  - **Bubble colors that actually show**: self bubbles use smoo-orange brand, other bubbles use `--card` (subtle elevation against the body's `--background`). Self bubbles get a directional tail (bottom-right corner reduced); other bubbles get the same on bottom-left.
  - **Message grouping**: consecutive messages from the same sender within 5 min stack tightly together; mid-group bubbles keep full rounded corners (no tail). Mirrors iMessage/Slack/Telegram convention.
  - **Avatar on first-of-group only** for the other party — multi-message bursts don't repeat the avatar 5 times.
  - **Time chip on last-of-group only**, side-aligned (right for self, left for other) and padded past the avatar slot so it lines up under the bubble.
  - **Compose strip + header** on `--card` background, body on `--background`, so the chrome reads as separate from the conversation surface.

  CSS-only changes use Smooblue's actual theme vars (`--card`, `--background`, `--border`, `--foreground`, `--muted`, `--muted-foreground`, `--color-smooai-orange`, `--color-smooai-red`) instead of the invented ones in the v1.12.0 shipped version.

## 1.12.2

### Patch Changes

- [`9d49e30`](https://github.com/SmooAI/smooblue/commit/9d49e306b01183100a07099870a4751a41bb693d) Thanks [@brentrager](https://github.com/brentrager)! - Add file-based diagnostic logging. macOS's unified log drops plain `eprintln!`/stderr from GUI apps launched via Finder — only `os_log`/`NSLog` make it through. That left remote debugging stuck unless the user relaunched from terminal.

  Now every diagnostic line (currently from the inbox ingestion task; more sites to convert opportunistically) appends to `directories::data_dir/smooblue/diag.log`, rotated when it crosses 1 MB. Still mirrors to stderr so terminal launches show output too. Per-line write is open-append-close behind a parking_lot mutex — safe across threads, no buffer that loses content on crash.

  Practical effect: when the inbox shows empty for a user, we can ask them to `cat ~/Library/Application\ Support/ai.smoo.smooblue/diag.log | tail` and immediately see whether `pages=N ingested=M` or `listNotifications failed: …` is in the log.

## 1.12.1

### Patch Changes

- [`bdb4be3`](https://github.com/SmooAI/smooblue/commit/bdb4be3346bc2eeb0a1da79c3d651a3cc8c79344) Thanks [@brentrager](https://github.com/brentrager)! - **Fix accessibility zoom** (pearl th-459511 follow-up). v1.12.0 shipped the a11y feature using WebKit's `zoom` property — works but breaks scroll (the viewport doesn't grow with the content) and clips elements past the original window dimensions. Switching to **text-only scaling**:

  - Every `font-size: Npx` declaration in `assets/styles.css` is now wrapped in `calc(Npx * var(--font-scale, 1))` (130 selectors).
  - `App` sets `--font-scale` on `document.documentElement` instead of `zoom`.
  - Layout reflows naturally as text grows — columns get taller, you scroll to see more, no viewport clipping.
  - All keyboard / wheel / slider bindings work unchanged.

  Chrome (icons, padding, buttons) stays at native size, which matches what the user asked for: "control the size of text etc instead of webview zoom."

## 1.12.0

### Minor Changes

- [`c96af2a`](https://github.com/SmooAI/smooblue/commit/c96af2a0801feed58e16f6a16b873d6311ddd972) Thanks [@brentrager](https://github.com/brentrager)! - **Accessibility — browser-style zoom + column width** (pearl th-459511, from a real user request: _"Poor eyesight and I had trouble with the small text in posts… needed to expand the column width."_).

  - **⌘= / ⌘+** zoom in
  - **⌘-** zoom out
  - **⌘0** reset to 100%
  - **⌘ + scroll wheel** zoom (matches Chrome/Safari/Firefox UX)
  - **Settings → Appearance → Text size slider** (50% → 300%, 5% steps) for the discoverable path
  - **Settings → Appearance → Column width slider** (240px → 640px) for users who bumped text size and need wider columns

  Both persist across launches via a new `UiPrefs` JSON at `directories::config_dir/smooblue/ui_prefs.json`. Applied via `document.documentElement.style.zoom` (WebKit's native browser-zoom property — scales text, padding, layouts together, not just rem-based fonts) and a new `--column-width` CSS var on the deck-column flex-basis. Keyboard handler sits at the deck-shell root and short-circuits BEFORE the existing vim-chord dispatcher so the shortcuts take precedence regardless of what's focused.

## 1.11.1

### Patch Changes

- [`25b670e`](https://github.com/SmooAI/smooblue/commit/25b670e42766e34c83461989c8b4c58300df18e8) Thanks [@brentrager](https://github.com/brentrager)! - Inbox ingestion now paginates notification fetches. Was: one page of 50; up to 3 pages of 100 (up to 300 items per cycle). Fixes a real bug where a user's hour-old reply never made it into the Inbox because the first 50 notifications were dominated by likes / reposts / follows that we filter out. Cursor-follow bails as soon as the AppView stops returning more, so accounts with sparse history pay nothing extra.

  Inbox column read cap bumped 200 → 500 to match. Proper scroll-based lazy load across all paginated column types (Home / Search / Feed / List / AuthorFeed / Inbox) is tracked separately as pearl th-f5d4f4.

## 1.11.0

### Minor Changes

- [`9196554`](https://github.com/SmooAI/smooblue/commit/91965547d6926a76a149bbe7ab81dcdb7c0d0b7e) Thanks [@brentrager](https://github.com/brentrager)! - **Inbox follower-count tiebreak** (pearl th-bce4fb). Within the same hour bucket, items now sort by the actor's follower count first, then by directness. Across hour boundaries the directness + recency dominance stays intact, so a celebrity's old mention can't lift past a fresh direct reply — followers only matter for items arriving in the same time slot.

  New ORDER BY for the Inbox column:

  ```
  ORDER BY ts_bucket DESC,
           actor_follower_count DESC,
           directness DESC,
           ts DESC
  ```

  `ts_bucket` is `epoch_seconds / 3600` (hour granularity), stored at insert time as an INTEGER column rather than computed via SQLite's `strftime` so the index is stable and the bucketing rule lives in one Rust function (`inbox::ts_bucket_for`). Schema migration v2 adds `actor_follower_count` + `ts_bucket` columns (default 0, backfilled on next ingestion cycle) and replaces the active-list index with the new composite.

  Profile enrichment: ingestion task now collects distinct actor DIDs each cycle and batch-fetches their profiles via new `AtClient::get_profiles` (lexicon-spec'd at 25 actors per call; helper splits oversized inputs into chunks). Follower counts then go to `inbox::set_actor_followers(did, count)`, which UPDATEs every row authored by that actor — so a celebrity's follower bump lifts ALL their inbox rows, not just the latest.

  The UPSERT path uses `MAX(actor_follower_count, excluded.actor_follower_count)` so a transient profile-fetch failure (which writes 0) can't silently downgrade a real cached value — protection against a re-ingest with stale data clobbering correct enrichment.

  Idempotent + stable: same inputs → same order; new arrivals don't reorder existing items because each row's bucket is fixed at insert time.

- [`24a929e`](https://github.com/SmooAI/smooblue/commit/24a929ec238e983775b60a046b2caa075ef90d20) Thanks [@brentrager](https://github.com/brentrager)! - **Inbox — Phase A foundation** (pearl th-e17045). New triage column lives in the deck (`ColumnKind::Inbox`, rail button between Messages and the divider). Phase A ships the schema + persistence + render skeleton; Phase B (next release) wires ingestion so the column actually populates.

  What's in Phase A:

  - **`smooblue_app::inbox` module** — types, persistence, scoring. Backed by SQLite via `rusqlite` (bundled, no system dep) at `directories::data_dir/smooblue/inbox.db` with WAL journaling.
  - **Schema** with `device_id` + `synced_at` columns from day 1 so a future smoo.ai sync layer drops in without migration. Two indexes: `inbox_active_idx` (directness DESC, ts DESC, WHERE archived = 0) for the column read; `inbox_unsynced_idx` (WHERE synced_at IS NULL) for the future sync push set.
  - **Directness scoring**: Reply-to-your-reply (100) > DM (90) > Quote (70) > Direct reply (60) > Mention (30). Age decay (1pt per 12h, capped at 40). Unread bump (+20) so unread floats within band.
  - **CRUD API**: `upsert`, `list_active`, `set_read`, `set_archived`, `set_snoozed`, `unread_count`, `get`. UPSERT semantics on the insert so re-ingestion is naturally idempotent + preserves local triage state (read/archived/snoozed) when upstream payloads refresh.
  - **Column render** with `InboxRow` component — avatar, actor, source chip (reply/mention/quote/DM), preview, age, unread dot. Click routes to ThreadFocus (posts) or MessagesFocus (DMs). Read rows dim.

  Empty for now (no ingestion path yet). 4 new unit tests over schema migration + UPSERT round-trip + directness math.

- [`33a77d5`](https://github.com/SmooAI/smooblue/commit/33a77d56ddf926b9c01aba4153444a95e1906ef4) Thanks [@brentrager](https://github.com/brentrager)! - **Inbox — Phase B ingestion** (pearl th-e17045). The Inbox column now actually populates. Background tokio task polls `listNotifications` (replies / mentions / quotes) + `listConvos` (DMs from someone other than you) every 30s and UPSERTs into the SQLite store. The Inbox column's 15s read poll picks up new rows automatically.

  Mapping:

  - `notification.reason = "reply"` → `InboxSource::DirectReply`
  - `notification.reason = "mention"` → `InboxSource::Mention`
  - `notification.reason = "quote"` → `InboxSource::Quote`
  - `convo.last_message` where the sender isn't you → `InboxSource::Dm`
  - Likes / reposts / follows / starterpack-joined are filtered out (noise for triage)

  Stable `item_id` keys (`notif:{reason}:{cid}:{ts}` for posts, `dm:{convo_id}:{message_id}` for DMs) make re-polling idempotent — same notification on the next cycle just refreshes display fields via UPSERT, never duplicates the row or clobbers local triage state.

  **Race-fix bonus**: applied three fixes the adversarial-review pass caught on MessagesSheet (the DM thread view shipped earlier):

  1. **P1 — convo-switch race**: switching from convo A to B while A's `chat_get_messages` was still in flight could land A's messages on B's view. Added a stale-result guard that drops A's response if `focus` has since moved to B.
  2. **P2 — perpetual poll wakeup**: the 10s polling loop used `continue` when focus was None, keeping the wakeup live forever even with the sheet closed. Replaced with a conditional bump — still sleeps every 10s but no-ops when closed.
  3. **P2 — load-older race**: two near-simultaneous scroll events could both pass the `loading_older.read()` gate before either set it, then both fire `chat_get_messages` and stack duplicate older pages. Replaced naive read+set with `with_mut` atomic check-and-set.

  Phase C (triage actions + quick-reply) next.

- [`c6def58`](https://github.com/SmooAI/smooblue/commit/c6def58ac49b3866b1980502f9936c49618e83d6) Thanks [@brentrager](https://github.com/brentrager)! - **Inbox — Phase C triage actions** (pearl th-e17045). Three actions per row, visible on hover (Stripe-Inbox style), backed by the SQLite triage state shipped in Phases A/B:

  - **Archive** (X icon) — `inbox::set_archived(true)` + optimistic local hide. Row disappears immediately; next 15s column poll confirms persisted state from disk.
  - **Snooze** (Clock icon) — dropdown with 1h / 4h / Tomorrow / Monday. `inbox::set_snoozed(Some(when))`; row hides until the snooze elapses, then the column query's `WHERE snoozed_until IS NULL OR snoozed_until <= now()` re-surfaces it.
  - **Reply** (MessageQuote icon) — DMs expand an inline textarea + Send button (calls `chat_send_message`); posts open the existing ThreadSheet for full-fidelity composing (facets / images / quote — losing those for inline reply would be a regression).

  Row click marks the item read (`inbox::set_read(true)`) + opens ThreadFocus or MessagesFocus depending on source.

  **Adversarial-review P2 fix bundled**: `inbox::with_db` no longer silently downgrades to `:memory:` on disk-open failure. The OnceLock now holds `Option<Connection>`; if the open fails, the slot stays empty and subsequent calls retry (transient permission glitches self-heal). The CRUD methods propagate the error to the UI as a real failure rather than letting triage actions silently land in a throwaway DB.

## 1.10.0

### Minor Changes

- [`9920655`](https://github.com/SmooAI/smooblue/commit/9920655d9e0b53df42e066d0801e1dd5961ae13f) Thanks [@brentrager](https://github.com/brentrager)! - **Drag an image anywhere onto the Smooblue window** and compose opens with it attached. Previously only the compose sheet's textarea accepted drops — anywhere else on the window was a no-op. Now the deck-shell root has its own drop handler that routes accepted images through the same `FilePromiseEvent::Drop` channel the screenshot-floater overlay uses, so the App-level listener opens compose + attaches in the same flow regardless of where the drop landed. Compose's own drop handler now calls `stop_propagation` so drops landing INSIDE the open sheet don't double-attach.

  **Alt-text now truncates to 2000 characters** (Bluesky's `app.bsky.embed.images#image.alt` lexicon cap). The LLM auto-suggestion path was producing 3-4k-char scene descriptions that would have been rejected at submit time with a validation error; we now cap proactively in `AttachedImage::computed_alt`, in both alt-input `oninput` handlers (image + video), and via a `maxlength="2000"` on the textarea so user typing is also bounded.

## 1.9.0

### Minor Changes

- [`ca90d76`](https://github.com/SmooAI/smooblue/commit/ca90d760646719cad8d61f197499bebd13aa7c14) Thanks [@brentrager](https://github.com/brentrager)! - Scroll-up in MessagesSheet now loads older messages automatically — no "Load more" button. As you scroll within 150px of the top of a conversation, the next page from `chat.bsky.convo.getMessages` is fetched and prepended; `overflow-anchor: auto` on the bubble container keeps the visible content stationary so you don't get yanked back to the new top. A subtle "Loading older messages…" strip surfaces while a fetch is in flight; once the server runs out of cursor we latch a "Start of conversation." marker so the user knows there's nothing more.

  Also: auto-scroll-to-bottom whenever the conversation's TAIL grows (initial load, poll-discovered new message, your own sent message), so the latest message is always in view by default — without yanking you back to the bottom when you scrolled up on purpose to read older messages.

  Implementation note: `dioxus::document::eval` is used both for reading `scrollTop` on every scroll event (gate against firing the fetch when scroll position is mid-conversation) and for scrolling-to-bottom on tail growth. Naive per-frame eval is fine because the `loading_older` latch self-guards against concurrent fetches.

  New follow-up pearls filed alongside (rich-text/facets, embeds, message delete) — all P3/P4, none blocking.

## 1.8.0

### Minor Changes

- [`434b31e`](https://github.com/SmooAI/smooblue/commit/434b31e0f15a0452ca4ade094df31c1c00659631) Thanks [@brentrager](https://github.com/brentrager)! - Inline `MessagesSheet` — tap a row in the Messages column and the conversation opens in a slide-over without leaving Smooblue (pearl th-57e3c9). Reads message history, lets you send (⌘↵ to submit), and marks the convo as read on open so the unread badge clears.

  Wired:

  - **New context**: `MessagesFocus(Option<convo_id>)` mirrors `ThreadFocus`. Mounted in DeckShell next to `ThreadSheet`. `ConvoRow`'s onclick switched from "open in bsky.app" to `messages_focus.set(...)`.
  - **History loading**: `chat.bsky.convo.getMessages` on open + every 10s while the sheet stays open (Bluesky chat doesn't push). Messages reversed to render oldest-top, newest-bottom.
  - **Bubbles**: right-aligned + brand-colored for your own messages; left-aligned + surface-alt for the other member's. Deleted messages render as a muted center-aligned "(message deleted)" tombstone. Bubbles cap at 75% width so long messages wrap rather than stretching across the sheet.
  - **Send**: `chat.bsky.convo.sendMessage` with the typed draft. Server-canonical message appended to the on-screen list on success; the next poll will dedupe (same id). Failures surface as a red strip above the input rather than disappearing silently.
  - **Mark-as-read**: `chat.bsky.convo.updateRead` fires in the background on every load — failure is cosmetic (the unread count in the column clears at the next 30s poll instead of instantly).
  - **Timestamps**: `HH:MM` in local time on each bubble, parsed from the ISO-8601 the server returns.
  - **CSS**: `.messages__sheet` + bubble variants live in `assets/styles.css` alongside the convo-row styles.

  Limitations the next pearl can pick up: facets (mentions / hashtags / links) render as plain text; embeds (images, quoted records) aren't rendered yet; only first-page pagination — older messages aren't load-more-able. All tracked under the existing DM follow-up surface.

## 1.7.0

### Minor Changes

- [`2f96053`](https://github.com/SmooAI/smooblue/commit/2f96053c250b8344343462d42da19e2bfb83eaac) Thanks [@brentrager](https://github.com/brentrager)! - Add Bluesky DMs as a new "Messages" deck column (pearls th-b313df + th-34805b). Tap the chat icon in the rail to add it. Renders your conversations newest-first: avatar + display-name/handle of the other member, last-message preview (one line), and an unread badge when applicable. Tapping a row opens that thread on `bsky.app/messages/{convoId}` in your browser — the inline message-history sheet + send-message support land in a follow-up (th-57e3c9), but read-the-inbox-from-Smooblue already removes the "switch to browser to see if anything's there" friction.

  Under the hood:

  - **`smooblue-atproto::chat`** — new module wrapping `chat.bsky.convo.{listConvos,getConvo,getMessages,sendMessage,updateRead,getConvoForMembers}`. All chat requests route through the user's PDS with the `atproto-proxy: did:web:api.bsky.chat#bsky_chat` header (Bluesky's documented chat-routing path). `AtClient::get_json_proxied` / `post_json_proxied` are the new generic primitives — the DPoP + nonce-retry machinery moved into a shared `do_json` so all four call sites (proxied + unproxied, GET + POST) share one implementation.
  - **Types**: `ConvoView` / `MessageView` / `DeletedMessageView` / `MessageInput` / `ChatProfile` etc., with a `$type`-tagged `Message` enum that round-trips live and deleted messages distinctly. Facets and embeds modeled as `serde_json::Value` for v1 — we'll narrow types once the inline sheet starts rendering rich text + embeds.
  - **State + UI**: `ColumnKind::Messages` enum variant + `ColumnSpec::messages()` constructor; `ColumnData::Convos(Vec<ConvoView>)` rendered via a new `ConvoRow` component. Sidebar gets a `MessageCircle`-iconned button that adds (or focuses) the column. Poll cadence: 30s, matching Notifications.

  **Security/privacy doc updated** (README privacy table + `docs/Security/Security.md` "What's NOT done" item 6) to make explicit that Bluesky DMs are NOT end-to-end encrypted — Bluesky's chat service stores message bodies in plaintext and their operators/moderators can read them. Smooblue inherits this from the protocol; there is no Smooblue setting that changes it.

## 1.6.1

### Patch Changes

- [`fd288c7`](https://github.com/SmooAI/smooblue/commit/fd288c7f4be643ce4aeeae757547b637124a5573) Thanks [@brentrager](https://github.com/brentrager)! - Add drag-over highlight when dragging the screenshot floater (pearl th-d061c5). v1.6.0 shipped the file-promise drop itself, but the AppKit overlay intercepted the drag before the compose textarea's HTML5 `dragover` handler could fire — so the existing yellow `compose__sheet--drag` highlight never lit up. The user saw the image attach correctly on drop but had no visual feedback during the hover.

  Fix: the overlay now emits a `FilePromiseEvent::DragEnter` / `DragExit` / `Drop(path)` stream instead of just paths. App-level listener translates the stream into two Dioxus signals (`pending_drops` + `promise_drag_active`); ComposeSheet ORs `promise_drag_active` with the existing HTML5 `dragging` signal both when deciding the outer container's `--drag` class and when opening compose proactively on drag-enter (so the user sees the highlight as they hover, not just after they drop).

## 1.6.0

### Minor Changes

- [`5e8e1a4`](https://github.com/SmooAI/smooblue/commit/5e8e1a4e75737c3c77cba2c48504e6863455f195) Thanks [@brentrager](https://github.com/brentrager)! - Re-enable the macOS NSFilePromise drop overlay (pearl th-78d25c). The v1.5.0 crash root cause was a self-inflicted msg_send! with snake_case "selectors" (`set_autoresizing_mask:`, `register_for_dragged_types:`) that don't exist in Cocoa — ObjC threw `NSInvalidArgumentException: unrecognized selector sent to instance`, which propagated through Rust's FFI boundary as a foreign exception and aborted the process before the first frame rendered. Two-part fix in `file_promise.rs`:

  1. **Use the typed objc2-app-kit methods directly** (`setAutoresizingMask`, `registerForDraggedTypes`, `addSubview_positioned_relativeTo`) instead of hand-rolled msg_send wrappers — the bindings know the real Cocoa selectors.
  2. **Defense in depth**: wrap install in `std::panic::catch_unwind` + `objc2::exception::catch` + `objc2::rc::autoreleasepool`, so any future installation bug degrades to a logged error rather than aborting the process. Every step `eprintln!`s its progress, so the CI smoke-launch job + user terminal launches expose exactly where any future install failure happens (the previous `tracing::warn!` calls were silent because Smooblue has no tracing subscriber).

  Behavior for the user: screenshot floater drag onto the Smooblue window should now resolve the promise via `NSFilePromiseReceiver.receivePromisedFilesAtDestination:`, write the image to the OS temp dir, open compose with it attached. The new CI smoke-launch job verifies launch survives before this ships.

## 1.5.1

### Patch Changes

- [`f088138`](https://github.com/SmooAI/smooblue/commit/f08813802bac669ac97a88446caba933de460adb) Thanks [@brentrager](https://github.com/brentrager)! - **Hotfix**: disable the v1.5.0 file-promise drop overlay — it crashed Smooblue on launch with `__rust_foreign_exception` → `abort()`. My AppKit code in `file_promise::install_on_main_window` threw an ObjC exception during initial render that propagated through Dioxus' component creation path and aborted the Rust runtime. Module stays compiled and wired so a proper fix can land without a wider revert. Tracked as th-78d25c.

  This reverts the user-visible behavior to v1.4.1 (paste works, drag-from-Finder works, screenshot-floater drag still requires ⌘C → ⌘V workaround). The proper fix needs GUI-test verification before reshipping — I shouldn't have shipped v1.5.0 without it.

## 1.5.0

### Minor Changes

- [`607ea48`](https://github.com/SmooAI/smooblue/commit/607ea48fb6e09843f6afbc5ad30c8462837c5b1c) Thanks [@brentrager](https://github.com/brentrager)! - Add macOS file-promise drop support — the screenshot floater (the thumbnail bottom-right after ⇧⌘4) can now be dragged directly onto the Smooblue window to attach to a post. Previously this did nothing because Wry's drag handler can only resolve `public.file-url` items, and the floater drags an unresolved `NSFilePromiseProvider` (the file doesn't exist on disk until the floater dismisses).

  The fix attaches an invisible overlay NSView to the main window's content view, registered for `com.apple.NSFilePromiseProvider` drag types **only** — so existing drag UX (Finder file drops, web-content drag/drop) is untouched. When a promise drop lands, the overlay resolves the file via `NSFilePromiseReceiver.receivePromisedFilesAtDestination:` into the OS temp directory and forwards the path through a tokio channel into the same image-attachment pipeline that drag-drop and the file picker use. If you drop while the compose sheet is closed, it flips open with the image attached. Linux + Windows: no-op stub (paths there don't go through NSFilePromise).

  Pearl: th-2c71e1.

## 1.4.1

### Patch Changes

- [`ff5c11d`](https://github.com/SmooAI/smooblue/commit/ff5c11dbf5aa2a41db57f6ecec3aa7bc8c3136b4) Thanks [@brentrager](https://github.com/brentrager)! - No-op patch bump to smoke-test the PAT-driven auto-tag flow (pearl th-5b49e0). The previous publish path relied on `pull_request_target:closed` firing from a GITHUB_TOKEN-authored auto-merge — which GitHub's anti-loop guard silently suppressed. Auto-merge now runs under a fine-grained PAT (`RELEASE_PAT`) so the merge commit is attributed to a real user and the downstream event fires normally. If this changeset rides through to v1.4.1 hands-off (Release PR opens → CI passes → auto-merge → publish job fires → v1.4.1 tag → release.yml builds + ships .app/.deb/.tar.gz + brew tap bumps), the fix is verified.

## 1.4.0

### Minor Changes

- [`7874abd`](https://github.com/SmooAI/smooblue/commit/7874abd148b0656ae769e5e75d658655afd778ab) Thanks [@brentrager](https://github.com/brentrager)! - Add ⌘V paste-image-from-clipboard support to the compose sheet. The textarea now intercepts ⌘V / Ctrl+V, reads the clipboard via `arboard`, and if there's an image there, PNG-encodes it and funnels it through the same prep / OCR / LLM-alt-text pipeline the file picker and drag-drop use. The textarea's native text-paste behavior still runs, so pasting plain text works unchanged.

  Why this matters: the macOS screenshot floater (the thumbnail bottom-right after a ⇧⌘4 capture) drags an `NSFilePromise` — the file hasn't been written to disk yet, and Wry's `DragDrop` event can't resolve promise items, so dropping the floater onto the compose sheet did nothing. The only escape was clicking the floater to dismiss it, then dragging the saved file from Finder. With paste support, ⌘C the floater (or just paste any image from anywhere) and it attaches directly. Same fix benefits Linux + Windows (paste-from-clipboard is expected UX everywhere, drag wasn't covering it).

## 1.3.2

### Patch Changes

- [`2a879ce`](https://github.com/SmooAI/smooblue/commit/2a879ce398b977987aba64aaea4863dc00e7db8f) Thanks [@brentrager](https://github.com/brentrager)! - Homebrew cask now auto-strips macOS quarantine on install. Without this, macOS Sequoia's Gatekeeper refuses to launch the adhoc-signed `.app` with "Apple could not verify Smooblue is free of malware" and offers no GUI "Open Anyway" button — the only escape was a terminal `xattr` command, which defeats the point of a one-line cask install. The cask now runs `xattr -cr` in a `postflight` block so `brew install --cask smooblue` (and `brew upgrade --cask smooblue`) launch cleanly on first try. Direct .zip downloads from a GitHub release are NOT modified — those still need the manual one-liner, documented in the README's Install section + the Security doc's "What's NOT done" list. Real fix (Apple Developer ID enrollment + notarization) tracked as a follow-up; held until the $99/yr cost is justified by usage.

## 1.3.1

### Patch Changes

- [`8f8d593`](https://github.com/SmooAI/smooblue/commit/8f8d5938308d2eda680eb16dfac733220e6ee817) Thanks [@brentrager](https://github.com/brentrager)! - Release notes on GitHub now lead with install + upgrade commands (Homebrew, .deb, manual) and end with an asset table — so anyone landing on a release page from an "update available" link gets a self-serve guide instead of a bare changeset list. The changelog body is unchanged; it's wrapped by a new `scripts/build-release-notes.sh` that `release.yml` calls when a tag fires. The same script can be run locally to retroactively re-render older releases (`./scripts/build-release-notes.sh 1.2.2 CHANGELOG.md > /tmp/n.md && gh release edit v1.2.2 --notes-file /tmp/n.md`).

## 1.3.0

### Minor Changes

- [`29da70c`](https://github.com/SmooAI/smooblue/commit/29da70cc386dd3b65dbff39d84b50b1965f20288) Thanks [@brentrager](https://github.com/brentrager)! - Search is now live results, not a column-builder. Typing in the search sheet fires a debounced `searchActorsTypeahead` + `searchPosts` in parallel; results appear in two stacked sections (Users + Posts). Clicking a user row opens their profile sheet; clicking a post row opens the thread. Each user row also has a "+ column" button to pin them as an author-feed column. The "Add as search column" footer button is still there if you want to materialise the current query as a permanent column — the old behaviour is now opt-in rather than the only option.

## 1.2.2

### Patch Changes

- [`22ca936`](https://github.com/SmooAI/smooblue/commit/22ca93672d8d505042ab0d6f6b02ae8de8e0c1ab) Thanks [@brentrager](https://github.com/brentrager)! - Fix the home / feed column scroll-flash. Earlier we added `content-visibility: auto` on `.post` and `.notif` to skip rendering of off-screen cards — great for the deep-thread scroll case it was added for, but on fast-scrolling feed columns it meant each card entering the viewport flashed blank briefly while WebKit's async content-visibility paint caught up. Dropped `content-visibility: auto` (and the associated `contain-intrinsic-size`) and kept the cheap `contain: layout style paint` per-card isolation. The original deep-thread flashing issue was actually image-decode reflows, which we already fix separately with per-image `aspectRatio` on embeds + the 16:9 CSS fallback — so we don't need content-visibility to solve it.

- [`2fbb0f5`](https://github.com/SmooAI/smooblue/commit/2fbb0f58e86988f73a4328d71c034141e6cdcbe3) Thanks [@brentrager](https://github.com/brentrager)! - Expand the security doc with a "Post-authentication: what protects your content in transit and at rest" section that walks through the three layers separately (TLS = transport, DPoP = per-request authenticity, AT Protocol = the honest "posts are public by design" content model). Adds explicit notes on DM support (intentionally none today; Bluesky hasn't shipped E2EE for chat yet), draft persistence on disk, and what Smooblue does NOT do with your content (no analytics, no third-party forwarding, no crash uploads). TL;DR table updated with rows for per-request authenticity, public-post content, and DMs so the reader gets the shape before drilling in.

- [`bab88b1`](https://github.com/SmooAI/smooblue/commit/bab88b18573aa069f8ab7e248d625d5a2e406294) Thanks [@brentrager](https://github.com/brentrager)! - Add a comprehensive security writeup at `docs/Security/Security.md` — auth model (PAR + PKCE + DPoP, why this is stronger than app passwords), transport (rustls TLS, no insecure fallbacks), the complete data egress table, URL hardening, what browser security extensions buy you vs don't, the process / sandboxing model, and an honest "what's NOT done" section (adhoc signing, no App Sandbox, plaintext session file, no SRI on auto-updater). Linked from the README and from Settings → About so users can find it in-app.

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
