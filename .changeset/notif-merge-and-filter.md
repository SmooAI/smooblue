---
"smooblue": patch
---

Two Notifications-column fixes:

- **Replies and mentions no longer go missing.** On each poll the column merged fresh notifications into existing ones keyed only by `(reason, subject)`. Because replies/mentions/quotes are ungrouped singletons (and often share a null subject), distinct ones collapsed into a single row and the newer notification was swallowed as a hidden sub-item. Notifications now merge by a key that keeps reply/mention/quote rows unique per notification, so every reply and mention shows up — while a re-fetched identical one still dedupes.
- **The header filter box now works on Notifications.** Typing in the column filter previously did nothing for a Notifications column. It now matches on the actor's handle/display name, the text they wrote, and the subject post's text/author.
