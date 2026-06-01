---
"smooblue": minor
---

**Fixed: empty Inbox column on every install.** The ingestion task that polls listNotifications + listConvos and upserts triage rows was defined since v1.11 but never actually called from anywhere — so the Inbox column has been silently empty since the feature shipped. Wired up at App-mount via `use_hook`, so the first poll fires ~5s after launch and every 30s thereafter. The fact-finding tool that surfaced this: the new `diag.log` (shipped v1.14.0) was empty after a full session, meaning zero ingestion cycles had fired. Pearl th-4eb2f1 tracks adding a regression test so this can't silently regress again.

**Infinite scroll on columns.** Scrolling near the bottom of any paginated column (Home, Search, Feeds, Lists, AuthorFeed) now auto-fetches the next page — no need to click the "Load more" button anymore. The button still renders as a fallback. Throttled internally so a fast scroll burst pre-warms the next page without firing N concurrent loads. Triggers ~600px before the end so the next page arrives before you actually hit it.

**⌘0 now resets the full a11y surface.** Previously ⌘0 only reset text size; column width was left at whatever you'd dragged it to. Now ⌘0 mirrors the Settings → "Reset to defaults" button — text size back to 100%, column width back to 320px. Keeps the keyboard shortcut and the visible button writing the same value so they can't drift.
