---
"smooblue": patch
---

Fix the home / feed column scroll-flash. Earlier we added `content-visibility: auto` on `.post` and `.notif` to skip rendering of off-screen cards — great for the deep-thread scroll case it was added for, but on fast-scrolling feed columns it meant each card entering the viewport flashed blank briefly while WebKit's async content-visibility paint caught up. Dropped `content-visibility: auto` (and the associated `contain-intrinsic-size`) and kept the cheap `contain: layout style paint` per-card isolation. The original deep-thread flashing issue was actually image-decode reflows, which we already fix separately with per-image `aspectRatio` on embeds + the 16:9 CSS fallback — so we don't need content-visibility to solve it.
