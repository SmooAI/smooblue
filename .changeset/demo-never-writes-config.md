---
"smooblue": patch
---

Demo mode (`SMOOBLUE_DEMO=1`) no longer touches your real settings. Adding a column during a demo run used to save the demo's column layout over your real one. That layout has "Discover" and "Rust" columns that are really Home feeds, so every column ended up showing Home. A sign-out inside demo mode could also have removed your real saved login. Demo mode now never saves or deletes your columns, theme, accounts, login or session files.
