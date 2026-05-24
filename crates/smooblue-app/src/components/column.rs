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

use crate::components::notification_card::NotificationCard;
use crate::components::post::PostCard;
use crate::icons;
use crate::state::{ColumnKind, ColumnSpec};
use dioxus::prelude::*;
use smooblue_atproto::{AtClient, FeedItem, Notification};
use smooblue_oauth::Session;
use std::time::Duration;
use url::Url;

#[derive(Clone, PartialEq, Default)]
enum ColumnData {
    #[default]
    Empty,
    Posts(Vec<FeedItem>),
    Notifications(Vec<Notification>),
}

impl ColumnData {
    fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Posts(p) => p.is_empty(),
            Self::Notifications(n) => n.is_empty(),
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
    let spec_kind = spec.kind.clone();

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
                let session_now = session_sig.read().clone();
                match fetch_once(&kind, session_now).await {
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

    rsx! {
        section { class: "deck-column",
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
                    (ColumnData::Notifications(items), _, _) => rsx! {
                        for n in items.iter() {
                            NotificationCard { key: "{n.uri}", notif: n.clone() }
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
async fn fetch_once(kind: &ColumnKind, session: Option<Session>) -> Result<ColumnData, String> {
    // Demo mode: canned data with no network.
    if crate::demo::is_active() {
        return Ok(match kind {
            ColumnKind::Notifications => ColumnData::Notifications(crate::demo::notifications()),
            ColumnKind::AuthorFeed { .. } => ColumnData::Posts(crate::demo::home_feed()),
            ColumnKind::Home | ColumnKind::Search { .. } | ColumnKind::Feed { .. } => {
                ColumnData::Posts(crate::demo::home_feed())
            }
        });
    }
    let Some(s) = session else {
        return Err("not signed in".into());
    };
    // OAuth-authenticated calls hit the user's PDS (which proxies app.bsky.*
    // to the AppView with service-auth on our behalf). Hitting the AppView
    // directly with a user token returns 401 AuthMissing.
    let base = Url::parse(&s.pds).map_err(|e| e.to_string())?;
    let client = AtClient::new(s, base);
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
        ColumnKind::Notifications => client
            .list_notifications(None, 30)
            .await
            .map(|r| ColumnData::Notifications(r.notifications))
            .map_err(|e| e.to_string()),
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

#[component]
fn ColumnHeader(id: String, title: String, kind: ColumnKind) -> Element {
    let mut cols = use_context::<Signal<Vec<crate::state::ColumnSpec>>>();
    let id_for_close = id.clone();
    let close = move |_| {
        crate::state::remove_column(&mut cols, &id_for_close);
    };
    rsx! {
        header { class: "deck-column__header",
            span { class: "deck-column__drag",
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
