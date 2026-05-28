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
use crate::state::{ColumnDrag, ColumnKind, ColumnSpec, FocusColumn};
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

/// How many items we ask for per page. Small enough that the first
/// page paints fast, large enough that scroll-to-bottom doesn't fire
/// a fetch_more on every flick.
const PAGE_SIZE: u32 = 30;

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
}

impl ColumnData {
    fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Posts(p) => p.is_empty(),
            Self::Notifications { groups, .. } => groups.is_empty(),
            Self::Suggestions(actors) => actors.is_empty(),
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
    }
}

#[component]
pub fn Column(spec: ColumnSpec) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let drag_ctx = use_context::<Signal<ColumnDrag>>();
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
    use_future(move || {
        let kind = kind_for_poll.clone();
        let session_sig = session;
        async move {
            let interval = poll_interval(&kind);
            let mut first_fetch = true;
            // Persistent across polls — used by the Notifications fetch
            // path to avoid re-hydrating subject posts that are already
            // known. Bounded at 500 entries so a long-running session
            // doesn't grow this unboundedly.
            let mut subjects_cache: HashMap<String, PostView> = HashMap::new();
            loop {
                match fetch_page(&kind, session_sig, None, &mut subjects_cache).await {
                    Ok(fresh) => {
                        error.set(None);
                        loading.set(false);
                        // Merge the fresh page into whatever we already
                        // have. First-fetch: just install. Subsequent
                        // polls: prepend new items, preserve tail.
                        let merged = match (data.peek().clone(), fresh.data) {
                            (_, ColumnData::Empty) => ColumnData::Empty,
                            (ColumnData::Posts(existing), ColumnData::Posts(new_page)) => {
                                ColumnData::Posts(merge_top_page(
                                    existing,
                                    new_page,
                                    MAX_POSTS_PER_COLUMN,
                                ))
                            }
                            // Notifications + Suggestions don't paginate
                            // this way — top-poll replaces wholesale.
                            (_, other) => other,
                        };
                        data.set(merged);
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

    // "Load more" click handler. Skips entirely for non-paginated
    // column kinds (Notifications, Suggestions) and when:
    //   - a fetch is already in flight
    //   - the server told us there's no more (at_end)
    //   - we'd push the column over MAX_POSTS_PER_COLUMN
    //
    // Auto-trigger on scroll-near-bottom is a follow-up — Dioxus
    // 0.6's ScrollData doesn't expose scroll position, so we'd need
    // a JS-eval'd IntersectionObserver. Button works today.
    let kind_for_more = spec_kind.clone();
    let load_more = move |_| {
        if !is_paginated(&kind_for_more) {
            return;
        }
        if *loading_more.read() || *at_end.read() {
            return;
        }
        // Cap-guard: refuse rather than evict.
        if let ColumnData::Posts(items) = &*data.peek() {
            if items.len() >= MAX_POSTS_PER_COLUMN {
                return;
            }
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
                    if let (ColumnData::Posts(existing), ColumnData::Posts(new_page)) =
                        (existing_snap, more.data)
                    {
                        data.set(ColumnData::Posts(append_bottom_page(
                            existing,
                            new_page,
                            MAX_POSTS_PER_COLUMN,
                        )));
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
    };
    // Whether to render the "Load more" button (only on paginated
    // kinds, only when not at-end, only when not capped).
    let kind_for_button_check = spec_kind.clone();
    let show_load_more = is_paginated(&kind_for_button_check)
        && !*at_end.read()
        && match &*data.read() {
            ColumnData::Posts(items) => !items.is_empty() && items.len() < MAX_POSTS_PER_COLUMN,
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
            div { class: "deck-column__body",
                match (&*data.read(), &*error.read(), *loading.read()) {
                    (_, _, true) if data.read().is_empty() => rsx! { div { class: "deck-column__loading", "Loading…" } },
                    (data, _, _) if data.is_empty() => rsx! { div { class: "deck-column__empty", "Nothing here yet." } },
                    (ColumnData::Posts(items), _, _) => {
                        let filtered: Vec<&FeedItem> = items
                            .iter()
                            .filter(|it| !has_filter || feed_item_matches(it, &filter_lower))
                            .collect();
                        if filtered.is_empty() {
                            rsx! {
                                div { class: "deck-column__empty",
                                    "No posts match \"{applied_snap}\""
                                }
                            }
                        } else {
                            rsx! {
                                for item in filtered.into_iter() {
                                    // Same post URI can appear twice in a
                                    // feed (e.g. two reposters surfaced it).
                                    // Disambiguate the key with the reposter
                                    // DID when present so Dioxus's keyed-diff
                                    // assertion holds.
                                    PostCard {
                                        key: "{feed_item_key(item)}",
                                        post: item.post.clone(),
                                        reposter: feed_item_reposter(item),
                                        reply_parent_handle: feed_item_parent_handle(item),
                                    }
                                }
                            }
                        }
                    }
                    (ColumnData::Notifications { groups, subjects }, _, _) => rsx! {
                        for (i, g) in groups.iter().enumerate() {
                            NotificationCard {
                                key: "{group_key(g, i)}",
                                group: g.clone(),
                                subject: g.items.first().and_then(|n| subject_for(n, subjects)).cloned(),
                            }
                        }
                    },
                    (ColumnData::Suggestions(actors), _, _) => rsx! {
                        for a in actors.iter() {
                            crate::components::suggestion::SuggestionRow { key: "{a.did}", actor: a.clone() }
                        }
                    },
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
                // memory ceiling.
                if matches!(&*data.read(), ColumnData::Posts(items) if !items.is_empty()) {
                    if *loading_more.read() {
                        div { class: "deck-column__more", "Loading more…" }
                    } else if matches!(&*data.read(), ColumnData::Posts(items) if items.len() >= MAX_POSTS_PER_COLUMN) {
                        div { class: "deck-column__more deck-column__more--cap",
                            "Scrollback cap reached ({MAX_POSTS_PER_COLUMN} posts). Refresh to reset."
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

/// True when the column supports cursor-based fetch_more on scroll.
/// Notifications and Suggestions have their own pagination semantics
/// (notifications are time-bucketed and small; suggestions are a
/// single page of personalized actors).
fn is_paginated(kind: &ColumnKind) -> bool {
    matches!(
        kind,
        ColumnKind::Home
            | ColumnKind::AuthorFeed { .. }
            | ColumnKind::Search { .. }
            | ColumnKind::Feed { .. }
            | ColumnKind::List { .. }
    )
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
            // Notifications don't paginate via fetch_more — top-poll
            // only. We don't expose a cursor.
            // 30 per fetch is the sweet spot: enough to give the user
            // a meaningful window, small enough that the cascade of
            // get_posts hydration + grouping + per-card clones stays
            // snappy. 50 was visibly laggy on busy accounts.
            let items = client
                .list_notifications(cur, 30)
                .await
                .map(|r| r.notifications)
                .map_err(|e| e.to_string())?;
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
                cursor: None,
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
/// - like / repost / quote: the user's post they engaged with → `reason_subject`
/// - reply: the reply post itself (lives at `notif.uri`)
/// - mention: the post that mentioned us (also `notif.uri`)
/// - follow / starterpack-joined: nothing
///
/// Deduped — list_notifications often has many likes of the same post.
fn collect_subject_uris(items: &[Notification]) -> Vec<String> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for n in items {
        let want = match n.reason.as_str() {
            "like" | "repost" | "quote" => n.reason_subject.clone(),
            "reply" | "mention" => Some(n.uri.clone()),
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
        "like" | "repost" | "quote" => n.reason_subject.as_deref()?,
        "reply" | "mention" => &n.uri,
        _ => return None,
    };
    subjects.get(key)
}

#[component]
fn ColumnHeader(id: String, title: String, kind: ColumnKind, filter_open: Signal<bool>) -> Element {
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
                    ColumnKind::Home => rsx! { icons::Home { size: icons::Size::Sm } },
                }
            }
            span { class: "deck-column__title", "{title}" }
            button { class: "deck-column__action",
                title: if *filter_open.read() { "Hide filter" } else { "Filter this column" },
                onclick: toggle_filter,
                icons::ListFilter { size: icons::Size::Sm }
            }
            button { class: "deck-column__action", title: "Close column", onclick: close,
                icons::X { size: icons::Size::Sm }
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
        // Notifications + Suggestions deliberately excluded — they
        // have their own pagination semantics.
        assert!(!is_paginated(&ColumnKind::Notifications));
        assert!(!is_paginated(&ColumnKind::Suggestions));
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
