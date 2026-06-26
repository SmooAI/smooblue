# Account Analytics — Data Sources & Findings

#engineering #idea

Research notes for a potential **account analytics** feature (e.g. a
"followers vs following over time" chart, growth stats). Captures where
the time-series data actually comes from, because Bluesky's API does
**not** expose historical counts — it has to be reconstructed.

Findings gathered 2026-06-26 while building a one-off growth chart for
`@rager.tech` (1,108 followers / 1,340 following at the time).

---

## The core problem

`app.bsky.actor.getProfile` returns only **current** `followersCount` /
`followsCount`. There is no first-party "counts over time" endpoint.
"Over time" must be reconstructed from the follow **records**, each of
which carries its own creation timestamp.

Two important truths:

- A follow is a record (`app.bsky.graph.follow`) with a `createdAt` in
  its value **and** an rkey that is a [TID](#tid-decoding) (also a
  timestamp). Either gives you "when this follow happened."
- Reconstruction yields a **cumulative growth curve**, not a true net
  count over time: unfollows delete the record, so they don't appear as
  dips. The curve counts follows that *still exist*, by their creation
  date.

---

## Following over time — easy & exact

The signed-in user's own follows live in their repo. List them straight
off the user's PDS (the AppView returns **501** for `listRecords` — it's
a PDS method, not an AppView one):

```
GET {pds}/xrpc/com.atproto.repo.listRecords
    ?repo={did}&collection=app.bsky.graph.follow&limit=100   (paginate via cursor)
```

Each record's `value.createdAt` = when you followed that account.
Complete and cheap (~14 pages for 1,340 follows). Resolve `{pds}` from
the DID doc (`https://plc.directory/{did}` → `AtprotoPersonalDataServer`
service endpoint).

We already do PDS-targeted XRPC in `smooblue-atproto` — this is one more
paginated call.

---

## Followers over time — the hard part

There is **no first-party bulk way** to learn "when did each person
follow me." Three approaches, worst → best:

### 1. `app.bsky.graph.getFollowers` — incomplete

AppView method, paginates, but:

- Returns **no follow timestamp** (just profile views, newest-first).
- Only enumerates a **subset**: for `@rager.tech` it exhausted at
  **515** of a displayed **1,108**. The gap is deactivated / suspended /
  blocked accounts that the *count* includes but the *list* won't return.

Useful only for "who currently follows me," not history.

### 2. Per-follower PDS crawl — slow, partial

For each follower DID, scan their `app.bsky.graph.follow` records for one
with `subject == myDid`, read its `createdAt`. Reality:

- ~1,100+ requests minimum, plus a PDS resolution per follower.
- A follower who follows thousands needs many pages to find your record
  (it's ordered by *their* follow time, not yours). A page cap (we used
  12, then 45) bounds cost but **misses heavy-following accounts** →
  only ~30–65% date coverage, minutes of runtime, rate-limit prone.

Works, but a bad foundation for a real feature.

### 3. Constellation backlink index — **the winner**

[Constellation](https://constellation.microcosm.blue) (by microcosm.blue)
is a **network-wide AT Proto backlink index**. It answers "what records
point at this target" directly — including every `follow` whose
`.subject` is your DID — without touching individual PDSes. This is the
method the public analysis sites use.

```
GET https://constellation.microcosm.blue/links
    ?target={did}&collection=app.bsky.graph.follow&path=.subject   (paginate via cursor)
→ { total, linking_records: [ { did, collection, rkey }, ... ], cursor }

GET .../links/count[/distinct-dids]?target=...&collection=...&path=...
→ { total }
```

For `@rager.tech`: **1,047** incoming follows / **1,039 distinct dated
followers, 0 undatable rkeys** — vs 515 (getFollowers) and 333 (crawl).
~94% of the displayed 1,108, in seconds.

Each `linking_record.rkey` is a TID → decode to the follow timestamp
(no need to fetch the record body).

Caveats: third-party service (availability + an egress dependency —
weigh against [[../Decisions/ADR-002-Safe-Open-Allowlist|the egress
posture]]); coverage depends on its backfill; still a growth curve.

---

## TID decoding

A follow record's rkey is a TID: a 13-char, base32-sortable, 64-bit
value where the high bits are **microseconds since the Unix epoch** and
the low 10 bits are a clock id.

```python
ALPHABET = "234567abcdefghijklmnopqrstuvwxyz"  # base32-sortable
def tid_to_micros(tid: str) -> int:
    n = 0
    for c in tid:
        n = n * 32 + ALPHABET.index(c)
    return n >> 10   # drop the 10-bit clock id → microseconds since epoch
```

Verified: `3mp5c2tkblw2p` → 2026-06-25T20:40:44Z (newest follower);
a known reply post's rkey decoded to its true creation date.

---

## If we build this into the app

- **Backfill** the historical curve once on first open: own repo for
  following (exact), Constellation for followers (rkey → date).
- **Snapshot forward**: also record `(timestamp, followersCount,
  followsCount)` from `getProfile` on a cadence into the local SQLite
  store, so the *displayed* count (incl. non-enumerable accounts) gets a
  true net series going forward — backfill seeds the past, snapshots
  give the accurate future.
- New XRPC client methods: `listRecords` for own follows (PDS-targeted),
  a small Constellation client (`/links`, `/links/count`). See
  [[Adding-an-XRPC-Endpoint]].
- Render with the existing chart approach (one-off used Chart.js in a
  standalone HTML; in-app would be a native Dioxus view / inline SVG).
- Be explicit in the UI that reconstructed curves are **growth curves**
  (no unfollow dips) and that follower coverage is ~the indexed subset,
  not the raw displayed count.

---

## Related

- [[Adding-an-XRPC-Endpoint]] — how to add the `listRecords` / Constellation calls
- [[Adding-a-Column-Type]] — if analytics ships as a column kind
- [[../Architecture/Architecture-Overview]] — where a new view/store would live
