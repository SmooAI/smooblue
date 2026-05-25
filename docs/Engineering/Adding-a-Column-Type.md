# Adding a Column Type

#engineering

Every column in the deck is one of `ColumnKind`. Adding a new kind means three coordinated edits.

---

## 1. `crates/smooblue-app/src/state.rs`

Add a variant to `ColumnKind` + a `ColumnSpec::your_kind(...)` constructor + the icon mapping in `ColumnHeader`. Keep the `ColumnSpec::id` deterministic and unique per concrete column (URI-based for feeds, kind-only for singletons like Home).

---

## 2. `crates/smooblue-app/src/components/column.rs`

Two touch points in this file:

- `fetch_once`: add a `match` arm for your new kind that returns the right `ColumnData` (Posts / Notifications / Suggestions) by calling the corresponding `AtClient` method.
- `poll_interval`: pick a sensible refresh cadence. Notifications: 25s. Custom feeds: 25s. Suggested follows: 5min (changes slowly).

Make sure the `key:` on the rendered card disambiguates duplicates — feed pages can repeat the same post URI if two reposters surface it. See `feed_item_key()` for the precedent.

---

## 3. Add it to the picker

`crates/smooblue-app/src/components/saved_feeds_sheet.rs` is the "+ Add column" sheet. If your column maps to a bsky concept (feed generator, list, account), surface it as a new section or as part of an existing one (Your feeds / Lists / Trending / Popular).

---

## 4. Tests

In `client.rs`, add a wiremock test for the new XRPC endpoint. In `feed.rs` (or wherever the new lexicon type lives), add a serde decode test for the response shape.

---

## Worked example

The "Your feeds" section in 1.0 added these:
- `AtClient::list_own_feed_generators` (new XRPC method — see [[Adding-an-XRPC-Endpoint]])
- `SavedFeedsSheet::Loaded::own_feeds` field
- `PopularFeedRow` reused for rendering (no new component needed)

No new `ColumnKind` because the underlying type is still `Feed { uri }` — just sourced from a different listing endpoint.

---

## Related

- [[Adding-an-XRPC-Endpoint]]
- [[../Architecture/Architecture-Overview#Columns]]
