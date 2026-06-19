---
"smooblue": patch
---

Smoother column scrolling — fixed the "wiggle" when scrolling a feed, especially while new posts stream in. The virtualized lists (feeds + notifications) assumed every row was one fixed height, so on a mixed feed (a text post ~120px next to a 4-image grid + quote ~500px+) the scrollbar math drifted from the real layout and the browser re-corrected the scroll position every few rows. They now measure each row's real height and place the virtual window + spacers from those measurements, so the content stays put. Rows fall back to the per-kind estimate until they've been measured once.
