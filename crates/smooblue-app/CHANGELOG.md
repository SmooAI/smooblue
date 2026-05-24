# Changelog

All notable changes to Smooblue. Generated from conventional-commit
messages by release-plz; see [release-plz.dev](https://release-plz.dev)
for the format.

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

