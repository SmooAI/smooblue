---
"smooblue": patch
---

@mention popover now pops UP above the textarea instead of down. The compose dialog has no spare room below the textarea (Post button + attachment row sit there), so the popover was clipping past the dialog footer and getting overlapped by the Post button. Up has the headroom — the dialog header is short — and the popover lifts cleanly into that space. Also bumps the popover z-index past 50 and sets `overflow: visible` on the compose sheet so a tall suggestion list isn't clipped by the upstream modal's overflow rule.
