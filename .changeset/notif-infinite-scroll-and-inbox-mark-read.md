---
"smooblue": minor
---

Notifications column now does infinite scroll — captures the `list_notifications` cursor (previously dropped on the floor), wired into the same `is_paginated` + scroll-geometry probe path as Home/Search/Feed/List/Author. Top-poll now merges new groups at the head (and grows existing same-key groups with new items) instead of wiping the column wholesale, so paginated scrollback survives the 15s refresh. Capped at 1000 groups per column (refuse-rather-than-evict, matching Posts policy).

Inbox column gained a per-row "Mark as read" button (visible when the item is unread) plus a "Mark all as read" header action. Per-row flips the row's read styling immediately and persists in SQLite. Header action runs a single `UPDATE` over all active items then re-reads from disk so the column reflects the change without waiting for the next 15s poll.
