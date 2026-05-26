---
"smooblue": patch
---

Tighten the post-action row — each icon+count is now wrapped in a `.post__action-pair` span with a 2px internal gap, while the gap between distinct groups (reply / repost / quote / like / copy) stays at 14px. Counts now read as belonging to their icons instead of floating mid-row. Reposts + quote now also show a zero count (matching reply + like) so the row stays the same width regardless of engagement state.
