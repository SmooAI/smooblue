---
"smooblue": patch
---

Expand the security doc with a "Post-authentication: what protects your content in transit and at rest" section that walks through the three layers separately (TLS = transport, DPoP = per-request authenticity, AT Protocol = the honest "posts are public by design" content model). Adds explicit notes on DM support (intentionally none today; Bluesky hasn't shipped E2EE for chat yet), draft persistence on disk, and what Smooblue does NOT do with your content (no analytics, no third-party forwarding, no crash uploads). TL;DR table updated with rows for per-request authenticity, public-post content, and DMs so the reader gets the shape before drilling in.
