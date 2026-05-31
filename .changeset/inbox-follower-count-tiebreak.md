---
"smooblue": minor
---

**Inbox follower-count tiebreak** (pearl th-bce4fb). Within the same hour bucket, items now sort by the actor's follower count first, then by directness. Across hour boundaries the directness + recency dominance stays intact, so a celebrity's old mention can't lift past a fresh direct reply — followers only matter for items arriving in the same time slot.

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
