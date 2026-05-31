---
"smooblue": minor
---

**Inbox — Phase B ingestion** (pearl th-e17045). The Inbox column now actually populates. Background tokio task polls `listNotifications` (replies / mentions / quotes) + `listConvos` (DMs from someone other than you) every 30s and UPSERTs into the SQLite store. The Inbox column's 15s read poll picks up new rows automatically.

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
