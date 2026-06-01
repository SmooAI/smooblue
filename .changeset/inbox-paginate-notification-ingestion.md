---
"smooblue": patch
---

Inbox ingestion now paginates notification fetches. Was: one page of 50; up to 3 pages of 100 (up to 300 items per cycle). Fixes a real bug where a user's hour-old reply never made it into the Inbox because the first 50 notifications were dominated by likes / reposts / follows that we filter out. Cursor-follow bails as soon as the AppView stops returning more, so accounts with sparse history pay nothing extra.

Inbox column read cap bumped 200 → 500 to match. Proper scroll-based lazy load across all paginated column types (Home / Search / Feed / List / AuthorFeed / Inbox) is tracked separately as pearl th-f5d4f4.
