---
"smooblue": patch
---

Inbox per-row mark-as-read button is now always rendered (dims + disables itself once the row is read) instead of hiding when the row was already read. Previously the affordance disappeared the moment you marked anything, so users couldn't find it after hitting "Mark all as read" once. Tooltip also flips from "Mark as read" → "Read" to make the state explicit.
