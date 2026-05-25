# Demo Mode

#engineering

`SMOOBLUE_DEMO=1` runs the entire app against canned data — no OAuth, no network, no real Bluesky account required. Useful for screenshots, recording, scale-testing the renderer, and iterating UI without burning a real session.

---

## Activating

```bash
SMOOBLUE_DEMO=1 cargo run -p smooblue-app
```

On launch with `SMOOBLUE_DEMO=1`:
- `state::use_bootstrap` injects a synthetic `Session` with a fake DID so the app skips past the login view.
- Every `Column::fetch_once` short-circuits to canned data instead of calling the AppView.
- The 1Hz `Tick` still runs (so timestamps tick "11s → 12s") — useful for verifying the perf fix where only `TimeAgo` subscribes.

---

## Scale tiers

`SMOOBLUE_DEMO_SCALE` controls how many posts each feed column returns. Default is `small`.

| Tier | Posts/column | Notifications | Purpose |
| --- | --- | --- | --- |
| `small` | 14 | 14 | Curated showcase — what screenshots use |
| `medium` | 100 | 100 | Sanity check |
| `large` | 500 | 500 | Catches signal-subscription regressions (timestamp re-render bug) |
| `huge` | 2000 | 2000 | Image fan-out / Dioxus diff cost |
| `insane` | 5000 | 5000 | "Does it OOM?" — not realistic, just smoke |

```bash
SMOOBLUE_DEMO=1 SMOOBLUE_DEMO_SCALE=large cargo run --release -p smooblue-app
```

Use `--release` for scale tests — dev builds spend most of their cycles in `iter()` overhead.

---

## What's canned

In `crates/smooblue-app/src/demo.rs`:

- `home_feed()` — curated 14-post timeline themed around smoo / Bluesky / Rust / atproto (timestamps relative to *now* so they always render as "2m" / "14m"); scales up by repeating with unique URIs + sliding timestamps
- `notifications_with_subjects()` — grouped likes / reposts / replies / mentions / follows with the right shape for the grouping logic
- `profile_for(actor)` — synthetic profile with banner + avatar + bio + counts + pinned post + viewer state
- `thread_for(uri)` — parent chain + nested replies
- `saved_feeds()` / `own_lists()` / `popular_feeds()` / `trending_topics()` — for the "+ Add column" sheet
- `suggestions()` — fake "people you might follow" list with bsky-folk-shaped names

All return real `smooblue_atproto::feed::*` types so the UI code can't tell it's demo data.

---

## Debug helpers

| Env var | Effect |
| --- | --- |
| `SMOOBLUE_DEBUG_OPEN_COMPOSE=1` | Boot straight into the compose sheet (screenshot the empty composer) |
| `SMOOBLUE_DEBUG_ATTACH=/path/to/image.jpg` | Inject a synthetic image attachment on compose mount (skip the file picker) |

---

## When demo mode masks real behavior

- **OAuth refresh** — bypassed entirely. Test the refresh path against a real account.
- **Anything that writes to the user's repo** (posts, profile edits, mutes, blocks) — demo mode just sleeps 400ms and closes the sheet. The bsky AppView's actual error responses are not exercised.
- **Network failure modes** — there are no errors in demo mode. Test transient failures against a real PDS by toggling Wi-Fi mid-fetch.

---

## Screenshot workflow

1. `SMOOBLUE_DEMO=1 cargo run --release -p smooblue-app`
2. Resize to a sensible window size (1280×800 is the default)
3. Add the columns you want to capture (Home, Notifications, Discover, profile sheet open, etc.)
4. ⌘⇧4 → spacebar → click the window for a clean PNG
5. Drop into `media/` if it's a doc asset, or `assets/icons/` if it's brand

---

## Related

- [[../Architecture/Architecture-Overview]]
- [[../Operations/Bundle-and-Install]]
