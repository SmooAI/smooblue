---
"smooblue": minor
---

**Accessibility — browser-style zoom + column width** (pearl th-459511, from a real user request: *"Poor eyesight and I had trouble with the small text in posts… needed to expand the column width."*).

- **⌘= / ⌘+** zoom in
- **⌘-** zoom out
- **⌘0** reset to 100%
- **⌘ + scroll wheel** zoom (matches Chrome/Safari/Firefox UX)
- **Settings → Appearance → Text size slider** (50% → 300%, 5% steps) for the discoverable path
- **Settings → Appearance → Column width slider** (240px → 640px) for users who bumped text size and need wider columns

Both persist across launches via a new `UiPrefs` JSON at `directories::config_dir/smooblue/ui_prefs.json`. Applied via `document.documentElement.style.zoom` (WebKit's native browser-zoom property — scales text, padding, layouts together, not just rem-based fonts) and a new `--column-width` CSS var on the deck-column flex-basis. Keyboard handler sits at the deck-shell root and short-circuits BEFORE the existing vim-chord dispatcher so the shortcuts take precedence regardless of what's focused.
