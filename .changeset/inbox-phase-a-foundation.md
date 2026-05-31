---
"smooblue": minor
---

**Inbox — Phase A foundation** (pearl th-e17045). New triage column lives in the deck (`ColumnKind::Inbox`, rail button between Messages and the divider). Phase A ships the schema + persistence + render skeleton; Phase B (next release) wires ingestion so the column actually populates.

What's in Phase A:

- **`smooblue_app::inbox` module** — types, persistence, scoring. Backed by SQLite via `rusqlite` (bundled, no system dep) at `directories::data_dir/smooblue/inbox.db` with WAL journaling.
- **Schema** with `device_id` + `synced_at` columns from day 1 so a future smoo.ai sync layer drops in without migration. Two indexes: `inbox_active_idx` (directness DESC, ts DESC, WHERE archived = 0) for the column read; `inbox_unsynced_idx` (WHERE synced_at IS NULL) for the future sync push set.
- **Directness scoring**: Reply-to-your-reply (100) > DM (90) > Quote (70) > Direct reply (60) > Mention (30). Age decay (1pt per 12h, capped at 40). Unread bump (+20) so unread floats within band.
- **CRUD API**: `upsert`, `list_active`, `set_read`, `set_archived`, `set_snoozed`, `unread_count`, `get`. UPSERT semantics on the insert so re-ingestion is naturally idempotent + preserves local triage state (read/archived/snoozed) when upstream payloads refresh.
- **Column render** with `InboxRow` component — avatar, actor, source chip (reply/mention/quote/DM), preview, age, unread dot. Click routes to ThreadFocus (posts) or MessagesFocus (DMs). Read rows dim.

Empty for now (no ingestion path yet). 4 new unit tests over schema migration + UPSERT round-trip + directness math.
