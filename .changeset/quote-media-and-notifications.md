---
"smooblue-app": patch
---

Quote posts now show the quoted post's media. A quoted **video** (or link card / record-with-media) used to render as just the author's name with no content — the quote card only knew how to draw nested *images*. It now renders video players, link cards, and record-with-media the same way a top-level embed does.

Quote **notifications** now show the post that quoted you. "X quoted your post" was hydrating your own original post instead of X's quoting post, so the actual quote (with your post nested inside it) never appeared. Reply/mention/quote now consistently surface the inbound post.
