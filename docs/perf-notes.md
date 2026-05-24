# Smooblue performance notes

Scratchpad for findings from `SMOOBLUE_DEMO_SCALE=` runs. Update
whenever you find or fix something measurable.

## Scale toggle

```bash
SMOOBLUE_DEMO=1 SMOOBLUE_DEMO_SCALE=small|medium|large|huge|insane \
    cargo run --release -p smooblue-app
```

| Tier | Posts/column | Notifications | Use case |
| --- | --- | --- | --- |
| `small` (default) | 14 | 14 | Curated showcase set; what screenshots use. |
| `medium` | 100 | 100 | Heavy-but-realistic user. |
| `large` | 500 | 500 | Power-user / catch-up-after-vacation. |
| `huge` | 2000 | 2000 | Stress test: image fan-out, diff cost. |
| `insane` | 5000 | 5000 | "Does it OOM?" curiosity. Not realistic. |

## Findings — 2026-05-24

### 1. Tick re-render burns 100% CPU at 500 posts ✅ FIXED

`PostCard` and `NotificationCard` each subscribed to the global
`Tick` signal so their relative-timestamp text could re-render
every second. At 500 cards, the 1 Hz tick caused 500 full-card
diffs/sec → one CPU core pegged at 100% continuously.

**Fix**: extracted `icons::TimeAgo` — a tiny `<span>` component
that's the *only* thing subscribed to the tick. PostCard +
NotificationCard render one TimeAgo child each; only N small text
nodes re-render per tick, not N entire cards.

**Verified**: scale=large drops from 100% steady-state CPU to 0%.

### 2. Image fan-out makes scrolling rough ✅ FIXED

Every PostCard / NotificationCard rendered its avatar + embed
thumbnails with `<img src=…>` eager-loaded. On a 500-post column
that's 1,000+ concurrent HTTP requests fired the moment the column
mounted, regardless of scroll position. User-reported as "scrolling
with lots of images is pretty rough."

**Fix**: added `loading="lazy"` + `decoding="async"` to every `<img>`
in the renderer (avatars, image grid tiles, link card thumbs, quote
avatars, suggestion rows, notification stack, profile feed). WebKit
defers fetch + decode until the element scrolls into view.

### 3. Memory is stable across all scale tiers ✅

60-second sample windows showed flat RSS at every tier — no leak.
`large` actually *shrank* from 391MB to 83MB mid-window when WebKit's
GC kicked in after the initial render burst. Healthy.

## Steady-state numbers (post-fix, release build, 60s sample window)

| Tier | Boot RSS | Steady RSS | Steady CPU |
| --- | --- | --- | --- |
| small | 105MB | 109MB | 0% |
| medium | 149MB | 150MB | 0.6% |
| large | 330MB | 83-180MB | 0% |
| huge | 387MB | 387MB | 0.7% |

## Known follow-ups

- **Subject post hydration is uncached**: `fetch_once` for the
  Notifications column re-fetches `get_posts` for the same subject
  URIs every 20s poll. If the URIs haven't changed since the last
  poll the bytes are wasted. Easy win: lift `subjects: HashMap<uri,
  PostView>` out of `ColumnData::Notifications` so it persists across
  fetches; only fetch URIs that aren't already in the map. Bound the
  map at e.g. 200 entries with an LRU eviction; invalidate on
  manual refresh. Filed as a follow-up.
- **Profile pages are uncached**: opening the same profile sheet
  twice re-fetches `getProfile` + `getAuthorFeed`. Could cache for
  5 minutes; user-perceived staleness is fine since profile fields
  change slowly.
- **Per-PostCard `post.clone()` allocations**: the column render
  loop clones the full PostView for every card on every poll
  (`PostCard { post: item.post.clone() }`). For 500 posts × 1
  poll/15s that's ~33 clones/sec. Inherent to Dioxus props (no
  borrowed-reference component args); acceptable cost.
- **Compose draft writes-per-keystroke**: persistence.rs writes
  draft.txt on every character. Fine in practice (file is tiny)
  but a debounce of ~250ms would halve syscalls.

## How to reproduce

Release build is necessary — debug builds add their own perf cost.

```bash
cargo build --release -p smooblue-app

# Pick the worst tier you want to test:
env SMOOBLUE_DEMO=1 SMOOBLUE_DEMO_SCALE=large \
    ~/.cargo/shared-target/release/smooblue-app > /tmp/smb.log 2>&1 &

# Sample memory + CPU every 10s
PID=$(pgrep -f smooblue-app | head -1)
for s in 8 18 28 38 48 58 68; do
    ps -o rss,%cpu -p $PID | tail -1 \
        | awk '{printf "t=%ds rss=%dMB cpu=%s%%\n", '"$s"', $1/1024, $2}'
    sleep 10
done
```
