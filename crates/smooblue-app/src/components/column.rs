//! A single deck column. Owns its own fetch state.
//!
//! A column's body is either a feed of posts (Home, AuthorFeed, Discover,
//! Search, custom feeds) or a feed of notifications (Notifications). Those
//! are different shapes, so [`ColumnData`] tags which view to render.

use crate::components::notification_card::NotificationCard;
use crate::components::post::PostCard;
use crate::icons;
use crate::state::{ColumnKind, ColumnSpec};
use dioxus::prelude::*;
use smooblue_atproto::{AtClient, FeedItem, Notification};
use smooblue_oauth::Session;
use url::Url;

#[derive(Clone, PartialEq)]
enum ColumnData {
    Posts(Vec<FeedItem>),
    Notifications(Vec<Notification>),
}

impl ColumnData {
    fn is_empty(&self) -> bool {
        match self {
            Self::Posts(p) => p.is_empty(),
            Self::Notifications(n) => n.is_empty(),
        }
    }
}

#[component]
pub fn Column(spec: ColumnSpec) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let spec_kind = spec.kind.clone();

    // Async fetch — re-runs when the session changes (login/refresh).
    let session_for_fetch = session.read().clone();
    let feed = use_resource(move || {
        let kind = spec_kind.clone();
        let session = session_for_fetch.clone();
        async move {
            // Demo mode short-circuits the API and returns canned data so
            // screenshots / docs / slowmo tours render without OAuth.
            if crate::demo::is_active() {
                return Ok::<ColumnData, String>(match kind {
                    ColumnKind::Notifications => {
                        ColumnData::Notifications(crate::demo::notifications())
                    }
                    ColumnKind::Home | ColumnKind::Search { .. } | ColumnKind::Feed { .. } => {
                        ColumnData::Posts(crate::demo::home_feed())
                    }
                    ColumnKind::AuthorFeed { .. } => ColumnData::Posts(crate::demo::home_feed()),
                });
            }
            let Some(s) = session else {
                return Err::<ColumnData, String>("not signed in".into());
            };
            let appview = Url::parse("https://api.bsky.app").map_err(|e| e.to_string())?;
            let client = AtClient::new(s, appview);
            match kind {
                ColumnKind::Home => client
                    .get_timeline(None, 30)
                    .await
                    .map(|r| ColumnData::Posts(r.feed))
                    .map_err(|e| e.to_string()),
                ColumnKind::AuthorFeed { actor } => client
                    .get_author_feed(&actor, None, 30)
                    .await
                    .map(|r| ColumnData::Posts(r.feed))
                    .map_err(|e| e.to_string()),
                ColumnKind::Notifications => client
                    .list_notifications(None, 30)
                    .await
                    .map(|r| ColumnData::Notifications(r.notifications))
                    .map_err(|e| e.to_string()),
                _ => Ok(ColumnData::Posts(Vec::new())),
            }
        }
    });

    rsx! {
        section { class: "deck-column",
            ColumnHeader { title: spec.title.clone(), kind: spec.kind.clone() }
            div { class: "deck-column__body",
                match &*feed.read_unchecked() {
                    Some(Ok(data)) if data.is_empty() => rsx! { div { class: "deck-column__empty", "Nothing here yet." } },
                    Some(Ok(ColumnData::Posts(items))) => rsx! {
                        for item in items.iter() {
                            PostCard { key: "{item.post.uri}", post: item.post.clone() }
                        }
                    },
                    Some(Ok(ColumnData::Notifications(items))) => rsx! {
                        for n in items.iter() {
                            NotificationCard { key: "{n.uri}", notif: n.clone() }
                        }
                    },
                    Some(Err(e)) => rsx! { div { class: "deck-column__error", "Failed to load: {e}" } },
                    None => rsx! { div { class: "deck-column__loading", "Loading…" } },
                }
            }
        }
    }
}

#[component]
fn ColumnHeader(title: String, kind: ColumnKind) -> Element {
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
            button { class: "deck-column__action", title: "Column settings",
                icons::Settings2 { size: icons::Size::Sm }
            }
        }
    }
}
