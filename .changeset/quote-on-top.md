---
"smooblue": patch
---

Fix: "Quote post" fired from inside a thread (or any other sheet) now opens the compose dialog ON TOP of the thread instead of hidden behind it. Same fix applies to the FAB when fired with another sheet open. Root cause: every sheet shared the same `.modal__backdrop` z-index, so DOM order decided stacking — and compose was rendered first in `deck.rs`, putting it under everything else. Added a `.modal__backdrop--compose` modifier (z-index 60 vs the default 50) so the compose sheet always wins.
