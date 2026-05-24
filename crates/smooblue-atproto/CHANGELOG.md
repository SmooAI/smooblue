# Changelog

All notable changes to Smooblue. Generated from conventional-commit
messages by release-plz; see [release-plz.dev](https://release-plz.dev)
for the format.

## [0.1.0] - 2026-05-24

### Changed
- Notifications grouping: collapse N likes/reposts/follows into one card
- Mutuals + tap-the-count: likes / reposters / quotes / known-followers
- Harden — body-read error logging + parking_lot Mutex (no poison)
- Profile sheet: banner + avatar + follow/unfollow + recent posts
- Refresh-token rotation — sessions survive past the 2h access-token TTL
- Thread view: click any post → modal with parents + focused + replies tree
- Rich-media renderer + notification subjects show full text + nested embeds
- Hydrate reason_subject + render quoted post under each card
- Image attachments — picker, thumbnails, alt-text inputs, blob upload
- Repost (optimistic) + reply UX + unified compose context
- Optimistic likes: tap heart, instant flip, server reconciles
- Add columns from sidebar + Search column + clickable avatars + close column
- Live deck: real compose + per-column polling + new-posts banner
- Fix 401 AuthMissing + redesign OAuth callback pages + smooblue-branded
- Adopt smooai-ui shared design system (github.com/SmooAI/ui)
- Add opt-in Smoo AI CRM sync (smooblue-crm crate)
- Initial commit: smooblue — Rust/Dioxus multi-column Bluesky client

