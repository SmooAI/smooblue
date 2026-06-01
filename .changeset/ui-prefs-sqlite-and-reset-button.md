---
"smooblue": minor
---

UI accessibility prefs (text size + column width) now persist in SQLite instead of `ui_prefs.json`. Migration v3 adds a generic `settings` k/v table to the existing SQLite store; the database file also got renamed `inbox.db` → `smooblue.db` (auto-rename on first open) since it's no longer inbox-only. One-time migration reads the legacy JSON, upserts it into SQLite, and deletes the file so manual edits to the JSON can't resurrect stale state. Follow-up pearl th-feacc8 tracks moving the other small JSON/text files (theme, columns, draft, last_handle) to the same store.

Also: **"Reset to defaults" button** in Settings → Appearance for the a11y sliders. One click puts text size back to 100% and column width back to 320px (the values you'd type ⌘0 to get for text alone; this fixes both at once).
