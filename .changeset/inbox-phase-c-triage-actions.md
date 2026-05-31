---
"smooblue": minor
---

**Inbox — Phase C triage actions** (pearl th-e17045). Three actions per row, visible on hover (Stripe-Inbox style), backed by the SQLite triage state shipped in Phases A/B:

- **Archive** (X icon) — `inbox::set_archived(true)` + optimistic local hide. Row disappears immediately; next 15s column poll confirms persisted state from disk.
- **Snooze** (Clock icon) — dropdown with 1h / 4h / Tomorrow / Monday. `inbox::set_snoozed(Some(when))`; row hides until the snooze elapses, then the column query's `WHERE snoozed_until IS NULL OR snoozed_until <= now()` re-surfaces it.
- **Reply** (MessageQuote icon) — DMs expand an inline textarea + Send button (calls `chat_send_message`); posts open the existing ThreadSheet for full-fidelity composing (facets / images / quote — losing those for inline reply would be a regression).

Row click marks the item read (`inbox::set_read(true)`) + opens ThreadFocus or MessagesFocus depending on source.

**Adversarial-review P2 fix bundled**: `inbox::with_db` no longer silently downgrades to `:memory:` on disk-open failure. The OnceLock now holds `Option<Connection>`; if the open fails, the slot stays empty and subsequent calls retry (transient permission glitches self-heal). The CRUD methods propagate the error to the UI as a real failure rather than letting triage actions silently land in a throwaway DB.
