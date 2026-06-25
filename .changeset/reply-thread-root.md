---
"smooblue": patch
---

Fix replies getting orphaned from their thread. When you replied to a post that was itself a reply (anything below the top of a thread), Smooblue stamped the new reply's `reply.root` as the immediate parent instead of the true thread root. Bluesky groups a thread by its root, so those replies rendered disconnected with no visible parent. Replies now inherit the correct thread root (falling back to the post itself for top-level posts), so they thread under the real conversation.
