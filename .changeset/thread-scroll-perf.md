---
"smooblue": patch
---

Long-thread scroll performance pass. The "flashing while scrolling a big thread" came from two compounding sources:

1. **Single-image embeds had no reserved space.** The 2/3/4-up image grids set `aspect-ratio: 2/1` in CSS but the 1-up grid didn't, so single-image cards started at 0 height and reflowed to the decoded height the moment `loading=lazy` fired — and the cascade of reflows looked like a flash storm in WebKit. `EmbedImage` now carries the per-image `aspectRatio` from the lexicon; the render plumbs it onto the embed div as an inline `aspect-ratio` style + `width`/`height` attrs on the `<img>`. Fallback CSS reserves 16:9 when the lexicon omitted dims so legacy posts still don't flash.

2. **Off-screen post cards were being laid out + painted on every scroll tick.** Added `content-visibility: auto` + `contain: layout style paint` (with `contain-intrinsic-size: 0 200px`) to `.post` and `.notif`. WebKit can now skip rendering off-screen cards entirely and never re-invalidate the rest of the column when one card changes. Biggest win on thread sheets with 100+ posts.

Plus an AGENTS.md / CLAUDE.md update codifying the "land the plane" workflow: every chunk of work runs fmt → clippy → tests → drop a changeset → commit → push, in that order, before being called done.
