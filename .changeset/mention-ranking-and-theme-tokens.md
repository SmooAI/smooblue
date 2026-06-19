---
"smooblue-app": patch
---

The @mention autocomplete now biases toward people you actually know. Bluesky's typeahead is only lightly personalized, so it buried mutuals under big strangers who happened to prefix-match. Results are now re-ranked: mutuals first, then people you follow, then people who follow you, then strangers — and within a tier a prefix match on the handle or display name beats a mid-string match. We fetch a wider candidate set and trim after ranking so a buried mutual can still surface.

Fixed transparent backgrounds across the compose @mention dropdown, the DM/messages sheet, and inbox rows. These used CSS custom properties (`--color-surface`, `--color-fg`, etc.) that this theme never defines, so they resolved to transparent/inherited — you could read content straight through the mention popover. They now use the real theme tokens (`--card`, `--foreground`, `--muted`, `--muted-foreground`, `--border`).
