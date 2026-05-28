---
"smooblue": patch
---

Three fixes from the field:

**Notifications: "interacted with you" generic phrase replaced with proper reasons.** The lexicon ships `like-via-repost` / `repost-via-repost` / `verified` / `unverified` / `subscribed-post` in addition to the original six, and the phrase mapping only knew about the originals — so likes on YOUR reposts showed up as the meaningless "X interacted with you." Now they read "X liked a post you reposted." Also unified the phrase + icon mapping into one source of truth on `NotificationGroup` so the next lexicon add only requires editing one file.

**Compose typing lag.** Every keystroke into the post box was doing an inline `create_dir_all + fs::write` for draft persistence — on long drafts this stacked up enough to be visibly laggy. Moved the save off the render thread via `tokio::task::spawn_blocking`; the textarea now updates instantly and the draft saves asynchronously.

**Notifications column slowness.** Three knobs: bumped poll interval from 20s → 30s (notifications churn slower than feeds and each poll allocates a chunk of memory for hydrated subject posts), dropped page size from 50 → 30 (50 was visibly laggy on busy accounts), and switched `.notif` / `.post` containment to `contain-intrinsic-size: auto …` so cards that scroll back into view use their *actual* last-rendered size instead of falling back to the fixed estimate every time.
