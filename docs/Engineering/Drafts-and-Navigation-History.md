# Drafts and Navigation History

#engineering #feature

Two features that make it hard to lose your place: **compose drafts** (every post, reply, quote and multi-post thread is kept until you post it or throw it away) and **sheet navigation history** (back and forward in threads and profiles, reopening what you just closed, and a History list).

---

## Why

The thread and profile views are modal sheets over the deck. One stray click on the backdrop closed a 200-reply thread, and the trail of replies you had clicked through to get there went with it. Compose had a similar problem. Before this change it kept only the first post's text, in `draft.txt`:

- closing the sheet dropped every continuation post in a thread
- a half-written reply showed up as the text of the next "New post"
- only one draft could exist at a time

---

## Drafts

### User flow

1. Start typing in compose (a new post, a reply, a quote, or a thread with **+ Add post**). The header shows **Draft saved** once there's something to keep.
2. Close the sheet however you like (Esc, ×, clicking the backdrop, or quitting the app). Nothing is lost.
3. Get back to it:
   - **Resume chip** above the + button: shows your newest draft. Click it to reopen.
   - **New post** (FAB / `n`) resumes your most recent unsent *top-level* draft.
   - **Reply** on a post resumes the draft you started for *that* post, if there is one. Otherwise it starts a fresh reply, and your other drafts stay saved.
   - **Drafts** in the compose header lists every draft (label, preview, age). Click one to switch. The one on screen is saved first.
4. **New** in the header saves the current draft and starts a blank post. The trash button in the bar discards the current draft (click twice).
5. Posting deletes the draft.

### What a draft holds

`crate::drafts::Draft`: account DID, reply or quote target, and every post in thread order. Each post has its text, image paths with alt text, and (on the first post) the video path with alt text. Media stays on disk. Restoring a draft re-runs the normal attach pipeline. Images that already have alt text skip the AI/OCR describe pass. Files that have since disappeared are dropped with a note. Pasted clipboard images are written under `<data_dir>/pasted/` instead of `$TMPDIR` so a draft that uses one still works days later.

### Storage

A `drafts` table in the app SQLite file (`smooblue.db`, schema **v7**, migration in `inbox::migrate`). The draft body is one JSON column, so its shape can change without a migration per field.

- Autosave is debounced 400 ms after the last edit and runs off the UI thread. Close and draft-switch save synchronously, so a quit right after can't lose anything.
- A stale background save (older `updated_at`) never overwrites a newer row.
- Drafts that were posted or discarded are *retired* for the session, so a late autosave can't resurrect a draft of something already published.
- The oldest drafts beyond 200 per account are pruned.
- A pre-existing `draft.txt` is imported once as a regular draft, then deleted.

Drafts are scoped per account. Legacy imported drafts have no account and show for everyone.

### Threads

- Each continuation post has its own character counter, image button (up to 4 images per post, via `AttachedImage::slot`), clipboard-paste target, and **Split into thread**.
- **Split into thread** breaks an over-long post at paragraph breaks, then sentence ends, then words (`drafts::split_for_thread`). It hard-cuts only a single word longer than the limit.
- The **whole** thread is validated before anything is published. A too-long post 3 used to fail *after* posts 1 and 2 were live.
- A reply can carry a thread too. The chain keeps the replied-to conversation's root.
- **Partial failure:** if post *k* fails, the posts that went out are removed from the composer. The rest becomes a reply to the last post that went out, and pressing **Reply** finishes the thread. Previously a retry re-posted the whole thing, duplicating what had already landed.
- The thread sheet refetches after you post (`PostedTick`), so a reply sent from inside a thread shows up there.

---

## Navigation history

### The rail's navigation group

Back, forward, reopen and History sit as buttons at the top of the left rail, right under the logo:
- **← / →** step back and forward along the timeline (see below). Their tooltips say where they go ("Back to a thread").
- **Reopen** (↺) is lit whenever something you closed can be brought back.
- **History** (🕘) opens the recently viewed list.

The group sits above the sheet backdrop (`.rail__nav` has z-index 55, the backdrop 50). So while a thread or profile is open, it stays sharp, clickable, and floats on its own surface. It sits below the compose sheet (60), so navigating can't pull a thread out from under a reply you're writing. The keyboard shortcuts below do the same things.

### Back and forward

`crate::history::NavHistory` keeps **one browser-style timeline** of what was on screen: the deck, the thread sheet and the profile sheet. It is not a per-sheet stack. Every change to either sheet's focus is a visit (one observer, `use_nav_observer`, mounted in `DeckShell`). Visits include opening a post from a column, clicking a reply or an embedded quote, opening a profile from a thread, closing a sheet (a return to the deck), and opening a History entry. **←** steps back along the timeline and **→** steps forward, exactly as in a browser. A new visit after going back drops the forward half.

In practice:
- Open a thread from a column and ← is already live; it goes back to the deck.
- At the deck, ← reopens the thread you just closed.
- Open a thread, close it, open another, and ← walks back through both.

Showing a timeline entry makes exactly that sheet the visible one. A back or forward is not recorded as a new visit (the `pending` view), and it doesn't count as a "close".

The first version kept a separate back stack per sheet that reset on every fresh open. So ← only ever lit up after clicking between replies *inside* one thread; in real use it never did.

Controls: the rail's ← →, the ← → in the thread header (and ← on the profile banner), and **⌘[** / **⌘]**. All of them work from the deck too.

### Reopen what you closed

When a thread or profile sheet closes, its key and trail are snapshotted (`NavHistory::last_closed`). A **Closed thread · Reopen** toast shows for 8 s, and **⌘⇧T** reopens it at any time, trail intact.

### Scroll memory

`THREAD_SCROLL_MEMORY_JS` (installed in `App`) records each thread body's `scrollTop`, keyed by its `data-uri`. Scroll events don't bubble, so it listens in the capture phase. When a thread's focused post mounts, `RESTORE_OR_FOCUS_JS` puts you back where you were, or glides to the focused post on a first visit. The thread body is keyed by URI so each thread gets a fresh scroll box.

### History sheet

**⌘Y** or the clock in the rail opens the History sheet. It lists recently viewed threads (author and snippet) and profiles (name and @handle), newest first, with a text filter and All/Threads/Profiles tabs. Rows live in the `nav_history` table (schema v7), upserted on view and pruned to 500. **Clear history** takes two clicks.

### Stacking

A post clicked inside a profile opens its thread *over* the profile. Before this change it opened underneath, hidden. The sheet raised most recently paints on top, and Esc closes that sheet first. Esc now also closes compose before whatever sheet is under it.

---

## Keyboard

| Keys | Action |
| --- | --- |
| ⌘[ / ⌘] | Back / forward in the top thread or profile sheet |
| ⌘⇧T | Reopen the thread / profile you just closed |
| ⌘Y | History |
| ⌘↵ (in any post of a thread) | Post the whole thread |
| Esc (in the @mention popover) | Close only the popover |

The navigation shortcuts are disabled while compose is open, so they can't pull a thread out from under a reply you're writing.

---

## Source

| Path | What |
| --- | --- |
| `crates/smooblue-app/src/drafts.rs` | Draft model, split, storage, legacy import |
| `crates/smooblue-app/src/history.rs` | NavStacks / NavHistory, sheet actions, `nav_history` storage |
| `crates/smooblue-app/src/components/compose.rs` | `Composer` (save/load/switch/post), thread editor, drafts list |
| `crates/smooblue-app/src/components/history_sheet.rs` | History sheet, reopen toast, resume-draft chip |
| `crates/smooblue-app/src/components/thread.rs` | Back/forward header, scroll restore, history recording |

---

## Related

- [[Engineering-Guide#Persistence locations]]
- [[Demo-Mode]]: demo mode uses an in-memory DB, so drafts and history work there without touching real data
