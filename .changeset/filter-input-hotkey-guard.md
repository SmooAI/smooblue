---
"smooblue": patch
---

Fix the column filter input swallowing letters. Keystrokes typed into a column's filter no longer leak into the deck's vim hotkey dispatcher, so letters that are also shortcuts (`j`, `k`, `h`, `l`, `n`, `g`, `G`, `?`, space) now type normally — you can finally filter for "jank" or "night". Escape in the filter clears it and closes the bar.
