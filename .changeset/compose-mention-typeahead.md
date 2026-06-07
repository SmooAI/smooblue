---
"smooblue": minor
---

@mention autocomplete in the compose sheet. Typing `@` (at start-of-line or after whitespace) opens a popover beneath the textarea with up to 8 actor suggestions from `app.bsky.actor.searchActorsTypeahead`, debounced 150ms so each keystroke doesn't fire a round-trip. Arrow Up/Down navigates, Enter or Tab inserts `@handle ` (preserving any text before the mention), Esc dismisses, click also works. Previously mentions were only resolved at post-time — typing `@al` and hitting Post would silently degrade to plain text if `al` didn't resolve to anyone.

Only fires when the cursor is at the end of an active partial (no whitespace after the `@<chars>` run). Editing inside an existing word — including the mid-word `@` in an email address — doesn't accidentally pop the popover.
