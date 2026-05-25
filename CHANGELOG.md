# Changelog

All notable changes documented here. release-plz writes new entries
automatically when a release PR merges; the initial 1.0 entry below
was authored by hand to capture the foundation.

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
- **Compose** — text, replies, quotes, self-threading, images (up to
  4) with auto-alt-text (Apple Vision OCR + LLM scene description),
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
