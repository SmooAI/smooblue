---
"smooblue": patch
---

Fix scrolling-column blankness on tall feeds. Re-added `content-visibility: auto` + `contain-intrinsic-size` on `.post`, `.notif`, `.inbox-row__wrap`, and `.convo-row` so WebKit can skip painting off-screen cards in the 2000-row scrollback. Without it, the GPU tile cache evicts painted tiles at scrollback distance and re-rasterizing the rich post DOM produced multi-second blank slots (both text and images missing) during scroll. The previous removal traded this away for a sub-100ms entry flash — that's the smaller artifact, and `contain-intrinsic-size: auto <px>` lets WebKit cache each card's last-measured size so the scrollbar stays accurate. Also flipped post/notification/quote-card avatars from `loading="lazy"` to `loading="eager"` — avatars are tiny and always wanted, and lazy decode contributed its own pop-in on fast scroll.
