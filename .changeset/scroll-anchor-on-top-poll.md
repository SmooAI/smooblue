---
"smooblue": patch
---

When a column's 15–30 s top-poll inserts new posts at the head, your scroll position now stays anchored to whatever post you were reading. Previously the new content shifted everything down by its own height and you'd lose your place — a real pain on the Home column while reading a thread mid-scroll.

CSS-only fix: explicit `overflow-anchor: auto` on `.deck-column__body` (default per spec but stating it makes the intent reviewable + protects against accidental override) plus `overflow-anchor: none` on the trailing loading / empty / error / load-more chrome so WebKit's anchor selection stays constrained to actual content cards.

If this doesn't fully hold for you in practice (Dioxus diffing edge cases can defeat anchor-selection), I'll follow up with a JS scroll-math compensation that captures `scrollHeight` before the merge + bumps `scrollTop` by the delta after.
