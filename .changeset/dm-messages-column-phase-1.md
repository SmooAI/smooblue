---
"smooblue": minor
---

Add Bluesky DMs as a new "Messages" deck column (pearls th-b313df + th-34805b). Tap the chat icon in the rail to add it. Renders your conversations newest-first: avatar + display-name/handle of the other member, last-message preview (one line), and an unread badge when applicable. Tapping a row opens that thread on `bsky.app/messages/{convoId}` in your browser — the inline message-history sheet + send-message support land in a follow-up (th-57e3c9), but read-the-inbox-from-Smooblue already removes the "switch to browser to see if anything's there" friction.

Under the hood:

- **`smooblue-atproto::chat`** — new module wrapping `chat.bsky.convo.{listConvos,getConvo,getMessages,sendMessage,updateRead,getConvoForMembers}`. All chat requests route through the user's PDS with the `atproto-proxy: did:web:api.bsky.chat#bsky_chat` header (Bluesky's documented chat-routing path). `AtClient::get_json_proxied` / `post_json_proxied` are the new generic primitives — the DPoP + nonce-retry machinery moved into a shared `do_json` so all four call sites (proxied + unproxied, GET + POST) share one implementation.
- **Types**: `ConvoView` / `MessageView` / `DeletedMessageView` / `MessageInput` / `ChatProfile` etc., with a `$type`-tagged `Message` enum that round-trips live and deleted messages distinctly. Facets and embeds modeled as `serde_json::Value` for v1 — we'll narrow types once the inline sheet starts rendering rich text + embeds.
- **State + UI**: `ColumnKind::Messages` enum variant + `ColumnSpec::messages()` constructor; `ColumnData::Convos(Vec<ConvoView>)` rendered via a new `ConvoRow` component. Sidebar gets a `MessageCircle`-iconned button that adds (or focuses) the column. Poll cadence: 30s, matching Notifications.

**Security/privacy doc updated** (README privacy table + `docs/Security/Security.md` "What's NOT done" item 6) to make explicit that Bluesky DMs are NOT end-to-end encrypted — Bluesky's chat service stores message bodies in plaintext and their operators/moderators can read them. Smooblue inherits this from the protocol; there is no Smooblue setting that changes it.
