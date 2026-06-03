---
"smooblue": minor
---

Virtualize the column body — only the rows within ~3 viewports of the visible area are mounted at a time, with top/bottom spacer divs preserving the scrollbar geometry as if all rows were rendered. Eliminates both failure modes the previous render path could hit on a 2000-row scrollback: the GPU tile-cache eviction that caused multi-second total blanks after deep scrollback (the original issue), and the `content-visibility: auto` per-card render gap that showed up as "blank cards have to load" after the column had been idle. Image bytes stay in WKWebView's image cache so re-mounting a row on scroll-back paints instantly.

Each column kind has its own estimated row height (240px posts, 110px notifications, 90px inbox, 72px messages, 96px suggestions); the 2-viewport buffer above and below the visible area absorbs the variance. Scroll-anchor on the body excludes spacer divs so top-poll prepends still keep the user's view stable.

Also drops the `content-visibility: auto` declarations added in 1.15.3 — virtualization caps the DOM size more aggressively than cv ever did.
