//! Saved feeds picker — modal sheet that lists the user's pinned +
//! saved feed generators (from bsky preferences) so they can add
//! any as a deck column with one click.
//!
//! Two-step load: first `get_preferences` for the list of saved feed
//! URIs, then `get_feed_generators` to resolve those URIs into
//! display views (name + description + avatar). Both calls are
//! cached for the lifetime of the sheet — closing + reopening
//! re-fetches.

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::{add_column_unique, ColumnSpec};
use dioxus::prelude::*;
use smooblue_atproto::{FeedGeneratorView, SavedFeedItem};
use smooblue_oauth::Session;

#[derive(Clone, PartialEq)]
struct Loaded {
    /// Resolved feed-generator views, in pinned-first order.
    feeds: Vec<(SavedFeedItem, Option<FeedGeneratorView>)>,
}

#[component]
pub fn SavedFeedsSheet(open: Signal<bool>) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();

    let key = *open.read();
    let data = use_resource(move || {
        let session_sig = session;
        let is_open = key;
        async move {
            if !is_open {
                return Err::<Loaded, String>("closed".into());
            }
            if crate::demo::is_active() {
                return Ok(Loaded {
                    feeds: crate::demo::saved_feeds(),
                });
            }
            let Some(client) = fresh_client(session_sig).await else {
                return Err("not signed in".into());
            };
            let prefs = client.get_preferences().await.map_err(|e| e.to_string())?;
            let saved = prefs.saved_feeds();
            // Resolve only `feed` entries — lists and the "following"
            // timeline don't need getFeedGenerators (Home column
            // already represents the timeline; lists land in their
            // own future picker).
            let feed_uris: Vec<String> = saved
                .iter()
                .filter(|s| s.kind == "feed")
                .map(|s| s.value.clone())
                .collect();
            let views = client
                .get_feed_generators(&feed_uris)
                .await
                .map(|r| r.feeds)
                .unwrap_or_default();
            // Index resolved views by URI so we can match back to
            // saved-feed entries (preserving the user's pinned-first
            // order).
            let view_by_uri: std::collections::HashMap<String, FeedGeneratorView> =
                views.into_iter().map(|v| (v.uri.clone(), v)).collect();
            let mut pinned: Vec<(SavedFeedItem, Option<FeedGeneratorView>)> = Vec::new();
            let mut other: Vec<(SavedFeedItem, Option<FeedGeneratorView>)> = Vec::new();
            for sf in saved.into_iter().filter(|s| s.kind == "feed") {
                let v = view_by_uri.get(&sf.value).cloned();
                if sf.pinned {
                    pinned.push((sf, v));
                } else {
                    other.push((sf, v));
                }
            }
            pinned.extend(other);
            Ok(Loaded { feeds: pinned })
        }
    });

    if !*open.read() {
        return rsx! { Fragment {} };
    }

    let mut open_close = open;
    let close = move |_| {
        open_close.set(false);
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet saved-feeds__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "saved-feeds__head",
                    span { class: "saved-feeds__title", "Your saved feeds" }
                    button { class: "saved-feeds__close", title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "saved-feeds__body",
                    match &*data.read_unchecked() {
                        Some(Ok(loaded)) => rsx! {
                            if loaded.feeds.is_empty() {
                                div { class: "saved-feeds__empty",
                                    "No saved feeds yet. Save a feed on Bluesky and it'll show up here."
                                }
                            } else {
                                for (sf, view) in loaded.feeds.iter() {
                                    SavedFeedRow {
                                        key: "{sf.value}",
                                        saved: sf.clone(),
                                        view: view.clone(),
                                        on_add: {
                                            let mut cols_for_add = cols;
                                            let mut open_after = open;
                                            move |spec: ColumnSpec| {
                                                add_column_unique(&mut cols_for_add, spec);
                                                open_after.set(false);
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        Some(Err(e)) => rsx! {
                            div { class: "saved-feeds__error", "Couldn't load feeds: {e}" }
                        },
                        None => rsx! {
                            div { class: "saved-feeds__loading", "Loading…" }
                        },
                    }
                }
            }
        }
    }
}

#[component]
fn SavedFeedRow(
    saved: SavedFeedItem,
    view: Option<FeedGeneratorView>,
    on_add: EventHandler<ColumnSpec>,
) -> Element {
    let display_name = view
        .as_ref()
        .map(|v| v.display_name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| saved.value.clone());
    let description = view
        .as_ref()
        .and_then(|v| v.description.clone())
        .unwrap_or_default();
    let avatar = view.as_ref().and_then(|v| v.avatar.clone());
    let pinned = saved.pinned;
    let value = saved.value.clone();
    let title_for_add = display_name.clone();
    let add = move |_| {
        on_add.call(ColumnSpec::feed_with_title(value.clone(), title_for_add.clone()));
    };
    rsx! {
        div { class: "saved-feeds__row",
            div { class: "saved-feeds__avatar",
                if let Some(url) = avatar {
                    img { loading: "lazy", decoding: "async", src: "{url}", alt: "{display_name}" }
                } else {
                    div { class: "saved-feeds__avatar-placeholder",
                        icons::Compass { size: icons::Size::Md }
                    }
                }
            }
            div { class: "saved-feeds__meta",
                div { class: "saved-feeds__name-row",
                    span { class: "saved-feeds__name", "{display_name}" }
                    if pinned {
                        span { class: "saved-feeds__pinned", "Pinned" }
                    }
                }
                if !description.is_empty() {
                    p { class: "saved-feeds__desc", "{description}" }
                }
            }
            button { class: "btn btn--primary saved-feeds__add",
                onclick: add,
                "+ Column"
            }
        }
    }
}
