---
"smooblue": patch
---

Hydrate + render the subject post for `like-via-repost` and `repost-via-repost` notifications. The reason mapping was fixed in the previous changeset but the subject-hydration code still only fetched URIs for `like` / `repost` / `quote`, so via-repost notifications had no post to show. Now they hydrate + display the post you reposted (the one that got the new engagement) with a "From your repost of @handle" caption so it's clear it's not your own post. Subscribed-post notifications get the same treatment.
