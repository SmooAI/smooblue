---
"smooblue-app": minor
"smooblue-atproto": minor
---

Posting a URL now attaches a link card. When the composer text contains a link, smooblue fetches its OpenGraph metadata (title, description, thumbnail) via CardyB — the same extractor the official Bluesky app uses — and shows a preview card under the textarea with a remove (×). On post it's published as an `app.bsky.embed.external` embed (or `recordWithMedia` when you're also quoting a post), so your followers see a real card instead of a bare URL. The card is skipped automatically when you've attached an image or video, since those own the post's single media slot.
