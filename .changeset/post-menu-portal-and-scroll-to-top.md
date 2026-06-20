---
"smooblue": minor
---

The post "…" action menu no longer gets clipped. It was rendered inside the post card, which has `contain: paint` and lives in a column with `overflow: hidden` — so the menu was cut off at the card edge. It's now rendered at the deck level and positioned `fixed` at the click point, floating above every column with its actions fully visible.

Columns now have a "jump to top" pill. When you've scrolled down, a small pill appears near the top of the column; tapping it smooth-scrolls back to the top and resets the virtual viewport, so newly-polled posts (which accumulate above your read position) become visible live again.
