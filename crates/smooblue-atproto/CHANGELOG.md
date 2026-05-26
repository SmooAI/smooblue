# Changelog

All notable changes to Smooblue. Generated from conventional-commit
messages by release-plz; see [release-plz.dev](https://release-plz.dev)
for the format.

## [1.0.0] - 2026-05-26

### Added
- Rich text — mentions/links/hashtags clickable in post body
- 1.0 cut — video compose, Borg-cybernetic icon, README + demo **(BREAKING)**
- 'Your feeds' in column picker + Add Column opens the rich picker
- Profile editor (display name, bio, avatar, banner)
- Trending topics + popular feeds browser
- Mute & block management in settings
- Pinned post on profile
- Self-update notifier + report account flow
- Quote-post compose (app.bsky.embed.record + recordWithMedia)
- Content-label warning interstitials on labeled posts
- Lists picker — add your curated lists as columns
- Mute + block actions on profile sheet
- Rich-text facets in compose — mentions, links, hashtags
- Saved feeds picker + settings panel + lists column + own-profile + subject-post cache + login emblem fix
- Suggested follows column (app.bsky.actor.getSuggestions)
- Mark notifications as read + unread badge on sidebar

### Changed
- Harden — URL scheme allowlist, video size cap, updater log + run-guard, tests
- Fix cargo clippy warnings under rust 1.95
- Apply cargo fmt --all
- Release v0.1.0 ([#8](https://github.com/SmooAI/smooblue/pull/8))
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

### Documentation
- Obsidian vault + pearls init + /save-status command + AGENTS.md

### Fixed
- Saved feeds picker handles both V1 + V2 preferences shape
- Image-post lexicon key, drag-drop attach, timestamp permalink
- Defensive feed-item decode; brand mark butterfly-primary
- Duplicate-key crash + reply / repost context chips


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

