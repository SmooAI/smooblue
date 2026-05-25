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
use smooblue_atproto::feed::TrendingTopic;
use smooblue_atproto::{FeedGeneratorView, ListView, SavedFeedItem};
use smooblue_oauth::Session;

#[derive(Clone, PartialEq)]
struct Loaded {
    /// Resolved feed-generator views, in pinned-first order.
    feeds: Vec<(SavedFeedItem, Option<FeedGeneratorView>)>,
    /// User's own curated lists. Modlists filtered out — only
    /// curatelists make sense as a column.
    lists: Vec<ListView>,
    /// Trending topics from `app.bsky.unspecced.getTrendingTopics`.
    /// Best-effort — empty on failure.
    trending: Vec<TrendingTopic>,
    /// Popular feed generators from
    /// `app.bsky.unspecced.getPopularFeedGenerators`. De-duped
    /// against `feeds` (user's already-saved feeds) so we don't
    /// suggest things they already have.
    popular: Vec<FeedGeneratorView>,
    /// Feed generators the signed-in user has authored (their own
    /// app.bsky.feed.generator records). Surfaces under "Your
    /// feeds" so creators can drop their own work into the deck
    /// without pasting URIs.
    own_feeds: Vec<FeedGeneratorView>,
}

#[component]
pub fn SavedFeedsSheet(open: Signal<bool>) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();

    // Reactive: read `open` *inside* the closure so the resource
    // re-runs when the sheet opens. Capturing `*open.read()` at the
    // outer render freezes the value to whatever it was on first
    // mount (false), then the cached Err("closed") leaks into the
    // UI as 'Couldn't load: closed' after the user opens the sheet.
    let data = use_resource(move || {
        let session_sig = session;
        let is_open = *open.read();
        async move {
            if !is_open {
                return Err::<Loaded, String>("closed".into());
            }
            if crate::demo::is_active() {
                return Ok(Loaded {
                    feeds: crate::demo::saved_feeds(),
                    lists: crate::demo::own_lists(),
                    trending: crate::demo::trending_topics(),
                    popular: crate::demo::popular_feeds(),
                    own_feeds: crate::demo::popular_feeds(),
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

            // Fetch the user's own curated lists too. Modlists are
            // filtered out — those exist as mute/block lists, not as
            // subscribable feeds.
            let session_did = session_sig
                .read()
                .as_ref()
                .map(|s| s.did.clone())
                .unwrap_or_default();
            let lists = if session_did.is_empty() {
                Vec::new()
            } else {
                client
                    .get_lists(&session_did, None, 50)
                    .await
                    .map(|r| {
                        r.lists
                            .into_iter()
                            .filter(|l| l.purpose == "app.bsky.graph.defs#curatelist")
                            .collect()
                    })
                    .unwrap_or_default()
            };

            // Trending topics + popular feeds — both unspecced, both
            // best-effort. Silent failure on each.
            let trending = client
                .get_trending_topics()
                .await
                .map(|r| {
                    let mut all = r.topics;
                    all.extend(r.suggested);
                    all
                })
                .unwrap_or_default();

            let saved_uris: std::collections::HashSet<String> =
                pinned.iter().map(|(sf, _)| sf.value.clone()).collect();
            let popular = client
                .get_popular_feed_generators()
                .await
                .map(|r| {
                    r.feeds
                        .into_iter()
                        .filter(|v| !saved_uris.contains(&v.uri))
                        .collect()
                })
                .unwrap_or_default();

            let own_feeds = client.list_own_feed_generators().await.unwrap_or_default();

            Ok(Loaded {
                feeds: pinned,
                lists,
                trending,
                popular,
                own_feeds,
            })
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
                    // Custom feed by AT-URI — for advanced users who
                    // want a feed that isn't in their bsky-saved list
                    // (someone else's curated feed, an experimental
                    // generator, etc.). One paste + Enter / + Add
                    // mints a deck column.
                    CustomFeedAdd {
                        on_add: {
                            let mut cols_for_add = cols;
                            let mut open_after = open;
                            move |spec: ColumnSpec| {
                                add_column_unique(&mut cols_for_add, spec);
                                open_after.set(false);
                            }
                        }
                    }
                    match &*data.read_unchecked() {
                        Some(Ok(loaded)) => rsx! {
                            if loaded.feeds.is_empty() && loaded.lists.is_empty() {
                                div { class: "saved-feeds__empty",
                                    "No saved feeds or lists yet. Save a feed or create a list on Bluesky and they'll show up here."
                                }
                            }
                            // "Your feeds" — the user's own authored
                            // feed generators (from listRecords on the
                            // app.bsky.feed.generator collection).
                            // Rendered first so they're impossible to
                            // miss when the user comes here looking for
                            // their own work.
                            if !loaded.own_feeds.is_empty() {
                                h3 { class: "saved-feeds__section-title", "Your feeds" }
                                for view in loaded.own_feeds.iter() {
                                    PopularFeedRow {
                                        key: "own-{view.uri}",
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
                            if !loaded.feeds.is_empty() {
                                h3 { class: "saved-feeds__section-title", "Feeds" }
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
                            if !loaded.lists.is_empty() {
                                h3 { class: "saved-feeds__section-title", "Your lists" }
                                for list in loaded.lists.iter() {
                                    ListRow {
                                        key: "{list.uri}",
                                        list: list.clone(),
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
                            if !loaded.trending.is_empty() {
                                h3 { class: "saved-feeds__section-title", "Trending now" }
                                div { class: "trending__chips",
                                    for topic in loaded.trending.iter() {
                                        TrendingChip {
                                            key: "{topic.topic}",
                                            topic: topic.clone(),
                                            on_open: {
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
                            }
                            if !loaded.popular.is_empty() {
                                h3 { class: "saved-feeds__section-title", "Popular feeds" }
                                for view in loaded.popular.iter() {
                                    PopularFeedRow {
                                        key: "popular-{view.uri}",
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
                        Some(Err(e)) => {
                            // Stale "closed" Err from the resource's
                            // first run is not a user-visible error.
                            if e == "closed" {
                                rsx! { div { class: "saved-feeds__loading", "Loading…" } }
                            } else {
                                rsx! { div { class: "saved-feeds__error", "Couldn't load: {e}" } }
                            }
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
fn ListRow(list: ListView, on_add: EventHandler<ColumnSpec>) -> Element {
    let display = list.name.clone();
    let desc = list.description.clone().unwrap_or_default();
    let avatar = list.avatar.clone();
    let count = list.list_item_count.unwrap_or(0);
    let uri = list.uri.clone();
    let title_for_add = display.clone();
    let add = move |_| {
        on_add.call(ColumnSpec::list(uri.clone(), title_for_add.clone()));
    };
    rsx! {
        div { class: "saved-feeds__row",
            div { class: "saved-feeds__avatar",
                if let Some(url) = avatar {
                    img { loading: "lazy", decoding: "async", src: "{url}", alt: "{display}" }
                } else {
                    div { class: "saved-feeds__avatar-placeholder",
                        icons::Users { size: icons::Size::Md }
                    }
                }
            }
            div { class: "saved-feeds__meta",
                div { class: "saved-feeds__name-row",
                    span { class: "saved-feeds__name", "{display}" }
                    span { class: "saved-feeds__pinned", "{count} accounts" }
                }
                if !desc.is_empty() {
                    p { class: "saved-feeds__desc", "{desc}" }
                }
            }
            button { class: "btn btn--primary saved-feeds__add",
                onclick: add,
                "+ Column"
            }
        }
    }
}

/// Paste-an-AT-URI box at the top of the saved-feeds sheet.
/// Accepts either an `at://did:plc:.../app.bsky.feed.generator/<rkey>`
/// URI directly, or a `https://bsky.app/profile/<handle>/feed/<rkey>`
/// link that we translate. Trims aggressively; rejects anything
/// that doesn't smell like a feed URI so we don't add bogus columns.
#[component]
fn CustomFeedAdd(on_add: EventHandler<ColumnSpec>) -> Element {
    let mut value = use_signal(String::new);
    let mut err = use_signal(|| None::<String>);

    let mut try_add_inner = move || {
        let raw = value.read().trim().to_string();
        if raw.is_empty() {
            return;
        }
        // bsky.app links → at-uri. Form: /profile/<handleOrDid>/feed/<rkey>
        let normalized: Option<String> = if let Some(rest) =
            raw.strip_prefix("https://bsky.app/profile/")
        {
            let mut parts = rest.split('/');
            let actor = parts.next().unwrap_or("");
            let kind = parts.next().unwrap_or("");
            let rkey = parts.next().unwrap_or("");
            if kind == "feed" && !actor.is_empty() && !rkey.is_empty() {
                // bsky.app uses handles in URLs but the AT-URI needs
                // a DID. Most generator profile pages also serve the
                // DID-keyed URL; if the user passed a handle we
                // surface an error guiding them to the at-uri form.
                if actor.starts_with("did:") {
                    Some(format!("at://{actor}/app.bsky.feed.generator/{rkey}"))
                } else {
                    err.set(Some("bsky.app link uses a handle — paste the at://did:plc:.../... form instead.".into()));
                    None
                }
            } else {
                err.set(Some("That bsky.app link doesn't look like a feed.".into()));
                None
            }
        } else if raw.starts_with("at://") && raw.contains("/app.bsky.feed.generator/") {
            Some(raw.clone())
        } else {
            err.set(Some(
                "Paste an at://… feed URI or a bsky.app /feed/ link.".into(),
            ));
            None
        };
        if let Some(uri) = normalized {
            err.set(None);
            value.set(String::new());
            // Title falls back to the rkey until the column header
            // fetch resolves the real generator name.
            let title = uri.rsplit('/').next().unwrap_or("Custom feed").to_string();
            on_add.call(ColumnSpec::feed_with_title(uri, title));
        }
    };

    rsx! {
        h3 { class: "saved-feeds__section-title", "Add a custom feed" }
        p { class: "custom-feed__hint",
            "Paste a feed AT-URI (at://did:plc:…/app.bsky.feed.generator/…)."
        }
        div { class: "custom-feed__row",
            input { class: "input",
                placeholder: "at://did:plc:.../app.bsky.feed.generator/...",
                value: "{value}",
                oninput: move |e| value.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        try_add_inner();
                    }
                },
            }
            button { class: "btn btn--primary",
                onclick: move |_| try_add_inner(),
                "+ Add"
            }
        }
        if let Some(msg) = err.read().clone() {
            p { class: "custom-feed__hint", style: "color: var(--color-smooai-red)",
                "{msg}"
            }
        }
    }
}

/// Pill rendering one trending topic. Clicking opens a search
/// column for the topic text (the `link` field would route inside
/// bsky.app to either a search or a profile; we map any link to
/// a search column for simplicity).
#[component]
fn TrendingChip(topic: TrendingTopic, on_open: EventHandler<ColumnSpec>) -> Element {
    let label = topic
        .display_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| topic.topic.clone());
    let query = topic.topic.clone();
    let click = move |_| {
        on_open.call(ColumnSpec::search(query.clone()));
    };
    rsx! {
        button { class: "trending__chip",
            title: "{topic.description.clone().unwrap_or_default()}",
            onclick: click,
            "{label}"
        }
    }
}

/// Row for one of the bsky-curated popular feed generators. Same
/// shape as SavedFeedRow but without the "Pinned" badge — these
/// aren't subscribed yet, so the "+ Column" button is the
/// affordance for adopting them.
#[component]
fn PopularFeedRow(view: FeedGeneratorView, on_add: EventHandler<ColumnSpec>) -> Element {
    let display_name = view.display_name.clone();
    let description = view.description.clone().unwrap_or_default();
    let avatar = view.avatar.clone();
    let uri = view.uri.clone();
    let title_for_add = display_name.clone();
    let add = move |_| {
        on_add.call(ColumnSpec::feed_with_title(
            uri.clone(),
            title_for_add.clone(),
        ));
    };
    rsx! {
        div { class: "saved-feeds__row",
            div { class: "saved-feeds__avatar",
                if let Some(url) = avatar {
                    img { loading: "lazy", decoding: "async", src: "{url}", alt: "{display_name}" }
                } else {
                    div { class: "saved-feeds__avatar-placeholder",
                        icons::Sparkles { size: icons::Size::Md }
                    }
                }
            }
            div { class: "saved-feeds__meta",
                div { class: "saved-feeds__name-row",
                    span { class: "saved-feeds__name", "{display_name}" }
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
        on_add.call(ColumnSpec::feed_with_title(
            value.clone(),
            title_for_add.clone(),
        ));
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
