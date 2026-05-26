# Changelog

All notable changes to Smooblue. Generated from conventional-commit
messages by release-plz; see [release-plz.dev](https://release-plz.dev)
for the format.

## [1.0.0] - 2026-05-26

### Added
- Notification cards use head-row + full-width body (deck.blue layout)
- Reclaim avatar-rail dead space — body flows full card width
- Sidebar nav scrolls to + flashes already-open column
- Debounced filter + avatar profile slot + Search-posts on profile
- Per-column fuzzy text filter
- Rich text — mentions/links/hashtags clickable in post body
- Full PostCard for inbound notification quotes (like/repost/quote/reply + counts)
- Scroll focused post into view on thread open
- Quote embeds click through to thread + stack name above handle
- Column scrollback grows — top-poll merges, Load more appends, capped at 2000
- Stacked name+handle layout + in-app lightbox for images/video
- 1.0 cut — video compose, Borg-cybernetic icon, README + demo **(BREAKING)**
- 'Your feeds' in column picker + Add Column opens the rich picker
- Multi-account switching
- Thread compose (chain replies into a self-thread)
- Profile editor (display name, bio, avatar, banner)
- Trending topics + popular feeds browser
- Mute & block management in settings
- Pinned post on profile
- Light theme with persistent toggle
- Self-update notifier + report account flow
- Quote-post compose (app.bsky.embed.record + recordWithMedia)
- Content-label warning interstitials on labeled posts
- Vim/nvim-style keyboard shortcuts + ? help overlay
- Lists picker — add your curated lists as columns
- Mute + block actions on profile sheet
- Rich-text facets in compose — mentions, links, hashtags
- Saved feeds picker + settings panel + lists column + own-profile + subject-post cache + login emblem fix
- Suggested follows column (app.bsky.actor.getSuggestions)
- Compose drafts persist across launches
- Mark notifications as read + unread badge on sidebar

### Changed
- Revert "feat: reclaim avatar-rail dead space — body flows full card width"
- Harden — URL scheme allowlist, video size cap, updater log + run-guard, tests
- Fix cargo clippy warnings under rust 1.95
- Apply cargo fmt --all
- Release v0.1.0 ([#8](https://github.com/SmooAI/smooblue/pull/8))
- Video playback: real HLS player via WKWebView native decode
- Notifications grouping: collapse N likes/reposts/follows into one card
- Mutuals + tap-the-count: likes / reposters / quotes / known-followers
- Profile sheet: banner + avatar + follow/unfollow + recent posts
- Drag-to-reorder columns + skeleton mac signing/notarization
- Scripts/bundle-macos.sh → distributable Smooblue.app
- Refresh-token rotation — sessions survive past the 2h access-token TTL
- Smarter alt-text chip — Combined / AI / OCR / Use AI
- Thread view: click any post → modal with parents + focused + replies tree
- Rich-media renderer + notification subjects show full text + nested embeds
- Hydrate reason_subject + render quoted post under each card
- Apple Vision OCR auto-fills alt-text alongside LLM
- Debug env vars for visual iteration without OS picker
- LLM-suggested auto alt-text via smoo.ai vision endpoint
- Image attachments — picker, thumbnails, alt-text inputs, blob upload
- Polish compose: progress ring + ⌘↵ + bigger textarea + brand-gradient post
- Repost (optimistic) + reply UX + unified compose context
- Optimistic likes: tap heart, instant flip, server reconciles
- Add columns from sidebar + Search column + clickable avatars + close column
- Auto-promote fresh items + ticking timestamps + callback logo
- Live deck: real compose + per-column polling + new-posts banner
- Login UX: pass handle to OAuth screen + remember last handle
- Brand refresh: cartoon butterfly icon + Smooblue wordmark
- Fix 401 AuthMissing + redesign OAuth callback pages + smooblue-branded
- Sweep .login__input + .compose__textarea to shared .input class
- Adopt smooai-ui shared design system (github.com/SmooAI/ui)
- Add opt-in Smoo AI CRM sync (smooblue-crm crate)
- Initial commit: smooblue — Rust/Dioxus multi-column Bluesky client

### Documentation
- Obsidian vault + pearls init + /save-status command + AGENTS.md

### Fixed
- Single-flight refresh — concurrent column polls were racing tokens
- Engagement sheet use_resource captured 'focus' by value
- Pause inline videos when they scroll out of view
- Inline video playback (drop CORS preflight + no-referrer attrs)
- Session storage to file (adhoc-sign churn nuked keychain), prune dup nav
- Saved-feeds sheet use_resource captured 'open' by value
- Activate as foreground app on launch so hotkey tools work
- Persist refreshed session to both keyring slots so auth survives restart
- Image-post lexicon key, drag-drop attach, timestamp permalink
- Defensive feed-item decode; brand mark butterfly-primary
- Duplicate-key crash + reply / repost context chips
- Clickable notifications, dead-button audit, profile/thread reactivity

### Performance
- TimeAgo extraction + lazy image loading + adversarial-review fixes


## [0.1.0] - 2026-05-24

### Changed
- Video playback: real HLS player via WKWebView native decode
- Notifications grouping: collapse N likes/reposts/follows into one card
- Mutuals + tap-the-count: likes / reposters / quotes / known-followers
- Profile sheet: banner + avatar + follow/unfollow + recent posts
- Drag-to-reorder columns + skeleton mac signing/notarization
- Scripts/bundle-macos.sh → distributable Smooblue.app
- Refresh-token rotation — sessions survive past the 2h access-token TTL
- Smarter alt-text chip — Combined / AI / OCR / Use AI
- Thread view: click any post → modal with parents + focused + replies tree
- Rich-media renderer + notification subjects show full text + nested embeds
- Hydrate reason_subject + render quoted post under each card
- Apple Vision OCR auto-fills alt-text alongside LLM
- Debug env vars for visual iteration without OS picker
- LLM-suggested auto alt-text via smoo.ai vision endpoint
- Image attachments — picker, thumbnails, alt-text inputs, blob upload
- Polish compose: progress ring + ⌘↵ + bigger textarea + brand-gradient post
- Repost (optimistic) + reply UX + unified compose context
- Optimistic likes: tap heart, instant flip, server reconciles
- Add columns from sidebar + Search column + clickable avatars + close column
- Auto-promote fresh items + ticking timestamps + callback logo
- Live deck: real compose + per-column polling + new-posts banner
- Login UX: pass handle to OAuth screen + remember last handle
- Brand refresh: cartoon butterfly icon + Smooblue wordmark
- Fix 401 AuthMissing + redesign OAuth callback pages + smooblue-branded
- Sweep .login__input + .compose__textarea to shared .input class
- Adopt smooai-ui shared design system (github.com/SmooAI/ui)
- Add opt-in Smoo AI CRM sync (smooblue-crm crate)
- Initial commit: smooblue — Rust/Dioxus multi-column Bluesky client

