//! A single deck column. Owns its own fetch state.

use crate::components::post::PostCard;
use crate::state::{ColumnKind, ColumnSpec};
use dioxus::prelude::*;
use smooblue_atproto::{AtClient, FeedItem};
use smooblue_oauth::Session;
use url::Url;

#[component]
pub fn Column(spec: ColumnSpec) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let spec_kind = spec.kind.clone();

    // Async timeline fetch — re-runs when the session changes (login/refresh).
    let session_for_fetch = session.read().clone();
    let feed = use_resource(move || {
        let kind = spec_kind.clone();
        let session = session_for_fetch.clone();
        async move {
            let Some(s) = session else {
                return Err::<Vec<FeedItem>, String>("not signed in".into());
            };
            let appview = Url::parse("https://api.bsky.app").map_err(|e| e.to_string())?;
            let client = AtClient::new(s, appview);
            match kind {
                ColumnKind::Home => client
                    .get_timeline(None, 30)
                    .await
                    .map(|r| r.feed)
                    .map_err(|e| e.to_string()),
                ColumnKind::AuthorFeed { actor } => client
                    .get_author_feed(&actor, None, 30)
                    .await
                    .map(|r| r.feed)
                    .map_err(|e| e.to_string()),
                _ => Ok(Vec::new()),
            }
        }
    });

    rsx! {
        section { class: "deck-column",
            ColumnHeader { title: spec.title.clone() }
            div { class: "deck-column__body",
                match &*feed.read_unchecked() {
                    Some(Ok(items)) if items.is_empty() => rsx! { div { class: "deck-column__empty", "Nothing here yet." } },
                    Some(Ok(items)) => rsx! {
                        for item in items.iter() {
                            PostCard { key: "{item.post.uri}", post: item.post.clone() }
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
fn ColumnHeader(title: String) -> Element {
    rsx! {
        header { class: "deck-column__header",
            span { class: "deck-column__drag",
                svg {
                    width: "10",
                    height: "16",
                    view_box: "0 0 10 16",
                    fill: "currentColor",
                    circle { cx: "2", cy: "3", r: "1.2" }
                    circle { cx: "8", cy: "3", r: "1.2" }
                    circle { cx: "2", cy: "8", r: "1.2" }
                    circle { cx: "8", cy: "8", r: "1.2" }
                    circle { cx: "2", cy: "13", r: "1.2" }
                    circle { cx: "8", cy: "13", r: "1.2" }
                }
            }
            span { class: "deck-column__icon",
                svg {
                    width: "12",
                    height: "12",
                    view_box: "0 0 24 24",
                    fill: "currentColor",
                    path { d: "M3 11.5L12 4l9 7.5V20a1 1 0 01-1 1h-5v-6h-6v6H4a1 1 0 01-1-1v-8.5z" }
                }
            }
            span { class: "deck-column__title", "{title}" }
            button { class: "deck-column__action", title: "Sort",
                svg { width: "14", height: "14", view_box: "0 0 24 24", fill: "currentColor",
                    path { d: "M3 6h18v2H3zM6 11h12v2H6zM9 16h6v2H9z" }
                }
            }
            button { class: "deck-column__action", title: "Settings",
                svg { width: "14", height: "14", view_box: "0 0 24 24", fill: "currentColor",
                    path { d: "M12 8a4 4 0 100 8 4 4 0 000-8zm9 4l-2.1 1.65c.04.32.07.65.07.98 0 .33-.03.66-.07.98L21 17.26l-2 3.46-2.49-1c-.52.4-1.08.73-1.69.98l-.38 2.65A.5.5 0 0114 24h-4a.5.5 0 01-.49-.42l-.38-2.65a7.03 7.03 0 01-1.69-.98l-2.49 1-2-3.46L3.07 14a7.03 7.03 0 010-2L1 10.34l2-3.46 2.49 1c.52-.4 1.08-.73 1.69-.98L7.5 4.42A.5.5 0 018 4h4c.24 0 .45.18.49.42l.38 2.65c.61.25 1.17.58 1.69.98l2.49-1 2 3.46L21 12z" }
                }
            }
        }
    }
}
