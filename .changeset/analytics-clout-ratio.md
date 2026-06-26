---
"smooblue": patch
---

Follower clout now accounts for the follower:following ratio, so accounts that farm follow-backs are penalized. A follower's reach is scaled by `clamp(followers / following, 0.25, 1.0)`: a ratio ≥ 1 (more followers than they follow — real clout) keeps full credit, while following far more than they're followed scales reach down (floored at 25%, never zeroed). The reach-driven lenses (Mutuals by Reach, High Clout Not Mutual, Lurkers) now rank by this ratio-adjusted reach, and existing follower scores are recomputed on launch so it applies right away.
