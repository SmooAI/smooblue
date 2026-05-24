//! A single deck column. Owns its own fetch state.
//!
//! A column's body is either a feed of posts (Home, AuthorFeed, Discover,
//! Search, custom feeds) or a feed of notifications (Notifications). Those
//! are different shapes, so [`ColumnData`] tags which view to render.
//!
//! Polling model (the "deck.blue feel"):
//! - Each column kind has its own cadence — see [`poll_interval`].
//! - The first fetch populates the column.
//! - Subsequent fetches go into a "pending" buffer. If the user is at the
//!   top of the column we slide them in automatically; otherwise we show
//!   a "N new posts" banner and the user opts in.
//! - No jetstream / firehose — pure XRPC polling against the AppView via
//!   the user's PDS, mirroring what deck.blue does.

use crate::auth_refresh::fresh_client;
use crate::components::notification_card::NotificationCard;
use crate::components::post::PostCard;
use crate::icons;
use crate::state::{ColumnDrag, ColumnKind, ColumnSpec};
use dioxus::prelude::*;
use smooblue_atproto::{group_notifications, FeedItem, Notification, NotificationGroup, PostView};
use smooblue_oauth::Session;
use std::collections::HashMap;
use std::time::Duration;

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
}

impl ColumnData {
    fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Posts(p) => p.is_empty(),
            Self::Notifications { groups, .. } => groups.is_empty(),
        }
    }
}

/// How often each column refetches. Picked to match deck.blue's feel
/// without hammering the AppView.
fn poll_interval(kind: &ColumnKind) -> Duration {
    match kind {
        ColumnKind::Home => Duration::from_secs(15),
        ColumnKind::Notifications => Duration::from_secs(20),
        ColumnKind::Search { .. } => Duration::from_secs(30),
        ColumnKind::Feed { .. } => Duration::from_secs(25),
        ColumnKind::AuthorFeed { .. } => Duration::from_secs(45),
    }
}

#[component]
pub fn Column(spec: ColumnSpec) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let drag_ctx = use_context::<Signal<ColumnDrag>>();
    let spec_kind = spec.kind.clone();
    let spec_id = spec.id.clone();

    // Current visible data. Fresh poll cycles overwrite directly — no
    // banner / opt-in. We auto-load.
    let mut data = use_signal(ColumnData::default);
    let mut error = use_signal::<Option<String>>(|| None);
    let mut loading = use_signal(|| true);

    // The polling loop. Re-fires when the session or kind changes.
    let kind_for_poll = spec_kind.clone();
    use_future(move || {
        let kind = kind_for_poll.clone();
        let session_sig = session;
        async move {
            let interval = poll_interval(&kind);
            loop {
                match fetch_once(&kind, session_sig).await {
                    Ok(fresh) => {
                        error.set(None);
                        loading.set(false);
                        data.set(fresh);
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

    // Visual state derived from the shared drag context — used to dim
    // the column being dragged and highlight the drop target.
    let drag_snap = drag_ctx.read();
    let is_dragging = drag_snap.dragging.as_deref() == Some(spec_id.as_str());
    let is_target = drag_snap.target.as_deref() == Some(spec_id.as_str())
        && drag_snap.dragging.as_deref() != Some(spec_id.as_str());
    drop(drag_snap);

    let section_class = match (is_dragging, is_target) {
        (true, _) => "deck-column deck-column--dragging",
        (_, true) => "deck-column deck-column--drop-target",
        _ => "deck-column",
    };

    rsx! {
        section { class: "{section_class}",
            ColumnHeader { id: spec.id.clone(), title: spec.title.clone(), kind: spec.kind.clone() }
            div { class: "deck-column__body",
                match (&*data.read(), &*error.read(), *loading.read()) {
                    (_, _, true) if data.read().is_empty() => rsx! { div { class: "deck-column__loading", "Loading…" } },
                    (data, _, _) if data.is_empty() => rsx! { div { class: "deck-column__empty", "Nothing here yet." } },
                    (ColumnData::Posts(items), _, _) => rsx! {
                        for item in items.iter() {
                            PostCard { key: "{item.post.uri}", post: item.post.clone() }
                        }
                    },
                    (ColumnData::Notifications { groups, subjects }, _, _) => rsx! {
                        for (i, g) in groups.iter().enumerate() {
                            NotificationCard {
                                key: "{group_key(g, i)}",
                                group: g.clone(),
                                subject: g.items.first().and_then(|n| subject_for(n, subjects)).cloned(),
                            }
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
            }
        }
    }
}

/// One fetch cycle for the column. Returns the freshest page of items;
/// the caller decides whether to install them or stash them as pending.
async fn fetch_once(
    kind: &ColumnKind,
    session_sig: Signal<Option<Session>>,
) -> Result<ColumnData, String> {
    // Demo mode: canned data with no network.
    if crate::demo::is_active() {
        return Ok(match kind {
            ColumnKind::Notifications => {
                let (items, subjects) = crate::demo::notifications_with_subjects();
                let groups = group_notifications(items);
                ColumnData::Notifications { groups, subjects }
            }
            ColumnKind::AuthorFeed { .. } => ColumnData::Posts(crate::demo::home_feed()),
            ColumnKind::Home | ColumnKind::Search { .. } | ColumnKind::Feed { .. } => {
                ColumnData::Posts(crate::demo::home_feed())
            }
        });
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
    match kind {
        ColumnKind::Home => client
            .get_timeline(None, 30)
            .await
            .map(|r| ColumnData::Posts(r.feed))
            .map_err(|e| e.to_string()),
        ColumnKind::AuthorFeed { actor } => client
            .get_author_feed(actor, None, 30)
            .await
            .map(|r| ColumnData::Posts(r.feed))
            .map_err(|e| e.to_string()),
        ColumnKind::Notifications => {
            let items = client
                .list_notifications(None, 50)
                .await
                .map(|r| r.notifications)
                .map_err(|e| e.to_string())?;
            // Hydrate subject posts in one batched call. Failures here
            // shouldn't blank the notifications — fall back to an empty
            // map and the cards just render without quoted context.
            let uris = collect_subject_uris(&items);
            let subjects = if uris.is_empty() {
                HashMap::new()
            } else {
                match client.get_posts(&uris).await {
                    Ok(posts) => posts.into_iter().map(|p| (p.uri.clone(), p)).collect(),
                    Err(_) => HashMap::new(),
                }
            };
            // Collapse 20 likes on the same post into one card etc.
            // Done after hydration so the same subjects map keys still work.
            let groups = group_notifications(items);
            Ok(ColumnData::Notifications { groups, subjects })
        }
        ColumnKind::Search { query } => client
            .search_posts(query, None, 30)
            .await
            .map(|r| ColumnData::Posts(r.feed))
            .map_err(|e| e.to_string()),
        ColumnKind::Feed { uri } => client
            .get_feed(uri, None, 30)
            .await
            .map(|r| ColumnData::Posts(r.feed))
            .map_err(|e| e.to_string()),
    }
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
fn ColumnHeader(id: String, title: String, kind: ColumnKind) -> Element {
    let mut cols = use_context::<Signal<Vec<crate::state::ColumnSpec>>>();
    let mut drag_ctx = use_context::<Signal<ColumnDrag>>();
    let id_for_close = id.clone();
    let close = move |_| {
        crate::state::remove_column(&mut cols, &id_for_close);
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
                    ColumnKind::Home => rsx! { icons::Home { size: icons::Size::Sm } },
                }
            }
            span { class: "deck-column__title", "{title}" }
            button { class: "deck-column__action", title: "Sort",
                icons::ListFilter { size: icons::Size::Sm }
            }
            button { class: "deck-column__action", title: "Close column", onclick: close,
                icons::X { size: icons::Size::Sm }
            }
        }
    }
}
