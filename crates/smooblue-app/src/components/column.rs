//! A single deck column. Owns its own fetch state.
//!
//! A column's body is either a feed of posts (Home, AuthorFeed, Discover,
//! Search, custom feeds) or a feed of notifications (Notifications). Those
//! are different shapes, so [`ColumnData`] tags which view to render.
//!
//! Polling model (the "deck.blue feel"):
//! - Each column kind has its own cadence — see [`poll_interval`].
//! - The first fetch populates the column.
//! - Subsequent top-polls merge new items at the head, deduped by URI,
//!   so old scrollback survives the refresh.
//! - Scrolling near the bottom triggers a `fetch_more` with the saved
//!   cursor — items append at the tail.
//! - Capacity-capped at [`MAX_POSTS_PER_COLUMN`] to keep per-column
//!   memory bounded (~6 MB at 2000 items). Cap behavior is
//!   **refuse-to-load-more**, not bottom-eviction — we don't shuffle
//!   data out from under a user who's scrolled into the deep tail.
//! - No jetstream / firehose — pure XRPC polling against the AppView via
//!   the user's PDS, mirroring what deck.blue does.

use crate::auth_refresh::fresh_client;
use crate::components::notification_card::NotificationCard;
use crate::components::post::PostCard;
use crate::icons;
use crate::state::{ColumnDrag, ColumnKind, ColumnSettings, ColumnSpec, FocusColumn, NotifFilter};
use dioxus::prelude::*;
use smooblue_atproto::{
    group_notifications, ActorProfile, FeedItem, Notification, NotificationGroup, PostView,
};
use smooblue_oauth::Session;
use std::collections::HashMap;
use std::time::Duration;

/// Per-column scrollback cap. ~2000 items × ~3 KB/item ≈ 6 MB per
/// column in-memory (image bytes live in WKWebView's image cache,
/// not here). Nine maxed columns ≈ 50 MB — well inside our budget.
/// Above this we **refuse** to load more rather than evict from the
/// tail; evicting under the user's scroll position would be jarring.
pub const MAX_POSTS_PER_COLUMN: usize = 2000;

/// Same idea for the Notifications column. Grouped reasons (like /
/// repost / follow) collapse N notifications into one row, so 1000
/// groups represents far more than 1000 raw items — easily covers the
/// active-triage window even for accounts that get thousands of likes
/// a day. Above this we refuse new pages, same refuse-not-evict
/// policy as posts.
pub const MAX_NOTIF_GROUPS_PER_COLUMN: usize = 1000;

/// How many items we ask for per page. Small enough that the first
/// page paints fast, large enough that scroll-to-bottom doesn't fire
/// a fetch_more on every flick.
const PAGE_SIZE: u32 = 30;

/// Below this `scrollTop` (px) a column counts as "at the top", so a
/// top-poll's freshly-prepended rows simply appear in view. Above it
/// the user has scrolled into the feed and we anchor instead — growing
/// the scrollbar upward so their read position doesn't move. A few px
/// of slack absorbs sub-pixel scroll rounding at rest.
const SCROLL_ANCHOR_MIN_PX: f64 = 8.0;

/// How close to the bottom (in pixels) the user would have to scroll
/// before an auto fetch_more would trigger. Currently unused —
/// Dioxus 0.6's `ScrollData` doesn't expose scroll position, so we
/// drive `fetch_more` from a "Load more" button instead. Kept as a
/// const for the future JS-eval IntersectionObserver wire-up.
#[allow(dead_code)]
const FETCH_MORE_THRESHOLD_PX: f64 = 400.0;

#[derive(Clone, PartialEq, Default)]
enum ColumnData {
    #[default]
    Empty,
    Posts(Vec<FeedItem>),
    /// Pre-grouped notifications + a side-table of hydrated subject
    /// posts (keyed by AT-URI). Groups collapse e.g. 20 likes on the
    /// same post into one card; non-grouping reasons (reply, mention,
    /// quote) stay as singletons. The hydration map serves both the
    /// grouped subject (likes/reposts) and the per-item subject
    /// (replies/mentions/quotes).
    Notifications {
        groups: Vec<NotificationGroup>,
        subjects: HashMap<String, PostView>,
    },
    /// List of actors the AppView suggests the viewer follows. Each
    /// is rendered as a follow-row card with bio + Follow button.
    Suggestions(Vec<ActorProfile>),
    /// Bluesky DMs (`chat.bsky.convo.listConvos`). Rendered as a list
    /// of conversation rows; clicking opens the thread on bsky.app
    /// until the inline MessagesSheet lands in a follow-up.
    Convos(Vec<smooblue_atproto::ConvoView>),
    /// Inbox triage list (pearl th-e17045). Rows come from the local
    /// SQLite store; ingestion populates it from listNotifications +
    /// listConvos. v1 ships read-only (Phase A); triage actions
    /// (archive/snooze/quick-reply) land in subsequent phases.
    Inbox(Vec<crate::inbox::InboxItem>),
}

impl ColumnData {
    fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Posts(p) => p.is_empty(),
            Self::Notifications { groups, .. } => groups.is_empty(),
            Self::Suggestions(actors) => actors.is_empty(),
            Self::Convos(convos) => convos.is_empty(),
            Self::Inbox(items) => items.is_empty(),
        }
    }
}

/// How often each column refetches. Picked to match deck.blue's feel
/// without hammering the AppView.
fn poll_interval(kind: &ColumnKind) -> Duration {
    match kind {
        ColumnKind::Home => Duration::from_secs(15),
        // Notifications churn slower than the home feed AND each
        // poll allocates ~30 hydrated subject posts + groups + clones
        // them on every render down the tree. 30s halves the GC
        // pressure without users noticing the latency difference.
        ColumnKind::Notifications => Duration::from_secs(30),
        ColumnKind::Search { .. } => Duration::from_secs(30),
        ColumnKind::Feed { .. } => Duration::from_secs(25),
        ColumnKind::AuthorFeed { .. } => Duration::from_secs(45),
        ColumnKind::List { .. } => Duration::from_secs(25),
        // Suggestions are personalized; refresh slowly — the user
        // doesn't want their suggested-follows list flickering.
        ColumnKind::Suggestions => Duration::from_secs(300),
        // DMs — same cadence as notifications. Unread count surfaces
        // new messages without waiting for the user to click in.
        ColumnKind::Messages => Duration::from_secs(30),
        // Inbox reads from the local SQLite store; the ingestion task
        // (Phase B) handles upstream polling. 15s keeps the rendered
        // list close to what the DB has if a triage action elsewhere
        // mutates a row.
        ColumnKind::Inbox => Duration::from_secs(15),
    }
}

#[component]
pub fn Column(spec: ColumnSpec) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let drag_ctx = use_context::<Signal<ColumnDrag>>();
    // Deck spec signal — the poll loop reads this for the live refresh
    // cadence, and the settings panel mutates this column's settings.
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();
    // Per-column settings panel (gear) open state.
    let settings_open = use_signal(|| false);
    let spec_kind = spec.kind.clone();
    let spec_id = spec.id.clone();

    // Current visible data. Top-polls merge new items at the head;
    // scroll-bottom triggers fetch_more which appends at the tail.
    let mut data = use_signal(ColumnData::default);
    let mut error = use_signal::<Option<String>>(|| None);
    let mut loading = use_signal(|| true);
    // Server-side cursor for the next fetch_more. None on first
    // mount; populated from each fetch's returned cursor (whether
    // top-poll or fetch-more) so the next page picks up where the
    // last one left off.
    let mut next_cursor = use_signal::<Option<String>>(|| None);
    // Pinned `true` while a fetch_more is in flight so the scroll
    // observer doesn't enqueue a second concurrent fetch.
    let mut loading_more = use_signal(|| false);
    // `true` when the server tells us the bottom-of-feed cursor is
    // None — we've hit the end and shouldn't keep firing fetches.
    let mut at_end = use_signal(|| false);
    // Viewport geometry for the virtualized render path AND scroll
    // anchoring: `(scroll_top, client_h, scroll_h)`. Declared up here
    // (rather than beside the scroll handler) so the polling loop can
    // read and adjust `scroll_top` when a top-poll prepends rows — see
    // the scroll-anchor block in that loop. Updated on every scroll
    // tick and once on mount via a `document::eval` round-trip.
    let mut viewport = use_signal(|| (0.0_f64, 0.0_f64, 0.0_f64));
    // Measured per-row heights keyed by the row's stable key (post URI,
    // notif group key, …). Feeds `measured_virtual_range` so the
    // virtualized window + spacers track real content heights instead
    // of one fixed estimate — that's what removes the scroll "wiggle"
    // on mixed feeds. Rows the user hasn't scrolled past yet fall back
    // to the per-kind estimate until measured. Updated from the same
    // scroll/mount eval that reads viewport geometry.
    let row_heights = use_signal::<HashMap<String, f64>>(HashMap::new);
    // Per-column fuzzy filter input. Empty string = show everything.
    // Match is case-insensitive substring on (text, author handle,
    // author displayName, reposter displayName, parent handle). No
    // levenshtein / fuzzy-skip — substring + lowercase is what users
    // mean when they say "filter for rust".
    //
    // Two-signal pattern for debouncing: `filter_text` tracks the
    // raw input (re-renders the <input>'s value attr instantly so
    // typing feels responsive); `filter_applied` lags 200ms behind
    // and is what the render path actually filters against. Without
    // this, every keystroke triggers a full Vec<FeedItem> filter +
    // PostCard re-render for the whole column body (~2000 items
    // worst case), which Dioxus diffs frame-by-frame and stutters.
    let mut filter_text = use_signal(String::new);
    let mut filter_applied = use_signal(String::new);
    let mut filter_open = use_signal(|| false);

    // Scroll-into-view + flash when the sidebar focuses us.
    // Stores the mounted root element so the effect can call
    // scroll_to; toggles `flash` on for ~1.5s to animate the border.
    let mut root_el = use_signal::<Option<std::rc::Rc<MountedData>>>(|| None);
    let mut flash = use_signal(|| false);
    let focus_sig = use_context::<Signal<FocusColumn>>();
    let id_for_effect = spec_id.clone();
    use_effect(move || {
        let focused = focus_sig.read();
        if focused.id.as_deref() != Some(id_for_effect.as_str()) {
            return;
        }
        // Both effects below run synchronously in the event-loop tick
        // following the signal change; use spawn for the actual
        // scroll/sleep so we don't block the signal write.
        let mounted_snap = root_el.peek().clone();
        spawn(async move {
            if let Some(m) = mounted_snap {
                let _ = m.scroll_to(ScrollBehavior::Smooth).await;
            }
        });
        flash.set(true);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            flash.set(false);
        });
    });

    // Debounce: spawn a sleep-and-set on every keystroke. The
    // closure captures the current value at spawn time; if a later
    // keystroke arrived before we wake up, the captured value won't
    // match the now-current input and we skip the set. Effectively
    // a "trailing-edge debounce" — only the last typed value
    // actually lands.
    use_effect(move || {
        let target = filter_text.read().clone();
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            // If the input has moved on since we slept, abandon
            // this stale apply. Reading filter_text.peek() here
            // does NOT re-subscribe the effect (we already react
            // to filter_text on the way in).
            if *filter_text.peek() == target && *filter_applied.peek() != target {
                filter_applied.set(target);
            }
        });
    });

    // The polling loop. Top-of-feed refresh on each tick: merges new
    // items at the head, preserves the user's scrollback below.
    let kind_for_poll = spec_kind.clone();
    let spec_id_for_poll = spec_id.clone();
    use_future(move || {
        let kind = kind_for_poll.clone();
        let col_id = spec_id_for_poll.clone();
        let session_sig = session;
        let cols_sig = cols;
        async move {
            let mut first_fetch = true;
            // Persistent across polls — used by the Notifications fetch
            // path to avoid re-hydrating subject posts that are already
            // known. Bounded at 500 entries so a long-running session
            // doesn't grow this unboundedly.
            let mut subjects_cache: HashMap<String, PostView> = HashMap::new();
            loop {
                // Resolve the live refresh cadence from this column's
                // settings each tick. `Default` → the per-kind interval;
                // `Off` → paused. While paused (after the initial load)
                // we idle, re-checking the setting every 2s so resuming
                // takes effect promptly.
                let fallback = poll_interval(&kind);
                let refresh = {
                    let list = cols_sig.read();
                    list.iter()
                        .find(|c| c.id == col_id)
                        .map(|c| c.settings.refresh)
                        .unwrap_or_default()
                };
                let effective = refresh.duration(fallback);
                if effective.is_none() && !first_fetch {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                let interval = effective.unwrap_or(fallback);
                match fetch_page(&kind, session_sig, None, &mut subjects_cache).await {
                    Ok(fresh) => {
                        error.set(None);
                        loading.set(false);
                        // Merge the fresh page into whatever we already
                        // have. First-fetch: just install. Subsequent
                        // polls: prepend new items, preserve tail.
                        // How many genuinely-new rows get prepended at the
                        // head this poll (same dedupe key `merge_top_page`
                        // uses) — drives the scroll-anchor adjustment below.
                        let mut prepended_rows = 0usize;
                        let merged = match (data.peek().clone(), fresh.data) {
                            (_, ColumnData::Empty) => ColumnData::Empty,
                            (ColumnData::Posts(existing), ColumnData::Posts(new_page)) => {
                                let existing_keys: std::collections::HashSet<String> =
                                    existing.iter().map(feed_item_key).collect();
                                prepended_rows = new_page
                                    .iter()
                                    .filter(|&it| !existing_keys.contains(&feed_item_key(it)))
                                    .count();
                                ColumnData::Posts(merge_top_page(
                                    existing,
                                    new_page,
                                    MAX_POSTS_PER_COLUMN,
                                ))
                            }
                            // Notifications: top-poll merges new groups
                            // at the head (and grows existing groups
                            // with new items) so paginated scrollback
                            // below survives the 15s refresh cycle.
                            (
                                ColumnData::Notifications {
                                    groups: existing_groups,
                                    subjects: existing_subjects,
                                },
                                ColumnData::Notifications {
                                    groups: new_groups,
                                    subjects: new_subjects,
                                },
                            ) => {
                                // New subjects win on conflict — they're
                                // fresher than anything in scrollback.
                                let mut merged_subjects = existing_subjects;
                                for (k, v) in new_subjects {
                                    merged_subjects.insert(k, v);
                                }
                                let merged_groups = merge_top_notif_groups(
                                    existing_groups,
                                    new_groups,
                                    MAX_NOTIF_GROUPS_PER_COLUMN,
                                );
                                ColumnData::Notifications {
                                    groups: merged_groups,
                                    subjects: merged_subjects,
                                }
                            }
                            // Suggestions / Messages / Inbox: top-poll
                            // replaces wholesale (single page or
                            // local-DB read).
                            (_, other) => other,
                        };
                        // Scroll anchoring. When the user has scrolled down
                        // into the feed, a top-poll that prepends N rows must
                        // not shove their read position around. Grow the feed
                        // *upward* instead: bump the virtual viewport and the
                        // real `scrollTop` by the prepended rows' height, so
                        // the scrollbar lengthens at the top while the visible
                        // content stays put. Near the top (scroll_top within
                        // SCROLL_ANCHOR_MIN_PX) we skip this so fresh posts
                        // simply appear in view. Posts-only: notification
                        // groups merge in-place (existing groups grow) so the
                        // prepend count doesn't map cleanly to a row height.
                        let cur_top = viewport.peek().0;
                        if !first_fetch && prepended_rows > 0 && cur_top > SCROLL_ANCHOR_MIN_PX {
                            let delta_px = prepended_rows as f64 * estimated_row_height_px(&kind);
                            // Bump the virtual window first so the re-render
                            // slices around the corrected position (no stale-
                            // window flash) ...
                            viewport.with_mut(|v| v.0 += delta_px);
                            data.set(merged);
                            // ... then nudge the real DOM scrollTop to match,
                            // inside rAF so it lands after the new rows reflow
                            // (avoids clamping against a not-yet-grown height).
                            let sel = format!("[data-column-body=\"{col_id}\"]");
                            spawn(async move {
                                let mut eval = dioxus::document::eval(&format!(
                                    r#"
                                    const el = document.querySelector({sel});
                                    if (el) {{
                                        requestAnimationFrame(() => {{
                                            el.scrollTop += {delta};
                                            dioxus.send(true);
                                        }});
                                    }} else {{ dioxus.send(false); }}
                                    "#,
                                    sel = serde_json::to_string(&sel)
                                        .unwrap_or_else(|_| "\"\"".to_string()),
                                    delta = delta_px,
                                ));
                                let _: bool = eval.recv().await.unwrap_or(false);
                            });
                        } else {
                            data.set(merged);
                        }
                        // Save the cursor from the top page — the FIRST
                        // top-poll's cursor tells us where to start
                        // paginating downward from. We don't overwrite
                        // on subsequent polls because top cursors point
                        // to "the page below the newest" and would
                        // shift as new items arrive.
                        if first_fetch {
                            next_cursor.set(fresh.cursor);
                            at_end.set(false);
                        }
                        // First successful Notifications fetch: tell
                        // the server we've seen them so the sidebar
                        // unread badge clears. Best-effort; failures
                        // are silent (the badge will catch up next
                        // poll cycle anyway).
                        if first_fetch
                            && matches!(&kind, ColumnKind::Notifications)
                            && !crate::demo::is_active()
                        {
                            if let Some(client) = fresh_client(session_sig).await {
                                let _ = client.update_seen(chrono::Utc::now()).await;
                            }
                        }
                        first_fetch = false;
                    }
                    Err(e) => {
                        loading.set(false);
                        error.set(Some(e));
                    }
                }
                tokio::time::sleep(interval).await;
            }
        }
    });

    // Trigger-load-more callback. Single source of truth — both the
    // visible "Load more" button (onclick) AND the infinite-scroll
    // onscroll handler call this. Skips entirely for non-paginated
    // column kinds (Notifications, Suggestions) and when:
    //   - a fetch is already in flight
    //   - the server told us there's no more (at_end)
    //   - we'd push the column over MAX_POSTS_PER_COLUMN
    //   - the saved cursor is empty
    //
    // use_callback wraps the closure as `Callback<()>` which is
    // Copy + Clone — so we can drop it into onclick AND the scroll
    // handler without the dance of cloning a non-Clone FnMut.
    let kind_for_more = spec_kind.clone();
    let trigger_load_more = use_callback(move |_: ()| {
        if !is_paginated(&kind_for_more) {
            return;
        }
        if *loading_more.peek() || *at_end.peek() {
            return;
        }
        // Cap-guard: refuse rather than evict.
        match &*data.peek() {
            ColumnData::Posts(items) if items.len() >= MAX_POSTS_PER_COLUMN => return,
            ColumnData::Notifications { groups, .. }
                if groups.len() >= MAX_NOTIF_GROUPS_PER_COLUMN =>
            {
                return
            }
            _ => {}
        }
        // Need a non-empty cursor to ask for more.
        let cursor = match next_cursor.peek().clone() {
            Some(c) if !c.is_empty() => c,
            _ => return,
        };
        let kind = kind_for_more.clone();
        loading_more.set(true);
        spawn(async move {
            match fetch_page(&kind, session, Some(cursor), &mut HashMap::new()).await {
                Ok(more) => {
                    // Drop the immutable borrow on `data` before we
                    // call `data.set` — Dioxus tracks signal borrows
                    // dynamically and a held read-guard during a
                    // write panics.
                    let existing_snap = data.peek().clone();
                    match (existing_snap, more.data) {
                        (ColumnData::Posts(existing), ColumnData::Posts(new_page)) => {
                            data.set(ColumnData::Posts(append_bottom_page(
                                existing,
                                new_page,
                                MAX_POSTS_PER_COLUMN,
                            )));
                        }
                        (
                            ColumnData::Notifications {
                                groups: existing_groups,
                                subjects: existing_subjects,
                            },
                            ColumnData::Notifications {
                                groups: new_groups,
                                subjects: new_subjects,
                            },
                        ) => {
                            // Merge newly-hydrated subjects (post views
                            // referenced by the older notifications).
                            // Existing wins on conflict — top-poll just
                            // ran and is fresher than anything we'd
                            // pull from a backfill page.
                            let mut merged_subjects = existing_subjects;
                            for (k, v) in new_subjects {
                                merged_subjects.entry(k).or_insert(v);
                            }
                            let merged_groups = append_bottom_notif_groups(
                                existing_groups,
                                new_groups,
                                MAX_NOTIF_GROUPS_PER_COLUMN,
                            );
                            data.set(ColumnData::Notifications {
                                groups: merged_groups,
                                subjects: merged_subjects,
                            });
                        }
                        _ => {}
                    }
                    if more.cursor.is_none() {
                        at_end.set(true);
                    } else {
                        next_cursor.set(more.cursor);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "smooblue: fetch_more failed");
                }
            }
            loading_more.set(false);
        });
    });
    let load_more = move |_| trigger_load_more.call(());

    // Drives both the visible-slice computation AND the infinite-scroll
    // trigger (within-600px-of-bottom heuristic). The `viewport` signal
    // it updates is declared earlier (near the other column signals) so
    // the polling loop can scroll-anchor against it.
    let mut scroll_check_pending = use_signal(|| false);
    let kind_for_scroll = spec_kind.clone();
    let body_selector = format!("[data-column-body=\"{}\"]", spec_id);
    let body_selector_for_scroll = body_selector.clone();
    let on_body_scroll = move |_evt: Event<ScrollData>| {
        if *scroll_check_pending.peek() {
            return;
        }
        let sel = body_selector_for_scroll.clone();
        let kind = kind_for_scroll.clone();
        scroll_check_pending.set(true);
        spawn(async move {
            let mut eval = dioxus::document::eval(&probe_js(&sel));
            let v: serde_json::Value = eval.recv().await.unwrap_or(serde_json::Value::Null);
            scroll_check_pending.set(false);
            if let Some((st, ch, sh)) = parse_probe_geometry(&v) {
                viewport.set((st, ch, sh));
                apply_measured_heights(&v, row_heights);
                // Infinite-scroll trigger: within 600px of bottom.
                let dist = sh - st - ch;
                if is_paginated(&kind)
                    && !*loading_more.peek()
                    && !*at_end.peek()
                    && (0.0..600.0).contains(&dist)
                {
                    trigger_load_more.call(());
                }
            }
        });
    };
    // Prime the viewport signal on first mount so virtualization has
    // a real clientHeight before the first user scroll. Without this,
    // the initial render slices using clientHeight=0 (renders zero
    // rows) until the user scrolls.
    let body_selector_for_mount = body_selector.clone();
    let on_body_mounted = move |_evt: Event<MountedData>| {
        let sel = body_selector_for_mount.clone();
        spawn(async move {
            let mut eval = dioxus::document::eval(&probe_js(&sel));
            let v: serde_json::Value = eval.recv().await.unwrap_or(serde_json::Value::Null);
            if let Some((st, ch, sh)) = parse_probe_geometry(&v) {
                viewport.set((st, ch, sh));
                apply_measured_heights(&v, row_heights);
            }
        });
    };
    // Re-measure rows whenever the data changes (new page, top-poll
    // prepend, filter toggle) so freshly-rendered rows pick up real
    // heights without waiting for the next scroll tick. Runs after the
    // render commits; diff-guarded inside apply_measured_heights so it
    // can't spin a render→measure loop.
    let body_selector_for_remeasure = body_selector.clone();
    use_effect(move || {
        let _ = data.read().is_empty(); // subscribe to data changes
        let sel = body_selector_for_remeasure.clone();
        spawn(async move {
            let mut eval = dioxus::document::eval(&probe_js(&sel));
            let v: serde_json::Value = eval.recv().await.unwrap_or(serde_json::Value::Null);
            apply_measured_heights(&v, row_heights);
        });
    });
    // "Mark all as read" callback — wired into the header's Inbox-only
    // action. Updates the DB then re-reads list_active so the column
    // reflects the change immediately (the 15s poll would otherwise
    // be the source of truth for stale UI). For non-Inbox columns
    // we pass None; the header just doesn't render the button.
    let mark_all_inbox_read: Option<Callback<()>> = if matches!(spec_kind, ColumnKind::Inbox) {
        let mut data_for_mark = data;
        Some(use_callback(move |_: ()| {
            spawn(async move {
                let res = tokio::task::spawn_blocking(|| {
                    crate::inbox::mark_all_read()?;
                    crate::inbox::list_active(500)
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("blocking task panicked: {e}")));
                match res {
                    Ok(items) => data_for_mark.set(ColumnData::Inbox(items)),
                    Err(err) => tracing::warn!(error = %err, "inbox: mark-all-read failed"),
                }
            });
        }))
    } else {
        None
    };

    // Whether to render the "Load more" button (only on paginated
    // kinds, only when not at-end, only when not capped).
    let kind_for_button_check = spec_kind.clone();
    let show_load_more = is_paginated(&kind_for_button_check)
        && !*at_end.read()
        && match &*data.read() {
            ColumnData::Posts(items) => !items.is_empty() && items.len() < MAX_POSTS_PER_COLUMN,
            ColumnData::Notifications { groups, .. } => {
                !groups.is_empty() && groups.len() < MAX_NOTIF_GROUPS_PER_COLUMN
            }
            _ => false,
        };

    // Whether the column has hit its scrollback cap (refuse-rather-
    // than-evict; the bottom indicator switches to a cap message).
    let at_cap = match &*data.read() {
        ColumnData::Posts(items) => items.len() >= MAX_POSTS_PER_COLUMN,
        ColumnData::Notifications { groups, .. } => groups.len() >= MAX_NOTIF_GROUPS_PER_COLUMN,
        _ => false,
    };

    // Visual state derived from the shared drag context — used to dim
    // the column being dragged and highlight the drop target.
    let drag_snap = drag_ctx.read();
    let is_dragging = drag_snap.dragging.as_deref() == Some(spec_id.as_str());
    let is_target = drag_snap.target.as_deref() == Some(spec_id.as_str())
        && drag_snap.dragging.as_deref() != Some(spec_id.as_str());
    drop(drag_snap);

    let flash_now = *flash.read();
    let base = match (is_dragging, is_target) {
        (true, _) => "deck-column deck-column--dragging",
        (_, true) => "deck-column deck-column--drop-target",
        _ => "deck-column",
    };
    let section_class = if flash_now {
        format!("{base} deck-column--flash")
    } else {
        base.to_string()
    };

    // Raw input string is what the <input>'s `value` attribute
    // shows (so typing feels instant); the *applied* debounced
    // value is what we actually filter against.
    let filter_snap = filter_text.read().clone();
    let applied_snap = filter_applied.read().clone();
    let filter_lower = applied_snap.trim().to_lowercase();
    let has_filter = !filter_lower.is_empty();

    rsx! {
        section { class: "{section_class}",
            onmounted: move |e| root_el.set(Some(e.data())),
            ColumnHeader {
                id: spec.id.clone(),
                title: spec.title.clone(),
                kind: spec.kind.clone(),
                filter_open,
                settings_open,
                mark_all_read: mark_all_inbox_read,
            }
            // Per-column settings panel — slides in below the header when
            // the gear is tapped. Filters + refresh cadence, persisted.
            if *settings_open.read() {
                ColumnSettingsPanel { id: spec.id.clone(), kind: spec.kind.clone(), settings: spec.settings.clone(), cols }
            }
            // Floating "jump to top" pill — appears once the column is
            // scrolled down. Tapping it smooth-scrolls to the top and
            // resets the virtual viewport to 0, so freshly-polled posts
            // (which prepend above the read position while you're
            // scrolled down) become visible live again.
            if viewport.read().0 > 600.0 {
                button {
                    class: "deck-column__to-top",
                    title: "Jump to top — resume live updates",
                    onclick: {
                        let sel = body_selector.clone();
                        move |_| {
                            viewport.with_mut(|v| v.0 = 0.0);
                            let sel = serde_json::to_string(&sel)
                                .unwrap_or_else(|_| "\"\"".to_string());
                            spawn(async move {
                                let _ = dioxus::document::eval(&format!(
                                    "var el=document.querySelector({sel}); if(el) el.scrollTo({{top:0,behavior:'smooth'}});"
                                ));
                            });
                        }
                    },
                    icons::ArrowUp { size: icons::Size::Sm }
                    "Top"
                }
            }
            // Filter input — slides in below the header when the
            // funnel button on the header is clicked or when the
            // user has anything typed (so a non-empty filter is
            // always visible).
            if *filter_open.read() || has_filter {
                div { class: "deck-column__filter",
                    input {
                        class: "input deck-column__filter-input",
                        placeholder: "Filter posts in this column…",
                        autofocus: true,
                        value: "{filter_snap}",
                        oninput: move |e| filter_text.set(e.value()),
                        // The deck shell's root onkeydown runs the vim
                        // hotkey dispatcher and prevent_default()s any key
                        // it consumes (j/k/h/l/n/g/G/?/space). Without this
                        // guard those letters never reach the input — you
                        // can't filter for "jank" or "night". Stop every
                        // keystroke from bubbling so typing works normally;
                        // handle Escape ourselves to clear + close the bar.
                        onkeydown: move |e| {
                            if e.key() == Key::Escape {
                                filter_text.set(String::new());
                                filter_applied.set(String::new());
                                filter_open.set(false);
                            }
                            e.stop_propagation();
                        },
                    }
                    if has_filter {
                        button { class: "deck-column__filter-clear",
                            title: "Clear filter",
                            onclick: move |_| {
                                filter_text.set(String::new());
                                // Apply immediately on explicit clear
                                // so the user doesn't wait 200ms to
                                // see the unfiltered feed return.
                                filter_applied.set(String::new());
                                filter_open.set(false);
                            },
                            icons::X { size: icons::Size::Sm }
                        }
                    }
                }
            }
            div {
                class: "deck-column__body",
                // Unique selector for the document::eval scroll-geometry
                // probe — column id is already namespaced by spec.id, so
                // it's safe to query via attribute selector.
                "data-column-body": "{spec.id}",
                onscroll: on_body_scroll,
                onmounted: on_body_mounted,
                match (&*data.read(), &*error.read(), *loading.read()) {
                    (_, _, true) if data.read().is_empty() => rsx! { div { class: "deck-column__loading", "Loading…" } },
                    (data, _, _) if data.is_empty() => rsx! { div { class: "deck-column__empty", "Nothing here yet." } },
                    (ColumnData::Posts(items), _, _) => {
                        let filtered: Vec<&FeedItem> = items
                            .iter()
                            .filter(|it| !has_filter || feed_item_matches(it, &filter_lower))
                            .filter(|it| passes_feed_settings(it, &spec.settings))
                            .collect();
                        if filtered.is_empty() {
                            rsx! {
                                div { class: "deck-column__empty",
                                    "No posts match \"{applied_snap}\""
                                }
                            }
                        } else {
                            let (vp_top, vp_h, _) = *viewport.read();
                            let est = estimated_row_height_px(&spec.kind);
                            let keys: Vec<String> =
                                filtered.iter().map(|it| feed_item_key(it)).collect();
                            let heights = heights_for(&keys, &row_heights.read(), est);
                            let (first, last, top_spacer, bot_spacer) =
                                measured_virtual_range(&heights, vp_top, vp_h);
                            let slice: Vec<&FeedItem> = filtered[first..last].to_vec();
                            rsx! {
                                div { class: "deck-column__virtual-spacer",
                                    style: "height: {top_spacer}px",
                                }
                                for item in slice.into_iter() {
                                    // Same post URI can appear twice in a
                                    // feed (e.g. two reposters surfaced it).
                                    // Disambiguate the key with the reposter
                                    // DID when present so Dioxus's keyed-diff
                                    // assertion holds. The wrapper carries
                                    // data-row-key so the scroll probe can
                                    // measure this row's real height; it's a
                                    // transparent block box (no margin/border)
                                    // so it doesn't change layout.
                                    div {
                                        class: "deck-column__vrow",
                                        key: "{feed_item_key(item)}",
                                        "data-row-key": "{feed_item_key(item)}",
                                        PostCard {
                                            post: item.post.clone(),
                                            reposter: feed_item_reposter(item),
                                            reply_parent_handle: feed_item_parent_handle(item),
                                        }
                                    }
                                }
                                div { class: "deck-column__virtual-spacer",
                                    style: "height: {bot_spacer}px",
                                }
                            }
                        }
                    }
                    (ColumnData::Notifications { groups, subjects }, _, _) => {
                        let (vp_top, vp_h, _) = *viewport.read();
                        let est = estimated_row_height_px(&spec.kind);
                        // Apply the column's notification-type filter
                        // (All / Mentions / Reactions) before virtualizing.
                        let nf = spec.settings.notif_filter;
                        let filtered: Vec<&NotificationGroup> = groups
                            .iter()
                            .filter(|g| passes_notif_filter(&g.reason, nf))
                            .collect();
                        let keys: Vec<String> = filtered
                            .iter()
                            .enumerate()
                            .map(|(i, g)| group_key(g, i))
                            .collect();
                        let heights = heights_for(&keys, &row_heights.read(), est);
                        let (first, last, top_spacer, bot_spacer) =
                            measured_virtual_range(&heights, vp_top, vp_h);
                        rsx! {
                            div { class: "deck-column__virtual-spacer",
                                style: "height: {top_spacer}px",
                            }
                            for (i, g) in filtered[first..last].iter().enumerate() {
                                div {
                                    class: "deck-column__vrow",
                                    key: "{group_key(g, first + i)}",
                                    "data-row-key": "{group_key(g, first + i)}",
                                    NotificationCard {
                                        group: (*g).clone(),
                                        subject: g.items.first().and_then(|n| subject_for(n, subjects)).cloned(),
                                    }
                                }
                            }
                            div { class: "deck-column__virtual-spacer",
                                style: "height: {bot_spacer}px",
                            }
                        }
                    }
                    (ColumnData::Suggestions(actors), _, _) => rsx! {
                        for a in actors.iter() {
                            crate::components::suggestion::SuggestionRow { key: "{a.did}", actor: a.clone() }
                        }
                    },
                    (ColumnData::Convos(convos), _, _) => {
                        // Identify the viewer's own DID so we can render
                        // the OTHER member in each 1:1 convo. (For
                        // group convos there can be more than one
                        // "other" — we just show the first for now.)
                        let me = session
                            .read()
                            .as_ref()
                            .map(|s| s.did.clone())
                            .unwrap_or_default();
                        let (vp_top, vp_h, _) = *viewport.read();
                        let row_h = estimated_row_height_px(&spec.kind);
                        let (first, last, top_spacer, bot_spacer) =
                            virtual_range(convos.len(), vp_top, vp_h, row_h);
                        rsx! {
                            div { class: "deck-column__virtual-spacer",
                                style: "height: {top_spacer}px",
                            }
                            for c in convos[first..last].iter() {
                                ConvoRow { key: "{c.id}", convo: c.clone(), me: me.clone() }
                            }
                            div { class: "deck-column__virtual-spacer",
                                style: "height: {bot_spacer}px",
                            }
                        }
                    }
                    (ColumnData::Inbox(items), _, _) => {
                        let (vp_top, vp_h, _) = *viewport.read();
                        let row_h = estimated_row_height_px(&spec.kind);
                        let (first, last, top_spacer, bot_spacer) =
                            virtual_range(items.len(), vp_top, vp_h, row_h);
                        rsx! {
                            div { class: "deck-column__virtual-spacer",
                                style: "height: {top_spacer}px",
                            }
                            for it in items[first..last].iter() {
                                InboxRow { key: "{it.item_id}", item: it.clone() }
                            }
                            div { class: "deck-column__virtual-spacer",
                                style: "height: {bot_spacer}px",
                            }
                        }
                    }
                    _ => rsx! {},
                }
                if let Some(msg) = &*error.read() {
                    if !data.read().is_empty() {
                        div { class: "deck-column__error deck-column__error--soft",
                            "Refresh failed: {msg}"
                        }
                    } else {
                        div { class: "deck-column__error",
                            "Failed to load: {msg}"
                        }
                    }
                }
                // Bottom indicator: "Load more" button when there's
                // more to fetch, "Loading more…" while in flight,
                // "End of feed" once we've exhausted the cursor,
                // "Scrollback cap reached" if we hit the per-column
                // memory ceiling. Renders for any paginated kind with
                // visible items — Posts (Home/Search/Feed/List/Author)
                // and Notifications.
                if is_paginated(&spec.kind) && !data.read().is_empty() {
                    if *loading_more.read() {
                        div { class: "deck-column__more", "Loading more…" }
                    } else if at_cap {
                        div { class: "deck-column__more deck-column__more--cap",
                            "Scrollback cap reached. Refresh to reset."
                        }
                    } else if *at_end.read() {
                        div { class: "deck-column__more", "End of feed." }
                    } else if show_load_more {
                        button { class: "deck-column__load-more",
                            onclick: load_more,
                            "Load more"
                        }
                    }
                }
            }
        }
    }
}

/// Per-kind estimated row height for the virtualized render path.
/// Rows actually vary in height (a text-only post is shorter than
/// one with an image grid + quote) but the estimate only has to be
/// close enough for the slice window to cover the viewport with
/// buffer. The buffer (2 viewports on each side) absorbs the drift.
pub fn estimated_row_height_px(kind: &ColumnKind) -> f64 {
    match kind {
        ColumnKind::Messages => 72.0,
        ColumnKind::Inbox => 90.0,
        ColumnKind::Notifications => 110.0,
        ColumnKind::Suggestions => 96.0,
        // Posts: home / search / feed / list / author. Real height
        // ranges ~120px (text-only) to ~500px+ (4-image grid + quote).
        // 240 is the rough median of a logged-in user's feed.
        _ => 240.0,
    }
}

/// Compute the visible-slice window for the virtualized render. Returns
/// (first_idx, last_idx, top_spacer_px, bot_spacer_px). The slice is
/// `[first..last]` (last exclusive). Spacer divs above/below preserve
/// the scrollbar geometry as if all `total` items were rendered.
pub fn virtual_range(
    total: usize,
    scroll_top: f64,
    client_h: f64,
    row_h: f64,
) -> (usize, usize, f64, f64) {
    if total == 0 || row_h <= 0.0 {
        return (0, 0, 0.0, 0.0);
    }
    // Cold-start (no viewport measurement yet): render a single
    // viewport's worth so the user has something to look at without
    // waiting for the post-mount eval round-trip. Without this the
    // first paint is empty.
    let effective_h = if client_h <= 0.0 { 800.0 } else { client_h };
    let items_per_vp = ((effective_h / row_h).ceil() as usize).max(1);
    // 2 viewports of buffer above + below. Enough that the user
    // would need to scroll-flick 2x screen-height to see a blank,
    // by which time onscroll has already updated the window.
    let buffer_items = items_per_vp.saturating_mul(2);
    let first_in_vp = (scroll_top.max(0.0) / row_h).floor() as usize;
    let first = first_in_vp.saturating_sub(buffer_items);
    let last = first_in_vp
        .saturating_add(items_per_vp)
        .saturating_add(buffer_items)
        .min(total);
    let top = first as f64 * row_h;
    let bot = total.saturating_sub(last) as f64 * row_h;
    (first, last, top, bot)
}

/// Measured-height variant of [`virtual_range`]. Instead of assuming a
/// single `row_h` for every row, it walks per-row `heights` (real
/// measured pixels where the row has been rendered + measured, an
/// estimate otherwise) to place the visible window exactly.
///
/// This is what kills the scroll "wiggle": with one fixed row height a
/// mixed feed (text ~120px vs a 4-image grid + quote ~500px+) makes
/// `scroll_top / row_h` drift from the true pixel position, so the
/// spacers don't match the rendered rows and the browser re-corrects
/// the scroll position every few rows. Walking real heights keeps the
/// spacers pixel-accurate, so the content stays put.
///
/// Returns `(first, last, top_spacer_px, bot_spacer_px)` — same
/// contract as [`virtual_range`]: render `[first..last]`, pad with the
/// two spacers. A `~1.5` viewport buffer is kept above and below the
/// strictly-visible window so a scroll-flick doesn't expose a blank.
pub fn measured_virtual_range(
    heights: &[f64],
    scroll_top: f64,
    client_h: f64,
) -> (usize, usize, f64, f64) {
    let total = heights.len();
    if total == 0 {
        return (0, 0, 0.0, 0.0);
    }
    let effective_h = if client_h <= 0.0 { 800.0 } else { client_h };
    let scroll_top = scroll_top.max(0.0);

    // First row whose bottom edge is past scroll_top — the first row at
    // least partly in the viewport.
    let mut acc = 0.0;
    let mut first_visible = total - 1;
    for (i, h) in heights.iter().enumerate() {
        if acc + h > scroll_top {
            first_visible = i;
            break;
        }
        acc += h;
    }

    // Last row needed to cover the viewport height downward.
    let mut covered = 0.0;
    let mut last_visible = first_visible + 1;
    for h in heights.iter().skip(first_visible) {
        covered += h;
        if covered >= effective_h {
            break;
        }
        last_visible += 1;
    }
    last_visible = last_visible.min(total);

    // Expand by ~1.5 viewports of buffer on each side (in px, since
    // rows vary in height).
    let buffer_px = effective_h * 1.5;
    let mut first = first_visible;
    let mut up = 0.0;
    while first > 0 && up < buffer_px {
        first -= 1;
        up += heights[first];
    }
    let mut last = last_visible;
    let mut down = 0.0;
    while last < total && down < buffer_px {
        down += heights[last];
        last += 1;
    }

    let top_spacer: f64 = heights[..first].iter().sum();
    let bot_spacer: f64 = heights[last..].iter().sum();
    (first, last, top_spacer, bot_spacer)
}

/// JS for the scroll/mount probe: returns `[scrollTop, clientHeight,
/// scrollHeight, {rowKey: offsetHeight, …}]`, or `null` if the body
/// isn't in the DOM. The row map drives [`measured_virtual_range`];
/// only rows tagged with `data-row-key` (the virtualized lists) are
/// measured. `offsetHeight` is an int so it forces a layout flush once
/// per probe — cheap, and the probe is already throttled by
/// `scroll_check_pending`.
fn probe_js(sel: &str) -> String {
    format!(
        r#"
        const el = document.querySelector({sel});
        if (!el) {{ dioxus.send(null); }}
        else {{
            const rows = {{}};
            el.querySelectorAll('[data-row-key]').forEach(function(r) {{
                rows[r.getAttribute('data-row-key')] = r.offsetHeight;
            }});
            dioxus.send([el.scrollTop, el.clientHeight, el.scrollHeight, rows]);
        }}
        "#,
        sel = serde_json::to_string(sel).unwrap_or_else(|_| "\"\"".to_string()),
    )
}

/// Pull `(scroll_top, client_h, scroll_h)` out of a [`probe_js`]
/// response. `None` for a missing body / malformed payload.
fn parse_probe_geometry(v: &serde_json::Value) -> Option<(f64, f64, f64)> {
    let arr = v.as_array()?;
    let st = arr.first()?.as_f64()?;
    let ch = arr.get(1)?.as_f64()?;
    let sh = arr.get(2)?.as_f64()?;
    if st < 0.0 {
        return None;
    }
    Some((st, ch, sh))
}

/// Fold measured row heights from a [`probe_js`] response into the
/// per-column height map. Diff-guarded: only writes when a height
/// actually changed by >0.5px, so it can't spin a render→measure→
/// render loop (a write would re-render, re-probe, and—if nothing
/// moved—find no change and stop).
fn apply_measured_heights(v: &serde_json::Value, mut row_heights: Signal<HashMap<String, f64>>) {
    let Some(rows) = v
        .as_array()
        .and_then(|a| a.get(3))
        .and_then(|r| r.as_object())
    else {
        return;
    };
    let changed = {
        let cur = row_heights.peek();
        rows.iter().any(|(k, val)| {
            val.as_f64()
                .filter(|h| *h > 0.0)
                .is_some_and(|h| cur.get(k).is_none_or(|old| (old - h).abs() > 0.5))
        })
    };
    if !changed {
        return;
    }
    let mut w = row_heights.write();
    for (k, val) in rows {
        if let Some(h) = val.as_f64().filter(|h| *h > 0.0) {
            w.insert(k.clone(), h);
        }
    }
}

/// Build the per-row height vector for a list of stable keys, using the
/// measured height where known and `estimate` otherwise. This is the
/// input to [`measured_virtual_range`].
fn heights_for(keys: &[String], measured: &HashMap<String, f64>, estimate: f64) -> Vec<f64> {
    keys.iter()
        .map(|k| measured.get(k).copied().unwrap_or(estimate))
        .collect()
}

/// True when the column supports cursor-based fetch_more on scroll.
/// Suggestions has its own pagination semantics (single page of
/// personalized actors). Messages is paginated by the chat lexicon
/// but we cap at the first 50 convos for v1 — most inboxes fit, and
/// Bluesky doesn't yet support search-within-DMs that would justify
/// scrolling further.
fn is_paginated(kind: &ColumnKind) -> bool {
    matches!(
        kind,
        ColumnKind::Home
            | ColumnKind::AuthorFeed { .. }
            | ColumnKind::Search { .. }
            | ColumnKind::Feed { .. }
            | ColumnKind::List { .. }
            | ColumnKind::Notifications
    )
}

/// One convo row in the Messages column. Avatar + handle/displayName
/// of the OTHER member + last-message preview + unread badge. Click
/// opens the inline [`MessagesSheet`] for that convo. For a 1:1 DM we
/// show the non-self member; for a group convo, the first non-self
/// member (the row caption notes "and N others").
#[component]
fn ConvoRow(convo: smooblue_atproto::ConvoView, me: String) -> Element {
    let convo_id = convo.id.clone();
    let mut messages_focus = use_context::<Signal<crate::state::MessagesFocus>>();
    let unread = convo.unread_count;
    // Pick the first member whose DID isn't ours; fall back to the
    // first member if the convo is somehow degenerate (e.g. our own
    // notes-to-self if Bluesky ever adds that).
    let other = convo
        .members
        .iter()
        .find(|m| m.did != me)
        .or_else(|| convo.members.first())
        .cloned();
    let display = other.as_ref().map(|p| {
        p.display_name
            .clone()
            .unwrap_or_else(|| format!("@{}", p.handle))
    });
    let handle = other.as_ref().map(|p| format!("@{}", p.handle));
    let others_caption = if convo.members.len() > 2 {
        Some(format!(" and {} others", convo.members.len() - 2))
    } else {
        None
    };
    let avatar = other.as_ref().and_then(|p| p.avatar.clone());
    // last_message can be a tombstone for a deleted message — render
    // an italic placeholder for those, otherwise the message text
    // (collapsed to a single line).
    let preview = convo.last_message.as_ref().map(|m| match m {
        smooblue_atproto::Message::Live(v) => v.text.clone(),
        smooblue_atproto::Message::Deleted(_) => "(message deleted)".to_string(),
    });

    let onclick = move |_| {
        messages_focus.set(crate::state::MessagesFocus(Some(convo_id.clone())));
    };

    rsx! {
        button { class: "convo-row",
            onclick: onclick,
            div { class: "convo-row__avatar",
                if let Some(u) = avatar.as_ref() {
                    img { src: "{u}", alt: "" }
                } else {
                    div { class: "convo-row__avatar-placeholder" }
                }
            }
            div { class: "convo-row__body",
                div { class: "convo-row__head",
                    if let Some(d) = display.as_ref() {
                        span { class: "convo-row__display", "{d}" }
                    }
                    if let Some(h) = handle.as_ref() {
                        span { class: "convo-row__handle", "{h}" }
                    }
                    if let Some(o) = others_caption.as_ref() {
                        span { class: "convo-row__others", "{o}" }
                    }
                }
                if let Some(p) = preview.as_ref() {
                    div { class: "convo-row__preview", "{p}" }
                }
            }
            if unread > 0 {
                span { class: "convo-row__badge", "{unread}" }
            }
        }
    }
}

/// One row in the Inbox triage column. Avatar + actor + action
/// caption + preview + age + source chip + triage actions visible
/// on hover (reply / snooze / archive). Click body → opens
/// thread/convo + marks read. Snooze opens a 4-option dropdown.
/// Reply on a DM expands an inline textarea + Send button; reply
/// on a post just opens the thread sheet so the existing PostCard
/// flow handles the composing.
#[component]
fn InboxRow(item: crate::inbox::InboxItem) -> Element {
    use crate::inbox::InboxSource;
    let mut thread_focus = use_context::<Signal<crate::state::ThreadFocus>>();
    let mut messages_focus = use_context::<Signal<crate::state::MessagesFocus>>();
    let session_sig = use_context::<Signal<Option<smooblue_oauth::Session>>>();
    // For the quick-reply "pop out" — hand the draft + reply target to
    // the full composer so rich replies (images / facets / quote) can
    // continue there.
    let mut compose_ctx = use_context::<Signal<crate::state::ComposeContext>>();
    let is_post_source = !matches!(item.source, InboxSource::Dm);

    // Optimistic hide: archive / snooze set this true; row disappears
    // immediately. The 15s column poll picks up the persisted state
    // from disk and confirms.
    let mut hidden = use_signal(|| false);
    let mut snooze_menu_open = use_signal(|| false);
    let mut reply_open = use_signal(|| false);
    let mut reply_draft = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let mut send_error = use_signal::<Option<String>>(|| None);
    // Optimistic read: mark-as-read button flips this immediately so
    // the row's --read styling applies before the 15s poll re-reads
    // from SQLite.
    let mut read_now = use_signal(|| item.read);

    if *hidden.read() {
        return rsx! { Fragment {} };
    }

    let actor_name = item.actor_display_name.clone().unwrap_or_else(|| {
        item.actor_handle
            .clone()
            .map(|h| format!("@{h}"))
            .unwrap_or_else(|| item.actor_did.clone())
    });

    let (action_label, source_chip) = match item.source {
        InboxSource::DirectReply => ("replied", "reply"),
        InboxSource::ReplyToReply => ("replied in your thread", "reply"),
        InboxSource::Quote => ("quoted your post", "quote"),
        InboxSource::Mention => ("mentioned you", "mention"),
        InboxSource::Dm => ("sent a DM", "DM"),
    };

    let id_for_click = item.item_id.clone();
    let subject_for_click = item.subject_uri.clone();
    let source_for_click = item.source;
    let on_row_click = move |_| {
        let id = id_for_click.clone();
        spawn(async move {
            let _ = tokio::task::spawn_blocking(move || crate::inbox::set_read(&id, true)).await;
        });
        match source_for_click {
            InboxSource::Dm => {
                messages_focus.set(crate::state::MessagesFocus(Some(subject_for_click.clone())));
            }
            _ => {
                thread_focus.set(crate::state::ThreadFocus(Some(subject_for_click.clone())));
            }
        }
    };

    let id_for_archive = item.item_id.clone();
    let on_archive = move |e: MouseEvent| {
        e.stop_propagation();
        hidden.set(true);
        let id = id_for_archive.clone();
        spawn(async move {
            let res = tokio::task::spawn_blocking(move || crate::inbox::set_archived(&id, true))
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("blocking task panicked: {e}")));
            if let Err(err) = res {
                tracing::warn!(error = %err, "inbox: archive failed");
            }
        });
    };

    let on_snooze_toggle = move |e: MouseEvent| {
        e.stop_propagation();
        snooze_menu_open.with_mut(|v| *v = !*v);
        reply_open.set(false);
    };

    let on_reply_toggle = move |e: MouseEvent| {
        e.stop_propagation();
        // Quick plain-text reply inline (posts and DMs alike). Rich
        // replies — images / facets / quote — escalate via the "pop
        // out" button (posts) which hands the draft to the full
        // composer; row-click still opens the ThreadSheet.
        reply_open.with_mut(|v| *v = !*v);
        snooze_menu_open.set(false);
    };

    // Per-callback closures rather than a shared inner closure
    // because the shared variant would capture String (`item_id`)
    // which isn't Copy, so only the first of the 4 menu buttons
    // could move it. Inlining via the helper fn keeps things tight.
    let id_1h = item.item_id.clone();
    let on_snooze_1h = move |e: MouseEvent| {
        e.stop_propagation();
        schedule_snooze(id_1h.clone(), 1, hidden, snooze_menu_open);
    };
    let id_4h = item.item_id.clone();
    let on_snooze_4h = move |e: MouseEvent| {
        e.stop_propagation();
        schedule_snooze(id_4h.clone(), 4, hidden, snooze_menu_open);
    };
    let id_tomorrow = item.item_id.clone();
    let on_snooze_tomorrow = move |e: MouseEvent| {
        e.stop_propagation();
        schedule_snooze(id_tomorrow.clone(), 24, hidden, snooze_menu_open);
    };
    let id_monday = item.item_id.clone();
    let on_snooze_monday = move |e: MouseEvent| {
        e.stop_propagation();
        // Hours until next Monday — approximate via local weekday.
        // Snooze precision is "later today / next week"; a few
        // hours' drift doesn't matter for triage UX.
        use chrono::Datelike;
        let weekday = chrono::Local::now().weekday().num_days_from_monday() as i64;
        let days_until_monday = if weekday == 0 { 7 } else { 7 - weekday };
        schedule_snooze(
            id_monday.clone(),
            days_until_monday * 24,
            hidden,
            snooze_menu_open,
        );
    };

    let subject_for_send = item.subject_uri.clone();
    let id_for_send = item.item_id.clone();
    let on_send = move |e: MouseEvent| {
        e.stop_propagation();
        let text = reply_draft.read().trim().to_string();
        if text.is_empty() || *sending.read() {
            return;
        }
        sending.set(true);
        send_error.set(None);
        let subject = subject_for_send.clone();
        let src = source_for_click;
        let item_id = id_for_send.clone();
        spawn(async move {
            let Some(client) = crate::auth_refresh::fresh_client(session_sig).await else {
                send_error.set(Some("not signed in".into()));
                sending.set(false);
                return;
            };
            let result: Result<(), String> = if matches!(src, InboxSource::Dm) {
                let input = smooblue_atproto::MessageInput {
                    text,
                    facets: None,
                    embed: None,
                };
                client
                    .chat_send_message(&subject, &input)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                // Plain-text reply to the subject post. We only need the
                // post's cid; mirror the composer's root = parent = target
                // simplification (see compose.rs / ReplyRef docs).
                match client.get_posts(std::slice::from_ref(&subject)).await {
                    Ok(posts) => match posts.into_iter().next() {
                        Some(p) => {
                            let r = smooblue_atproto::ReplyRef {
                                root: smooblue_atproto::StrongRef {
                                    uri: subject.clone(),
                                    cid: p.cid.clone(),
                                },
                                parent: smooblue_atproto::StrongRef {
                                    uri: subject.clone(),
                                    cid: p.cid.clone(),
                                },
                            };
                            client
                                .create_post_full(&text, Some(&r), &[], &[], None, None, None)
                                .await
                                .map(|_| ())
                                .map_err(|e| e.to_string())
                        }
                        None => Err("could not load the post to reply to".into()),
                    },
                    Err(e) => Err(e.to_string()),
                }
            };
            match result {
                Ok(()) => {
                    reply_draft.set(String::new());
                    reply_open.set(false);
                    // Replying is triage-complete — mark read.
                    read_now.set(true);
                    let id = item_id.clone();
                    let _ = tokio::task::spawn_blocking(move || crate::inbox::set_read(&id, true))
                        .await;
                }
                Err(e) => send_error.set(Some(e)),
            }
            sending.set(false);
        });
    };

    // "Pop out" the in-progress reply to the full composer (rich
    // replies — images / facets / quote). Needs the subject's cid, so
    // a quick get_posts, then seed ComposeContext with the reply target
    // + the draft prefill and open it.
    let subject_for_popout = item.subject_uri.clone();
    let handle_for_popout = item.actor_handle.clone().unwrap_or_default();
    let on_reply_popout = move |e: MouseEvent| {
        e.stop_propagation();
        let draft = reply_draft.read().clone();
        let subject = subject_for_popout.clone();
        let handle = handle_for_popout.clone();
        reply_open.set(false);
        spawn(async move {
            let Some(client) = crate::auth_refresh::fresh_client(session_sig).await else {
                return;
            };
            if let Ok(posts) = client.get_posts(std::slice::from_ref(&subject)).await {
                if let Some(p) = posts.into_iter().next() {
                    compose_ctx.with_mut(|w| {
                        w.reply_to = Some(crate::state::ReplyTarget {
                            uri: subject.clone(),
                            cid: p.cid.clone(),
                            handle: handle.clone(),
                            text: String::new(),
                        });
                        w.quote_to = None;
                        w.prefill = (!draft.trim().is_empty()).then(|| draft.clone());
                        w.open = true;
                    });
                }
            }
        });
    };

    let id_for_mark_read = item.item_id.clone();
    let on_mark_read = move |e: MouseEvent| {
        e.stop_propagation();
        read_now.set(true);
        let id = id_for_mark_read.clone();
        spawn(async move {
            let res = tokio::task::spawn_blocking(move || crate::inbox::set_read(&id, true))
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("blocking task panicked: {e}")));
            if let Err(err) = res {
                tracing::warn!(error = %err, "inbox: mark-as-read failed");
            }
        });
    };

    let age = relative_age(item.ts);
    let is_read = *read_now.read();
    let row_class = if is_read {
        "inbox-row inbox-row--read"
    } else {
        "inbox-row"
    };

    // Clout signal: follower count + a followers:following ratio. A big
    // follower count built by mass-following everyone (ratio ≈ 1) isn't
    // the clout of a big count with few follows, so we surface both and
    // flag the likely over-followers. None until the profile-enrichment
    // pass fills the counts (both default 0).
    let clout: Option<(String, bool)> = (item.actor_follower_count > 0).then(|| {
        let followers = item.actor_follower_count;
        let follows = item.actor_follows_count;
        let label = if follows > 0 {
            let ratio = followers as f64 / follows as f64;
            let r = if ratio >= 10.0 {
                format!("{ratio:.0}×")
            } else {
                format!("{ratio:.1}×")
            };
            format!("{} · {}", humanize_count(followers), r)
        } else {
            humanize_count(followers)
        };
        // Over-follower heuristic: follows a lot AND the ratio is near
        // parity — a "big" account whose reach is mostly follow-back.
        let low = follows >= 1000 && (followers as f64) < follows as f64 * 1.5;
        (label, low)
    });

    rsx! {
        div { class: "inbox-row__wrap",
            div { class: "{row_class}",
                onclick: on_row_click,
                div { class: "inbox-row__avatar",
                    if let Some(u) = item.actor_avatar.as_ref() {
                        img { src: "{u}", alt: "" }
                    } else {
                        div { class: "inbox-row__avatar-placeholder" }
                    }
                }
                div { class: "inbox-row__body",
                    div { class: "inbox-row__head",
                        span { class: "inbox-row__actor", "{actor_name}" }
                        span { class: "inbox-row__action", " {action_label}" }
                        span { class: "inbox-row__chip", "{source_chip}" }
                        span { class: "inbox-row__age", "· {age}" }
                        if let Some((clout_label, low)) = clout.as_ref() {
                            span {
                                class: if *low { "inbox-row__clout inbox-row__clout--low" } else { "inbox-row__clout" },
                                title: "Followers · followers-to-following ratio",
                                "{clout_label}"
                            }
                        }
                    }
                    if let Some(p) = item.preview.as_ref() {
                        div { class: "inbox-row__preview", "{p}" }
                    }
                }
                div { class: "inbox-row__actions",
                    button {
                        class: "inbox-row__action-btn",
                        title: "Reply",
                        onclick: on_reply_toggle,
                        icons::MessageQuote { size: icons::Size::Sm }
                    }
                    button {
                        class: "inbox-row__action-btn",
                        title: "Snooze",
                        onclick: on_snooze_toggle,
                        icons::Clock { size: icons::Size::Sm }
                    }
                    // Always render so the affordance is discoverable.
                    // Dims (--done class) once the row is read; clicking
                    // an already-read row is a no-op via the disabled
                    // attribute (set_read(true) on a read row would be
                    // wasted writes + needless sync churn).
                    button {
                        class: if is_read { "inbox-row__action-btn inbox-row__action-btn--done" } else { "inbox-row__action-btn" },
                        title: if is_read { "Read" } else { "Mark as read" },
                        disabled: is_read,
                        onclick: on_mark_read,
                        icons::Check { size: icons::Size::Sm }
                    }
                    button {
                        class: "inbox-row__action-btn",
                        title: "Archive",
                        onclick: on_archive,
                        icons::Archive { size: icons::Size::Sm }
                    }
                }
                if !is_read {
                    span { class: "inbox-row__unread-dot", title: "Unread" }
                }
            }
            if *snooze_menu_open.read() {
                div { class: "inbox-row__snooze-menu",
                    onclick: move |e| e.stop_propagation(),
                    button { class: "inbox-row__snooze-item", onclick: on_snooze_1h, "1 hour" }
                    button { class: "inbox-row__snooze-item", onclick: on_snooze_4h, "4 hours" }
                    button { class: "inbox-row__snooze-item", onclick: on_snooze_tomorrow, "Tomorrow" }
                    button { class: "inbox-row__snooze-item", onclick: on_snooze_monday, "Monday" }
                }
            }
            if *reply_open.read() {
                div { class: "inbox-row__reply",
                    onclick: move |e| e.stop_propagation(),
                    textarea {
                        class: "input inbox-row__reply-input",
                        placeholder: "Write a reply…",
                        value: "{reply_draft.read()}",
                        oninput: move |e| reply_draft.set(e.value()),
                    }
                    if let Some(err) = send_error.read().as_ref() {
                        div { class: "inbox-row__reply-error", "Send failed: {err}" }
                    }
                    div { class: "inbox-row__reply-actions",
                        if is_post_source {
                            button {
                                class: "btn btn--ghost inbox-row__reply-popout",
                                title: "Open in the full composer — images, links, quote",
                                onclick: on_reply_popout,
                                "Pop out ↗"
                            }
                        }
                        button {
                            class: "btn btn--primary",
                            disabled: *sending.read() || reply_draft.read().trim().is_empty(),
                            onclick: on_send,
                            if *sending.read() { "Sending…" } else { "Send" }
                        }
                    }
                }
            }
        }
    }
}

/// Common snooze action: hide locally, write the DB. Extracted as a
/// fn (not a captured closure) so the per-button callbacks can each
/// reference it without taking ownership of a non-Copy String.
fn schedule_snooze(
    item_id: String,
    hours: i64,
    mut hidden: Signal<bool>,
    mut snooze_menu: Signal<bool>,
) {
    let until = Some(chrono::Utc::now() + chrono::Duration::hours(hours));
    snooze_menu.set(false);
    hidden.set(true);
    spawn(async move {
        let res = tokio::task::spawn_blocking(move || crate::inbox::set_snoozed(&item_id, until))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("blocking task panicked: {e}")));
        if let Err(err) = res {
            tracing::warn!(error = %err, "inbox: snooze failed");
        }
    });
}

/// Compact follower-count formatting for the inbox clout badge:
/// `850`, `12.4k`, `1.2M`. Trailing `.0` is dropped so round numbers
/// read `12k` not `12.0k`.
fn humanize_count(n: i64) -> String {
    fn trim(v: f64) -> String {
        let s = format!("{v:.1}");
        s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
    }
    if n >= 1_000_000 {
        format!("{}M", trim(n as f64 / 1_000_000.0))
    } else if n >= 1_000 {
        format!("{}k", trim(n as f64 / 1_000.0))
    } else {
        n.to_string()
    }
}

/// Render an absolute timestamp as a short relative age (e.g. "3m",
/// "2h", "5d"). Mirrors the Twitter/Bluesky time chip style.
fn relative_age(ts: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let dur = now - ts;
    if dur.num_seconds() < 60 {
        format!("{}s", dur.num_seconds().max(1))
    } else if dur.num_minutes() < 60 {
        format!("{}m", dur.num_minutes())
    } else if dur.num_hours() < 24 {
        format!("{}h", dur.num_hours())
    } else if dur.num_days() < 7 {
        format!("{}d", dur.num_days())
    } else {
        ts.format("%b %d").to_string()
    }
}

/// One page of results from `fetch_page` — the data view + the cursor
/// the AppView gave us for the next page (None ⇒ end of feed).
struct Page {
    data: ColumnData,
    cursor: Option<String>,
}

/// One fetch cycle for the column at a given cursor position.
/// `cursor: None` ⇒ top of feed; `cursor: Some(c)` ⇒ continue from c.
/// Returns both the data and the cursor for the page below this one.
async fn fetch_page(
    kind: &ColumnKind,
    session_sig: Signal<Option<Session>>,
    cursor: Option<String>,
    subjects_cache: &mut HashMap<String, PostView>,
) -> Result<Page, String> {
    // Demo mode: canned data, no cursor — second fetch_more call
    // returns an empty page so the column shows "End of feed".
    if crate::demo::is_active() {
        let data = match kind {
            ColumnKind::Notifications => {
                let (items, subjects) = crate::demo::notifications_with_subjects();
                let groups = group_notifications(items);
                ColumnData::Notifications { groups, subjects }
            }
            ColumnKind::AuthorFeed { .. } => ColumnData::Posts(crate::demo::home_feed()),
            ColumnKind::Suggestions => ColumnData::Suggestions(crate::demo::suggestions()),
            // Demo mode shows an empty inbox — no canned convos yet.
            ColumnKind::Messages => ColumnData::Convos(Vec::new()),
            // Demo mode: empty inbox triage list. The SQLite store is
            // user-real anyway; demo just doesn't seed canned items.
            ColumnKind::Inbox => ColumnData::Inbox(Vec::new()),
            ColumnKind::Home
            | ColumnKind::Search { .. }
            | ColumnKind::Feed { .. }
            | ColumnKind::List { .. } => {
                if cursor.is_some() {
                    // Fake pagination in demo: empty page on
                    // fetch_more so the indicator lands at "End".
                    ColumnData::Posts(Vec::new())
                } else {
                    ColumnData::Posts(crate::demo::home_feed())
                }
            }
        };
        return Ok(Page { data, cursor: None });
    }
    // OAuth-authenticated calls hit the user's PDS (which proxies app.bsky.*
    // to the AppView with service-auth on our behalf). Hitting the AppView
    // directly with a user token returns 401 AuthMissing.
    //
    // fresh_client transparently refreshes the access token if it's
    // expired/expiring so long-running polling loops survive across
    // the ~2h token TTL without the user getting silently booted.
    let Some(client) = fresh_client(session_sig).await else {
        return Err("not signed in".into());
    };
    let cur = cursor.as_deref();
    match kind {
        ColumnKind::Home => client
            .get_timeline(cur, PAGE_SIZE)
            .await
            .map(|r| Page {
                data: ColumnData::Posts(r.feed),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::AuthorFeed { actor } => client
            .get_author_feed(actor, cur, PAGE_SIZE)
            .await
            .map(|r| Page {
                data: ColumnData::Posts(r.feed),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::Notifications => {
            // 30 per fetch is the sweet spot: enough to give the user
            // a meaningful window, small enough that the cascade of
            // get_posts hydration + grouping + per-card clones stays
            // snappy. 50 was visibly laggy on busy accounts.
            let resp = client
                .list_notifications(cur, 30)
                .await
                .map_err(|e| e.to_string())?;
            let next_cursor = resp.cursor;
            let items = resp.notifications;
            // Hydrate subject posts in one batched call — but only
            // the URIs we don't already have cached from a prior poll.
            // For a notification-heavy user this can drop the per-
            // poll get_posts payload from ~30 URIs to ~2.
            let needed: Vec<String> = collect_subject_uris(&items)
                .into_iter()
                .filter(|u| !subjects_cache.contains_key(u))
                .collect();
            if !needed.is_empty() {
                if let Ok(posts) = client.get_posts(&needed).await {
                    for p in posts {
                        subjects_cache.insert(p.uri.clone(), p);
                    }
                }
            }
            // Crude bounded-cache: blow it away when we hit 500 entries.
            // A real LRU is overkill — a notification page can't reference
            // more than ~30 subjects so the cap is generous.
            if subjects_cache.len() > 500 {
                subjects_cache.clear();
            }
            // Collapse 20 likes on the same post into one card etc.
            // Done after hydration so the same subjects map keys still work.
            let groups = group_notifications(items);
            Ok(Page {
                data: ColumnData::Notifications {
                    groups,
                    subjects: subjects_cache.clone(),
                },
                cursor: next_cursor,
            })
        }
        ColumnKind::Search { query } => client
            .search_posts(query, cur, PAGE_SIZE)
            .await
            .map(|r| Page {
                data: ColumnData::Posts(r.feed),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::Feed { uri } => client
            .get_feed(uri, cur, PAGE_SIZE)
            .await
            .map(|r| Page {
                data: ColumnData::Posts(r.feed),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::List { uri } => client
            .get_list_feed(uri, cur, PAGE_SIZE)
            .await
            .map(|r| Page {
                data: ColumnData::Posts(r.feed),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::Suggestions => client
            .get_suggestions(cur, 25)
            .await
            .map(|r| Page {
                data: ColumnData::Suggestions(r.actors),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::Messages => client
            .chat_list_convos(cur, 50)
            .await
            .map(|r| Page {
                data: ColumnData::Convos(r.convos),
                cursor: r.cursor,
            })
            .map_err(|e| e.to_string()),
        ColumnKind::Inbox => {
            // Inbox doesn't hit the network here — it reads from the
            // local SQLite store. Ingestion (inbox_ingest.rs) is the
            // side that polls the AppView / chat service. 500-item
            // cap covers the active-triage UX even for accounts that
            // have backfilled a lot (older items eventually fall off
            // the bottom as bucket-DESC sort favors freshness).
            // Cursor-based scroll-load on the column itself is
            // tracked as pearl th-f5d4f4.
            let _ = client; // silence unused-variable when this arm is the only one to compile
            tokio::task::spawn_blocking(|| crate::inbox::list_active(500))
                .await
                .map_err(|e| format!("inbox list task panicked: {e}"))?
                .map(|items| Page {
                    data: ColumnData::Inbox(items),
                    cursor: None,
                })
                .map_err(|e| e.to_string())
        }
    }
}

/// Merge a fresh top-of-feed page into the existing item list.
/// Fresh page items are newest-first; any whose dedupe key isn't in
/// `existing` get prepended (preserving fresh's relative order).
/// Existing items keep their tail. Capped at `cap` from the head if
/// the merged result is too long — the cap-as-policy is "newer wins
/// when we have to choose."
fn merge_top_page(existing: Vec<FeedItem>, fresh: Vec<FeedItem>, cap: usize) -> Vec<FeedItem> {
    use std::collections::HashSet;
    let existing_keys: HashSet<String> = existing.iter().map(feed_item_key).collect();
    let mut new_items: Vec<FeedItem> = fresh
        .into_iter()
        .filter(|item| !existing_keys.contains(&feed_item_key(item)))
        .collect();
    new_items.extend(existing);
    if new_items.len() > cap {
        new_items.truncate(cap);
    }
    new_items
}

/// Merge a fresh top-of-feed Notifications page into the existing
/// group list. New unique groups go at the head (preserving fresh
/// order). For new groups whose key matches an existing one (same
/// reason+subject — e.g. another like on the same post), the new
/// items are spliced into the existing group's `items` at the front
/// (newest-first), with the existing group keeping its position so
/// the user's scroll doesn't jump. Cap from the head if the merged
/// result is too long.
fn merge_top_notif_groups(
    existing: Vec<NotificationGroup>,
    fresh: Vec<NotificationGroup>,
    cap: usize,
) -> Vec<NotificationGroup> {
    use std::collections::{HashMap, HashSet};

    fn key(g: &NotificationGroup) -> (String, Option<String>) {
        (g.reason.clone(), g.reason_subject.clone())
    }

    // Index existing groups by key for O(1) merge lookup.
    let mut existing: Vec<NotificationGroup> = existing;
    let mut existing_idx: HashMap<(String, Option<String>), usize> = existing
        .iter()
        .enumerate()
        .map(|(i, g)| (key(g), i))
        .collect();

    // Split fresh into "merge into existing" vs "new groups to prepend."
    let mut to_prepend: Vec<NotificationGroup> = Vec::new();
    for fresh_group in fresh {
        let k = key(&fresh_group);
        if let Some(&idx) = existing_idx.get(&k) {
            // Merge: prepend fresh items (newest-first), dedupe by uri+cid.
            let mut seen: HashSet<(String, String)> = existing[idx]
                .items
                .iter()
                .map(|n| (n.uri.clone(), n.cid.clone()))
                .collect();
            let mut new_items: Vec<Notification> = fresh_group
                .items
                .into_iter()
                .filter(|n| seen.insert((n.uri.clone(), n.cid.clone())))
                .collect();
            new_items.extend(std::mem::take(&mut existing[idx].items));
            existing[idx].items = new_items;
            if fresh_group.latest_at.is_some() {
                existing[idx].latest_at = fresh_group.latest_at;
            }
        } else {
            to_prepend.push(fresh_group);
        }
    }
    // Prepend the new groups; existing index becomes stale but unused.
    let _ = &mut existing_idx;
    to_prepend.extend(existing);
    if to_prepend.len() > cap {
        to_prepend.truncate(cap);
    }
    to_prepend
}

/// Append a bottom-of-feed page (older items) to the existing list.
/// De-dupe by key. Respects the cap — drops any items from `more`
/// that would push us over the limit (refuse-rather-than-evict so a
/// user scrolled into the deep tail isn't surprised by content
/// disappearing).
fn append_bottom_page(
    mut existing: Vec<FeedItem>,
    more: Vec<FeedItem>,
    cap: usize,
) -> Vec<FeedItem> {
    use std::collections::HashSet;
    let existing_keys: HashSet<String> = existing.iter().map(feed_item_key).collect();
    let room = cap.saturating_sub(existing.len());
    for item in more
        .into_iter()
        .filter(|item| !existing_keys.contains(&feed_item_key(item)))
        .take(room)
    {
        existing.push(item);
    }
    existing
}

/// Append an older-notifications page to the existing list of groups.
/// For each new group, if a group with the same `(reason, reason_subject)`
/// key already exists, merge the new (older) items into that existing
/// group — keeps "20 people liked your post" rolled into one card
/// even when half the likers came in on a later page. Otherwise
/// append at the bottom. Respects the cap (refuse-rather-than-evict).
fn append_bottom_notif_groups(
    mut existing: Vec<NotificationGroup>,
    more: Vec<NotificationGroup>,
    cap: usize,
) -> Vec<NotificationGroup> {
    use std::collections::HashSet;

    fn group_key_pair(g: &NotificationGroup) -> (String, Option<String>) {
        (g.reason.clone(), g.reason_subject.clone())
    }

    // Index existing groups by their dedup key for O(1) lookup.
    let mut existing_idx: std::collections::HashMap<(String, Option<String>), usize> =
        std::collections::HashMap::new();
    for (i, g) in existing.iter().enumerate() {
        existing_idx.insert(group_key_pair(g), i);
    }

    for new_group in more {
        let key = group_key_pair(&new_group);
        if let Some(&idx) = existing_idx.get(&key) {
            // Merge new items into the existing group, deduping by
            // the item's own uri+cid (a single notification can show
            // up on adjacent pages if a new one arrived between our
            // top-poll and the backfill fetch).
            let mut seen: HashSet<(String, String)> = existing[idx]
                .items
                .iter()
                .map(|n| (n.uri.clone(), n.cid.clone()))
                .collect();
            for item in new_group.items {
                let id = (item.uri.clone(), item.cid.clone());
                if seen.insert(id) {
                    existing[idx].items.push(item);
                }
            }
        } else if existing.len() < cap {
            existing_idx.insert(key, existing.len());
            existing.push(new_group);
        }
        // else: silently drop — the cap message in the column footer
        // tells the user why no more groups are coming in.
    }
    existing
}

/// Stable key for a feed row. URI alone isn't unique (a post can
/// appear twice when surfaced by two different reposters), so we
/// suffix the repost actor DID when present.
fn feed_item_key(item: &smooblue_atproto::FeedItem) -> String {
    match item.reposter_did() {
        Some(did) => format!("{}|rp:{}", item.post.uri, did),
        None => item.post.uri.clone(),
    }
}

fn feed_item_reposter(item: &smooblue_atproto::FeedItem) -> Option<String> {
    item.reposter_display()
}

/// True when the item matches a (case-insensitive, already-lowercased)
/// filter substring. Checks post text, author handle, author display
/// name, reposter display name, and reply-parent handle so the
/// user's mental model of "filter to anything with X in it" works.
/// True if a feed item carries real media — an image grid, a video, an
/// external link card, or record-with-media. A bare quote (record-only)
/// embed does NOT count as media. Drives the media-only / text-only
/// column filters.
pub fn item_has_media(item: &smooblue_atproto::FeedItem) -> bool {
    use smooblue_atproto::{Embed, EmbedKind};
    matches!(
        &item.post.embed,
        Some(Embed::Known(
            EmbedKind::Images { .. }
                | EmbedKind::Video { .. }
                | EmbedKind::External { .. }
                | EmbedKind::RecordWithMedia { .. }
        ))
    )
}

/// Apply a column's structured feed filters (hide reposts / replies,
/// media-only / text-only) to one item. Returns true to keep it.
pub fn passes_feed_settings(item: &smooblue_atproto::FeedItem, s: &ColumnSettings) -> bool {
    if s.hide_reposts && item.reposter_did().is_some() {
        return false;
    }
    if s.hide_replies && item.reply.is_some() {
        return false;
    }
    let media = item_has_media(item);
    if s.media_only && !media {
        return false;
    }
    if s.text_only && media {
        return false;
    }
    true
}

/// Whether a notification group passes the column's notification-type
/// filter, keyed off the group's reason.
pub fn passes_notif_filter(reason: &str, f: NotifFilter) -> bool {
    match f {
        NotifFilter::All => true,
        NotifFilter::Mentions => matches!(reason, "reply" | "mention" | "quote"),
        NotifFilter::Reactions => matches!(
            reason,
            "like" | "like-via-repost" | "repost" | "repost-via-repost" | "follow"
        ),
    }
}

pub fn feed_item_matches(item: &smooblue_atproto::FeedItem, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    // Bind owned Options into locals so .as_deref() borrows from
    // them instead of dropped temporaries.
    let reposter = item.reposter_display();
    let parent = item.reply_parent_handle();
    let haystacks: [&str; 5] = [
        item.post.record.text.as_str(),
        item.post.author.handle.as_str(),
        item.post.author.display_name.as_deref().unwrap_or(""),
        reposter.as_deref().unwrap_or(""),
        parent.as_deref().unwrap_or(""),
    ];
    haystacks.iter().any(|h| h.to_lowercase().contains(needle))
}

fn feed_item_parent_handle(item: &smooblue_atproto::FeedItem) -> Option<String> {
    item.reply_parent_handle()
}

/// Stable key for a notification group — used by Dioxus's `key:`
/// attribute on the render loop. Built from (reason, subject, first
/// item uri) + the loop index as a tiebreaker so two adjacent groups
/// with identical reason+subject (which can happen across pagination
/// boundaries) still get distinct keys.
fn group_key(g: &NotificationGroup, idx: usize) -> String {
    let first_uri = g.items.first().map(|n| n.uri.as_str()).unwrap_or("");
    format!(
        "{idx}:{r}:{s}:{first_uri}",
        r = g.reason,
        s = g.reason_subject.as_deref().unwrap_or(""),
    )
}

/// Which AT-URIs do we need hydrated to give each notification context?
///
/// - like / repost: the user's post they engaged with → `reason_subject`
/// - reply / mention / quote: the *event* post itself (lives at
///   `notif.uri`) — for a quote that's the post that quoted you,
///   which carries your post as a nested record embed. Using
///   `reason_subject` here would surface your own post instead of the
///   quote, which is the bug this avoids.
/// - follow / starterpack-joined: nothing
///
/// Deduped — list_notifications often has many likes of the same post.
fn collect_subject_uris(items: &[Notification]) -> Vec<String> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for n in items {
        let want = match n.reason.as_str() {
            // Engagement-on-something cases: the reason_subject is
            // the post the user wants to see (their own post that
            // got engagement, or — for the -via-repost variants —
            // the post they reposted that someone else then liked
            // or re-reposted).
            "like" | "like-via-repost" | "repost" | "repost-via-repost" | "subscribed-post" => {
                n.reason_subject.clone()
            }
            // Inbound conversation: show the post that quoted /
            // replied / mentioned you (notif.uri), not your own.
            "reply" | "mention" | "quote" => Some(n.uri.clone()),
            _ => None,
        };
        if let Some(uri) = want {
            if seen.insert(uri.clone()) {
                out.push(uri);
            }
        }
    }
    out
}

/// Look up the PostView that gives context to a single notification.
/// Returns `None` for follows / starterpack notifications (no subject)
/// or when hydration didn't find the post (deleted, blocked, etc.).
fn subject_for<'a>(
    n: &Notification,
    subjects: &'a HashMap<String, PostView>,
) -> Option<&'a PostView> {
    let key = match n.reason.as_str() {
        "like" | "like-via-repost" | "repost" | "repost-via-repost" | "subscribed-post" => {
            n.reason_subject.as_deref()?
        }
        "reply" | "mention" | "quote" => &n.uri,
        _ => return None,
    };
    subjects.get(key)
}

#[component]
fn ColumnHeader(
    id: String,
    title: String,
    kind: ColumnKind,
    filter_open: Signal<bool>,
    settings_open: Signal<bool>,
    /// Inbox-only: invoked by the "Mark all as read" header button.
    /// `None` for other kinds — the button just doesn't render.
    mark_all_read: Option<Callback<()>>,
) -> Element {
    let mut cols = use_context::<Signal<Vec<crate::state::ColumnSpec>>>();
    let mut drag_ctx = use_context::<Signal<ColumnDrag>>();
    let id_for_close = id.clone();
    let close = move |_| {
        crate::state::remove_column(&mut cols, &id_for_close);
    };
    let mut filter_open_w = filter_open;
    let toggle_filter = move |_| {
        let now = !*filter_open_w.read();
        filter_open_w.set(now);
    };
    let mut settings_open_w = settings_open;
    let toggle_settings = move |_| {
        let now = !*settings_open_w.read();
        settings_open_w.set(now);
    };

    // Drag-and-drop handlers — header is the drag handle (grip icon),
    // the whole header acts as the drop target. We use a shared
    // ColumnDrag context so visual feedback (dimmed dragged column +
    // highlighted drop target) renders on the right elements.
    let id_drag_start = id.clone();
    let dragstart = move |_evt: DragEvent| {
        drag_ctx.set(ColumnDrag {
            dragging: Some(id_drag_start.clone()),
            target: None,
        });
    };
    let dragend = move |_evt: DragEvent| {
        drag_ctx.set(ColumnDrag::default());
    };
    // dragover MUST preventDefault on every fire or the browser
    // disallows the drop. We also update the target id so the drop
    // zone gets its visual highlight.
    let id_dragover = id.clone();
    let dragover = move |evt: DragEvent| {
        evt.prevent_default();
        let mut state = drag_ctx.write();
        if state.target.as_deref() != Some(id_dragover.as_str()) {
            state.target = Some(id_dragover.clone());
        }
    };
    let dragleave = move |_evt: DragEvent| {
        let mut state = drag_ctx.write();
        state.target = None;
    };
    let id_drop = id.clone();
    let drop = move |evt: DragEvent| {
        evt.prevent_default();
        let snap = drag_ctx.read().clone();
        if let Some(src) = snap.dragging.clone() {
            crate::state::move_column(&mut cols, &src, &id_drop);
        }
        drag_ctx.set(ColumnDrag::default());
    };

    rsx! {
        header { class: "deck-column__header",
            draggable: "true",
            ondragstart: dragstart,
            ondragend: dragend,
            ondragover: dragover,
            ondragleave: dragleave,
            ondrop: drop,
            span { class: "deck-column__drag", title: "Drag to reorder",
                icons::GripVertical { size: icons::Size::Sm }
            }
            span { class: "deck-column__icon",
                match kind {
                    ColumnKind::Notifications => rsx! { icons::Bell { size: icons::Size::Sm } },
                    ColumnKind::Search { .. } => rsx! { icons::Search { size: icons::Size::Sm } },
                    ColumnKind::AuthorFeed { .. } => rsx! { icons::User { size: icons::Size::Sm } },
                    ColumnKind::Feed { .. } => rsx! { icons::Compass { size: icons::Size::Sm } },
                    ColumnKind::List { .. } => rsx! { icons::Users { size: icons::Size::Sm } },
                    ColumnKind::Suggestions => rsx! { icons::Sparkles { size: icons::Size::Sm } },
                    ColumnKind::Messages => rsx! { icons::MessageCircle { size: icons::Size::Sm } },
                    ColumnKind::Inbox => rsx! { icons::Inbox { size: icons::Size::Sm } },
                    ColumnKind::Home => rsx! { icons::Home { size: icons::Size::Sm } },
                }
            }
            span { class: "deck-column__title", "{title}" }
            if let Some(cb) = mark_all_read {
                button { class: "deck-column__action",
                    title: "Mark all as read",
                    onclick: move |_| cb.call(()),
                    icons::CheckCheck { size: icons::Size::Sm }
                }
            }
            button { class: "deck-column__action",
                title: if *filter_open.read() { "Hide filter" } else { "Filter this column" },
                onclick: toggle_filter,
                icons::ListFilter { size: icons::Size::Sm }
            }
            button { class: "deck-column__action",
                title: if *settings_open.read() { "Hide column settings" } else { "Column settings" },
                onclick: toggle_settings,
                icons::Settings2 { size: icons::Size::Sm }
            }
            button { class: "deck-column__action", title: "Close column", onclick: close,
                icons::X { size: icons::Size::Sm }
            }
        }
    }
}

/// Per-column settings panel that slides in under the header (gear).
/// Shows feed filters for post columns, a notification-type selector
/// for Notifications, and a refresh-cadence selector for all. Each
/// control mutates this column's persisted settings immediately.
#[component]
fn ColumnSettingsPanel(
    id: String,
    kind: ColumnKind,
    settings: ColumnSettings,
    cols: Signal<Vec<ColumnSpec>>,
) -> Element {
    let is_feed = matches!(
        kind,
        ColumnKind::Home
            | ColumnKind::Feed { .. }
            | ColumnKind::List { .. }
            | ColumnKind::AuthorFeed { .. }
            | ColumnKind::Search { .. }
    );
    let is_notif = matches!(kind, ColumnKind::Notifications);

    rsx! {
        div { class: "deck-column__settings",
            if is_feed {
                div { class: "deck-column__settings-row",
                    span { class: "deck-column__settings-label", "Show" }
                    button {
                        class: if settings.hide_reposts { "chip chip--on" } else { "chip" },
                        onclick: {
                            let id = id.clone();
                            let mut cols = cols;
                            move |_| crate::state::update_column_settings(&mut cols, &id, |s| s.hide_reposts = !s.hide_reposts)
                        },
                        "Hide reposts"
                    }
                    button {
                        class: if settings.hide_replies { "chip chip--on" } else { "chip" },
                        onclick: {
                            let id = id.clone();
                            let mut cols = cols;
                            move |_| crate::state::update_column_settings(&mut cols, &id, |s| s.hide_replies = !s.hide_replies)
                        },
                        "Hide replies"
                    }
                    button {
                        class: if settings.media_only { "chip chip--on" } else { "chip" },
                        onclick: {
                            let id = id.clone();
                            let mut cols = cols;
                            move |_| crate::state::update_column_settings(&mut cols, &id, |s| {
                                s.media_only = !s.media_only;
                                if s.media_only { s.text_only = false; }
                            })
                        },
                        "Media only"
                    }
                    button {
                        class: if settings.text_only { "chip chip--on" } else { "chip" },
                        onclick: {
                            let id = id.clone();
                            let mut cols = cols;
                            move |_| crate::state::update_column_settings(&mut cols, &id, |s| {
                                s.text_only = !s.text_only;
                                if s.text_only { s.media_only = false; }
                            })
                        },
                        "Text only"
                    }
                }
            }
            if is_notif {
                div { class: "deck-column__settings-row",
                    span { class: "deck-column__settings-label", "Show" }
                    for (label, val) in [
                        ("All", NotifFilter::All),
                        ("Mentions", NotifFilter::Mentions),
                        ("Reactions", NotifFilter::Reactions),
                    ] {
                        button {
                            key: "{label}",
                            class: if settings.notif_filter == val { "chip chip--on" } else { "chip" },
                            onclick: {
                                let id = id.clone();
                                let mut cols = cols;
                                move |_| crate::state::update_column_settings(&mut cols, &id, |s| s.notif_filter = val)
                            },
                            "{label}"
                        }
                    }
                }
            }
            div { class: "deck-column__settings-row",
                span { class: "deck-column__settings-label", "Refresh" }
                for (label, val) in [
                    ("Auto", crate::state::RefreshInterval::Default),
                    ("15s", crate::state::RefreshInterval::S15),
                    ("30s", crate::state::RefreshInterval::S30),
                    ("60s", crate::state::RefreshInterval::S60),
                    ("Off", crate::state::RefreshInterval::Off),
                ] {
                    button {
                        key: "{label}",
                        class: if settings.refresh == val { "chip chip--on" } else { "chip" },
                        onclick: {
                            let id = id.clone();
                            let mut cols = cols;
                            move |_| crate::state::update_column_settings(&mut cols, &id, |s| s.refresh = val)
                        },
                        "{label}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smooblue_atproto::feed::{PostAuthor, PostRecord, PostView};

    fn mk(uri: &str) -> FeedItem {
        FeedItem {
            post: PostView {
                uri: uri.into(),
                cid: format!("cid:{uri}"),
                author: PostAuthor {
                    did: "did:plc:a".into(),
                    handle: "a.test".into(),
                    display_name: None,
                    avatar: None,
                },
                record: PostRecord {
                    text: String::new(),
                    created_at: None,
                    facets: None,
                },
                embed: None,
                indexed_at: None,
                reply_count: 0,
                repost_count: 0,
                like_count: 0,
                quote_count: 0,
                viewer: None,
                labels: Vec::new(),
            },
            reply: None,
            reason: None,
        }
    }

    fn mk_repost(uri: &str) -> FeedItem {
        let mut it = mk(uri);
        it.reason = Some(serde_json::json!({
            "$type": "app.bsky.feed.defs#reasonRepost",
            "by": { "did": "did:plc:reposter", "handle": "rp.test" }
        }));
        it
    }

    fn mk_reply(uri: &str) -> FeedItem {
        let mut it = mk(uri);
        it.reply = Some(serde_json::json!({ "parent": { "uri": "at://parent" } }));
        it
    }

    fn mk_with_image(uri: &str) -> FeedItem {
        use smooblue_atproto::{Embed, EmbedImage, EmbedKind};
        let mut it = mk(uri);
        it.post.embed = Some(Embed::Known(EmbedKind::Images {
            images: vec![EmbedImage {
                thumb: "t".into(),
                fullsize: "f".into(),
                alt: String::new(),
                aspect_ratio: None,
            }],
        }));
        it
    }

    #[test]
    fn feed_settings_hide_reposts_and_replies() {
        let s = ColumnSettings {
            hide_reposts: true,
            ..Default::default()
        };
        assert!(passes_feed_settings(&mk("at://x/1"), &s));
        assert!(!passes_feed_settings(&mk_repost("at://x/2"), &s));

        let s2 = ColumnSettings {
            hide_replies: true,
            ..Default::default()
        };
        assert!(passes_feed_settings(&mk("at://x/3"), &s2));
        assert!(!passes_feed_settings(&mk_reply("at://x/4"), &s2));
    }

    #[test]
    fn feed_settings_media_and_text_only() {
        let img = mk_with_image("at://x/5");
        let txt = mk("at://x/6");

        let media = ColumnSettings {
            media_only: true,
            ..Default::default()
        };
        assert!(passes_feed_settings(&img, &media));
        assert!(!passes_feed_settings(&txt, &media));

        let text = ColumnSettings {
            text_only: true,
            ..Default::default()
        };
        assert!(!passes_feed_settings(&img, &text));
        assert!(passes_feed_settings(&txt, &text));
    }

    #[test]
    fn notif_filter_buckets() {
        assert!(passes_notif_filter("like", NotifFilter::All));
        // Mentions bucket = conversational.
        assert!(passes_notif_filter("reply", NotifFilter::Mentions));
        assert!(passes_notif_filter("quote", NotifFilter::Mentions));
        assert!(!passes_notif_filter("like", NotifFilter::Mentions));
        // Reactions bucket = likes / reposts / follows.
        assert!(passes_notif_filter("like", NotifFilter::Reactions));
        assert!(passes_notif_filter("follow", NotifFilter::Reactions));
        assert!(!passes_notif_filter("reply", NotifFilter::Reactions));
    }

    #[test]
    fn refresh_interval_resolves() {
        use crate::state::RefreshInterval;
        use std::time::Duration;
        let fallback = Duration::from_secs(25);
        assert_eq!(RefreshInterval::Default.duration(fallback), Some(fallback));
        assert_eq!(RefreshInterval::Off.duration(fallback), None);
        assert_eq!(
            RefreshInterval::S15.duration(fallback),
            Some(Duration::from_secs(15))
        );
    }

    fn mk_post(uri: &str) -> PostView {
        mk(uri).post
    }

    #[test]
    fn merge_top_prepends_new_and_keeps_existing_tail() {
        let existing = vec![mk("at://x/a"), mk("at://x/b"), mk("at://x/c")];
        let fresh = vec![mk("at://x/new1"), mk("at://x/new2"), mk("at://x/a")];
        let merged = merge_top_page(existing, fresh, 100);
        // new1 + new2 prepended; a (dup) skipped; existing tail kept.
        let uris: Vec<&str> = merged.iter().map(|i| i.post.uri.as_str()).collect();
        assert_eq!(
            uris,
            vec![
                "at://x/new1",
                "at://x/new2",
                "at://x/a",
                "at://x/b",
                "at://x/c"
            ]
        );
    }

    #[test]
    fn merge_top_respects_cap_from_the_head() {
        // Big merge: 5 fresh + 10 existing, cap at 8 → keep the newest 8.
        let existing: Vec<FeedItem> = (0..10).map(|i| mk(&format!("at://x/old{i}"))).collect();
        let fresh: Vec<FeedItem> = (0..5).map(|i| mk(&format!("at://x/new{i}"))).collect();
        let merged = merge_top_page(existing, fresh, 8);
        assert_eq!(merged.len(), 8);
        // First 5 = the fresh items (newest); next 3 = the start of existing.
        assert_eq!(merged[0].post.uri, "at://x/new0");
        assert_eq!(merged[4].post.uri, "at://x/new4");
        assert_eq!(merged[5].post.uri, "at://x/old0");
        assert_eq!(merged[7].post.uri, "at://x/old2");
    }

    #[test]
    fn merge_top_empty_fresh_keeps_existing() {
        let existing = vec![mk("at://x/a"), mk("at://x/b")];
        let merged = merge_top_page(existing.clone(), vec![], 100);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].post.uri, "at://x/a");
    }

    #[test]
    fn merge_top_empty_existing_takes_full_fresh() {
        let fresh = vec![mk("at://x/n1"), mk("at://x/n2")];
        let merged = merge_top_page(vec![], fresh, 100);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn scroll_anchor_keeps_top_item_after_prepend() {
        // The scroll-anchor fix relies on this invariant: prepending N
        // rows and bumping scroll_top by N*row_h advances the first-in-
        // viewport index by exactly N (so the same item stays at the top)
        // and grows the top spacer by exactly N*row_h (so the visible
        // content doesn't shift).
        let row_h = 240.0;
        let client_h = 800.0;
        let total = 100;
        // User scrolled so item index 10 sits at the top of the viewport.
        let scroll_top = 10.0 * row_h;
        let (_f0, _l0, top0, _b0) = virtual_range(total, scroll_top, client_h, row_h);

        // Three genuinely-new rows arrive at the head; compensate by N*row_h.
        let n = 3usize;
        let scroll_top2 = scroll_top + n as f64 * row_h;
        let (_f1, _l1, top1, _b1) = virtual_range(total + n, scroll_top2, client_h, row_h);

        let first_in_vp_before = (scroll_top / row_h) as usize;
        let first_in_vp_after = (scroll_top2 / row_h) as usize;
        // What was index 10 is now index 13 — and that's what's at the top
        // after compensation, so the user's read position is preserved.
        assert_eq!(first_in_vp_after, first_in_vp_before + n);
        // The top spacer grew by exactly the prepended height → no shift.
        assert_eq!(top1 - top0, n as f64 * row_h);
    }

    #[test]
    fn humanize_count_formats_compactly() {
        assert_eq!(humanize_count(0), "0");
        assert_eq!(humanize_count(850), "850");
        assert_eq!(humanize_count(1_000), "1k");
        assert_eq!(humanize_count(12_400), "12.4k");
        assert_eq!(humanize_count(12_000), "12k");
        assert_eq!(humanize_count(1_200_000), "1.2M");
        assert_eq!(humanize_count(2_000_000), "2M");
    }

    #[test]
    fn scroll_anchor_noop_at_top() {
        // At the very top we deliberately skip the scroll bump so fresh
        // posts appear in view: the newest item stays rendered first and
        // the top spacer stays at zero before and after a prepend.
        let row_h = 240.0;
        let client_h = 800.0;
        let (_f, _l, top_before, _b) = virtual_range(100, 0.0, client_h, row_h);
        let (_f2, _l2, top_after, _b2) = virtual_range(103, 0.0, client_h, row_h);
        assert_eq!(top_before, 0.0);
        assert_eq!(top_after, 0.0);
    }

    #[test]
    fn append_bottom_appends_new_items_only() {
        let existing = vec![mk("at://x/a"), mk("at://x/b")];
        let more = vec![mk("at://x/c"), mk("at://x/b"), mk("at://x/d")];
        let out = append_bottom_page(existing, more, 100);
        let uris: Vec<&str> = out.iter().map(|i| i.post.uri.as_str()).collect();
        assert_eq!(uris, vec!["at://x/a", "at://x/b", "at://x/c", "at://x/d"]);
    }

    #[test]
    fn append_bottom_refuses_to_evict_past_cap() {
        // Existing already at cap → no items should be appended even
        // though `more` has 3 fresh ones. This is the load-bearing
        // memory guard — "refuse rather than evict".
        let existing: Vec<FeedItem> = (0..5).map(|i| mk(&format!("at://x/{i}"))).collect();
        let more = vec![mk("at://x/m1"), mk("at://x/m2"), mk("at://x/m3")];
        let out = append_bottom_page(existing.clone(), more, 5);
        assert_eq!(out.len(), 5);
        // None of the m* items made it in.
        for item in &out {
            assert!(!item.post.uri.starts_with("at://x/m"));
        }
    }

    #[test]
    fn append_bottom_takes_only_what_fits() {
        // Existing has 3 slots free, more has 5 candidates → take 3.
        let existing: Vec<FeedItem> = (0..2).map(|i| mk(&format!("at://x/{i}"))).collect();
        let more: Vec<FeedItem> = (0..5).map(|i| mk(&format!("at://x/m{i}"))).collect();
        let out = append_bottom_page(existing, more, 5);
        assert_eq!(out.len(), 5);
        assert_eq!(out[2].post.uri, "at://x/m0");
        assert_eq!(out[4].post.uri, "at://x/m2");
    }

    #[test]
    fn is_paginated_classifies_kinds_correctly() {
        assert!(is_paginated(&ColumnKind::Home));
        assert!(is_paginated(&ColumnKind::Search { query: "x".into() }));
        assert!(is_paginated(&ColumnKind::Feed {
            uri: "at://x".into()
        }));
        assert!(is_paginated(&ColumnKind::AuthorFeed { actor: "a".into() }));
        assert!(is_paginated(&ColumnKind::List {
            uri: "at://x".into()
        }));
        // Notifications now paginates (infinite-scroll wired 2026-06-03).
        assert!(is_paginated(&ColumnKind::Notifications));
        // Suggestions stays single-page (server returns a single
        // personalized actor set — there's no "older suggestions").
        assert!(!is_paginated(&ColumnKind::Suggestions));
    }

    fn mk_notif(reason: &str, subject: Option<&str>, uri: &str) -> Notification {
        Notification {
            uri: uri.into(),
            cid: format!("cid:{uri}"),
            author: PostAuthor {
                did: format!("did:plc:{uri}"),
                handle: format!("{uri}.test"),
                display_name: None,
                avatar: None,
            },
            reason: reason.into(),
            reason_subject: subject.map(String::from),
            indexed_at: None,
            is_read: false,
            record: None,
        }
    }

    fn mk_group(reason: &str, subject: Option<&str>, uris: &[&str]) -> NotificationGroup {
        NotificationGroup {
            reason: reason.into(),
            reason_subject: subject.map(String::from),
            items: uris.iter().map(|u| mk_notif(reason, subject, u)).collect(),
            latest_at: None,
        }
    }

    #[test]
    fn merge_top_notif_prepends_new_groups_keeps_existing_tail() {
        let existing = vec![
            mk_group("like", Some("at://post/a"), &["like1", "like2"]),
            mk_group("reply", None, &["reply1"]),
        ];
        let fresh = vec![
            mk_group("follow", None, &["fnew1"]),
            mk_group("like", Some("at://post/a"), &["like3"]), // merges into existing
        ];
        let merged = merge_top_notif_groups(existing, fresh, 100);
        // Order: new follow group first, then the (still-positioned)
        // existing like group with merged items, then the reply group.
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].reason, "follow");
        assert_eq!(merged[1].reason, "like");
        assert_eq!(merged[1].items.len(), 3); // like3 prepended + like1, like2
        assert_eq!(merged[1].items[0].uri, "like3");
        assert_eq!(merged[2].reason, "reply");
    }

    #[test]
    fn merge_top_notif_dedupes_items_by_uri_cid() {
        let existing = vec![mk_group("like", Some("at://post/a"), &["like1"])];
        let fresh = vec![mk_group("like", Some("at://post/a"), &["like1", "like2"])];
        let merged = merge_top_notif_groups(existing, fresh, 100);
        assert_eq!(merged.len(), 1);
        // like1 from fresh deduped; only like2 added.
        let uris: Vec<&str> = merged[0].items.iter().map(|n| n.uri.as_str()).collect();
        assert_eq!(uris, vec!["like2", "like1"]);
    }

    #[test]
    fn append_bottom_notif_merges_same_key_groups_in_place() {
        let existing = vec![
            mk_group("reply", None, &["reply_top"]),
            mk_group("like", Some("at://post/a"), &["like1"]),
        ];
        let more = vec![
            mk_group("like", Some("at://post/a"), &["like_older"]),
            mk_group("repost", Some("at://post/b"), &["repost_older"]),
        ];
        let out = append_bottom_notif_groups(existing, more, 100);
        // Same-key group stays in place; new key appended at bottom.
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].reason, "reply");
        assert_eq!(out[1].reason, "like");
        assert_eq!(out[1].items.len(), 2);
        assert_eq!(out[2].reason, "repost");
    }

    #[test]
    fn append_bottom_notif_refuses_to_evict_past_cap() {
        let existing: Vec<NotificationGroup> = (0..3)
            .map(|i| mk_group("follow", None, &[&format!("f{i}")]))
            .collect();
        // 3 unique-key groups + cap = 3 → no new groups should append.
        let existing: Vec<NotificationGroup> = existing
            .into_iter()
            .enumerate()
            .map(|(i, mut g)| {
                g.reason_subject = Some(format!("subj{i}"));
                g
            })
            .collect();
        let more = vec![
            mk_group("like", Some("at://post/x"), &["x1"]),
            mk_group("like", Some("at://post/y"), &["y1"]),
        ];
        let out = append_bottom_notif_groups(existing, more, 3);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|g| g.reason == "follow"));
    }

    #[test]
    fn collect_subject_uris_quote_hydrates_quoting_post_not_your_own() {
        // The bug: a quote notification used to hydrate `reason_subject`
        // (your own quoted post) instead of `uri` (the post that quoted
        // you, which carries your post as a nested record embed). The
        // user then never saw the quote itself. Quote must behave like
        // reply/mention and surface the inbound post at `uri`.
        let items = vec![
            mk_notif("quote", Some("at://you/original"), "at://them/quoting"),
            mk_notif("reply", Some("at://you/thread"), "at://them/replyrec"),
            mk_notif("mention", None, "at://them/mentionrec"),
            mk_notif("like", Some("at://you/liked"), "at://them/likerec"),
            mk_notif("repost", Some("at://you/reposted"), "at://them/repostrec"),
            mk_notif("follow", None, "at://them/followrec"),
        ];
        let uris = collect_subject_uris(&items);

        // Inbound conversation → the event post (notif.uri).
        assert!(uris.contains(&"at://them/quoting".to_string()));
        assert!(uris.contains(&"at://them/replyrec".to_string()));
        assert!(uris.contains(&"at://them/mentionrec".to_string()));
        // ...never your own quoted/replied-to post.
        assert!(!uris.contains(&"at://you/original".to_string()));

        // Engagement-on-your-post → reason_subject (your post).
        assert!(uris.contains(&"at://you/liked".to_string()));
        assert!(uris.contains(&"at://you/reposted".to_string()));

        // Follow has no subject to hydrate.
        assert!(!uris.contains(&"at://them/followrec".to_string()));
    }

    #[test]
    fn subject_for_quote_keys_on_notif_uri() {
        // subject_for mirrors collect_subject_uris' arm selection — the
        // map is keyed by the URI we chose to hydrate. For a quote that
        // is notif.uri, so a subjects map keyed by reason_subject must
        // miss while one keyed by uri hits.
        let n = mk_notif("quote", Some("at://you/original"), "at://them/quoting");

        let mut wrong = HashMap::new();
        wrong.insert(
            "at://you/original".to_string(),
            mk_post("at://you/original"),
        );
        assert!(subject_for(&n, &wrong).is_none());

        let mut right = HashMap::new();
        right.insert(
            "at://them/quoting".to_string(),
            mk_post("at://them/quoting"),
        );
        assert_eq!(
            subject_for(&n, &right).map(|p| p.uri.as_str()),
            Some("at://them/quoting")
        );
    }

    #[test]
    fn virtual_range_empty_list_returns_empty_window() {
        let (first, last, top, bot) = virtual_range(0, 0.0, 800.0, 240.0);
        assert_eq!((first, last), (0, 0));
        assert_eq!((top, bot), (0.0, 0.0));
    }

    #[test]
    fn virtual_range_cold_viewport_renders_a_default_window() {
        // viewport=0 (not measured yet) should fall back to a single
        // viewport so the first paint has rows visible without waiting
        // for the eval round-trip.
        let (first, last, _top, _bot) = virtual_range(500, 0.0, 0.0, 240.0);
        assert_eq!(first, 0);
        // 800px fallback / 240px row ≈ 4 in viewport, no buffer above
        // (we're at the top), 2vp = 8 items of buffer below → ~12 total.
        assert!((4..=20).contains(&last), "got last={last}");
    }

    #[test]
    fn virtual_range_at_top_of_long_list_renders_buffer_below() {
        // scroll_top=0, 800px viewport, 240px rows, 500 items.
        // items_per_vp = ceil(800/240) = 4
        // first_in_vp = 0
        // buffer = 2*4 = 8
        // first = 0, last = 0 + 4 + 8 = 12
        let (first, last, top, bot) = virtual_range(500, 0.0, 800.0, 240.0);
        assert_eq!(first, 0);
        assert_eq!(last, 12);
        assert_eq!(top, 0.0);
        assert_eq!(bot, (500 - 12) as f64 * 240.0);
    }

    #[test]
    fn virtual_range_in_middle_keeps_buffer_on_both_sides() {
        // Scroll to halfway through 500-item list: scroll_top = 250*240 = 60000.
        let (first, last, top, _bot) = virtual_range(500, 60000.0, 800.0, 240.0);
        // first_in_vp = 250; buffer below + viewport = 4+8 = 12;
        // buffer above = 8 → first = 242, last = 262.
        assert_eq!(first, 242);
        assert_eq!(last, 262);
        assert_eq!(top, 242.0 * 240.0);
    }

    #[test]
    fn virtual_range_clamps_window_at_end_of_list() {
        // Scroll to the bottom: scroll_top should clamp `last` to `total`.
        let (first, last, _top, bot) = virtual_range(20, 4000.0, 800.0, 240.0);
        assert!(last <= 20);
        assert!(first <= last);
        // No items past the end, so bottom spacer is zero.
        assert_eq!(bot, (20 - last) as f64 * 240.0);
    }

    #[test]
    fn measured_range_empty_is_empty() {
        assert_eq!(measured_virtual_range(&[], 0.0, 800.0), (0, 0, 0.0, 0.0));
    }

    #[test]
    fn measured_range_spacers_account_for_all_offscreen_height() {
        // Mixed heights; the two spacers plus the rendered slice must
        // sum to the full content height regardless of where we are.
        let heights = vec![
            120.0, 500.0, 90.0, 300.0, 450.0, 80.0, 220.0, 600.0, 130.0, 400.0,
        ];
        let total_h: f64 = heights.iter().sum();
        for &st in &[0.0, 350.0, 900.0, 1500.0, 5000.0] {
            let (first, last, top, bot) = measured_virtual_range(&heights, st, 400.0);
            assert!(first <= last && last <= heights.len());
            let rendered: f64 = heights[first..last].iter().sum();
            assert!(
                (top + rendered + bot - total_h).abs() < 1e-6,
                "spacers+slice must equal total at scroll_top={st}"
            );
            // top spacer is exactly the height above `first`.
            assert!((top - heights[..first].iter().sum::<f64>()).abs() < 1e-6);
        }
    }

    #[test]
    fn measured_range_first_tracks_real_heights_not_an_average() {
        // Row 0 is a tall 1000px video; everything else is short. A
        // fixed-height model would mis-place the window, but the
        // measured walk must keep row 0 in view until we scroll past
        // its real height.
        let mut heights = vec![1000.0];
        heights.extend(std::iter::repeat_n(100.0, 20));
        // Scrolled 200px in — still inside row 0, so row 0 stays in the
        // window (with buffer, first is 0).
        let (first, _last, top, _bot) = measured_virtual_range(&heights, 200.0, 400.0);
        assert_eq!(first, 0);
        assert_eq!(top, 0.0);
        // Scrolled 2000px — well past row 0 plus the 600px (1.5×400)
        // buffer, so the tall first row falls out of the window and
        // lands in the top spacer.
        let (first2, _l2, top2, _b2) = measured_virtual_range(&heights, 2000.0, 400.0);
        assert!(first2 >= 1, "expected to scroll past the tall first row");
        assert!(
            top2 >= 1000.0 - 1.0,
            "top spacer should include the tall row"
        );
    }

    #[test]
    fn measured_range_clamps_past_end() {
        let heights = vec![200.0; 10];
        let (first, last, _top, bot) = measured_virtual_range(&heights, 100_000.0, 800.0);
        assert!(last <= 10 && first <= last);
        assert_eq!(bot, 0.0); // nothing below the rendered window
    }

    #[test]
    fn measured_range_matches_uniform_heights_roughly() {
        // With uniform heights the measured walk should land on the
        // same neighbourhood as the legacy fixed-height computation.
        let heights = vec![240.0; 50];
        let (mf, ml, _, _) = measured_virtual_range(&heights, 2400.0, 800.0);
        let (vf, vl, _, _) = virtual_range(50, 2400.0, 800.0, 240.0);
        // Same ballpark window (buffers differ slightly: px vs items).
        assert!((mf as i64 - vf as i64).abs() <= 3);
        assert!((ml as i64 - vl as i64).abs() <= 3);
    }

    #[test]
    fn memory_budget_per_column_is_reasonable() {
        // Sanity: 2000 representative FeedItems shouldn't push past
        // a few MB of Vec overhead. The real per-item heap is
        // dominated by String contents that this measurement won't
        // capture, but the Vec's *fixed* overhead alone is one of
        // the things that could quietly balloon if FeedItem grows.
        let items: Vec<FeedItem> = (0..MAX_POSTS_PER_COLUMN)
            .map(|i| mk(&format!("at://x/{i}")))
            .collect();
        let stack_bytes = std::mem::size_of_val(items.as_slice());
        // A FeedItem at 1.0 is ~712 bytes of struct overhead (PostView
        // is the bulk — String headers + Option<Vec> + the Embed enum's
        // worst-case variant). 2000 × 712 ≈ 1.4 MB. The cap below has
        // ~40% slack so small additions don't break the test, but
        // anything that takes us past 2 MB stack-only means a real
        // refactor — break the test, force the audit, decide whether
        // MAX_POSTS_PER_COLUMN should drop.
        assert!(
            stack_bytes < 2_000_000,
            "FeedItem stack footprint grew unexpectedly: {} bytes for {} items \
             (~{} bytes / item) — review MAX_POSTS_PER_COLUMN budget",
            stack_bytes,
            MAX_POSTS_PER_COLUMN,
            stack_bytes / MAX_POSTS_PER_COLUMN,
        );
    }

    // ── feed_item_matches (per-column fuzzy filter) ────────────────

    fn mk_with(text: &str, handle: &str, display: Option<&str>) -> FeedItem {
        let mut item = mk("at://x/one");
        item.post.record.text = text.into();
        item.post.author.handle = handle.into();
        item.post.author.display_name = display.map(String::from);
        item
    }

    #[test]
    fn filter_empty_needle_matches_everything() {
        let item = mk_with("hello", "alice.bsky", None);
        assert!(feed_item_matches(&item, ""));
    }

    #[test]
    fn filter_matches_post_text_case_insensitive() {
        // Matcher contract: needle is already lowercased (callers
        // call .to_lowercase() once up front — the column does this
        // exactly once per render, not per item).
        let item = mk_with("I love Rust today", "anon.bsky", None);
        assert!(feed_item_matches(&item, "rust"));
        assert!(feed_item_matches(&item, "love"));
        assert!(!feed_item_matches(&item, "javascript"));
    }

    #[test]
    fn filter_matches_handle_and_display_name() {
        let item = mk_with("ok", "alice.bsky.social", Some("Alice McEntire"));
        assert!(feed_item_matches(&item, "alice"));
        assert!(feed_item_matches(&item, "bsky.social"));
        assert!(feed_item_matches(&item, "mcentire"));
    }

    #[test]
    fn filter_returns_false_for_no_match() {
        let item = mk_with("just a post", "x.test", Some("X"));
        assert!(!feed_item_matches(&item, "rust"));
        assert!(!feed_item_matches(&item, "🦀"));
    }
}
