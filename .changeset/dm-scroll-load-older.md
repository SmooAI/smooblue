---
"smooblue": minor
---

Scroll-up in MessagesSheet now loads older messages automatically — no "Load more" button. As you scroll within 150px of the top of a conversation, the next page from `chat.bsky.convo.getMessages` is fetched and prepended; `overflow-anchor: auto` on the bubble container keeps the visible content stationary so you don't get yanked back to the new top. A subtle "Loading older messages…" strip surfaces while a fetch is in flight; once the server runs out of cursor we latch a "Start of conversation." marker so the user knows there's nothing more.

Also: auto-scroll-to-bottom whenever the conversation's TAIL grows (initial load, poll-discovered new message, your own sent message), so the latest message is always in view by default — without yanking you back to the bottom when you scrolled up on purpose to read older messages.

Implementation note: `dioxus::document::eval` is used both for reading `scrollTop` on every scroll event (gate against firing the fetch when scroll position is mid-conversation) and for scrolling-to-bottom on tail growth. Naive per-frame eval is fine because the `loading_older` latch self-guards against concurrent fetches.

New follow-up pearls filed alongside (rich-text/facets, embeds, message delete) — all P3/P4, none blocking.
