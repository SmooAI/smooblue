---
"smooblue": minor
---

Inline `MessagesSheet` — tap a row in the Messages column and the conversation opens in a slide-over without leaving Smooblue (pearl th-57e3c9). Reads message history, lets you send (⌘↵ to submit), and marks the convo as read on open so the unread badge clears.

Wired:

- **New context**: `MessagesFocus(Option<convo_id>)` mirrors `ThreadFocus`. Mounted in DeckShell next to `ThreadSheet`. `ConvoRow`'s onclick switched from "open in bsky.app" to `messages_focus.set(...)`.
- **History loading**: `chat.bsky.convo.getMessages` on open + every 10s while the sheet stays open (Bluesky chat doesn't push). Messages reversed to render oldest-top, newest-bottom.
- **Bubbles**: right-aligned + brand-colored for your own messages; left-aligned + surface-alt for the other member's. Deleted messages render as a muted center-aligned "(message deleted)" tombstone. Bubbles cap at 75% width so long messages wrap rather than stretching across the sheet.
- **Send**: `chat.bsky.convo.sendMessage` with the typed draft. Server-canonical message appended to the on-screen list on success; the next poll will dedupe (same id). Failures surface as a red strip above the input rather than disappearing silently.
- **Mark-as-read**: `chat.bsky.convo.updateRead` fires in the background on every load — failure is cosmetic (the unread count in the column clears at the next 30s poll instead of instantly).
- **Timestamps**: `HH:MM` in local time on each bubble, parsed from the ISO-8601 the server returns.
- **CSS**: `.messages__sheet` + bubble variants live in `assets/styles.css` alongside the convo-row styles.

Limitations the next pearl can pick up: facets (mentions / hashtags / links) render as plain text; embeds (images, quoted records) aren't rendered yet; only first-page pagination — older messages aren't load-more-able. All tracked under the existing DM follow-up surface.
