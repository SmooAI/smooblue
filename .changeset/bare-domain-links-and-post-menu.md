---
"smooblue": minor
---

Bare URLs now linkify and embed. Typing a domain without a scheme (`smoo.ai`, `google.com`, `docs.example.io/guide`) is now detected as a link — it becomes a clickable facet on the published post and feeds the link-card preview, matching Bluesky's own composer. Previously only `http(s)://`-prefixed URLs were detected, so a bare domain published as plain text with no card.

The post "…" overflow button is now a real action menu. It used to just copy the link. It now opens a menu: Copy link, Open in browser, and — on your own posts — Delete (removes the post and hides it immediately); on others' posts — Mute, Block, and Report.
