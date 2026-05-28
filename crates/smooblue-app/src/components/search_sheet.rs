//! Live search across users + posts, with per-result "open" + an
//! always-available "add as column" affordance. Replaces the old
//! flow which jumped straight to creating a search column.
//!
//! Layout:
//! ```text
//!  ┌─ Search ──────────────────────────────────────────────┐
//!  │ [ search box                                      ✕ ] │
//!  │                                                       │
//!  │ Users                                                 │
//!  │   ◉ Alice  @alice.bsky.social         [+ column]      │
//!  │   ◉ Bob    @bob.bsky.social           [+ column]      │
//!  │                                                       │
//!  │ Posts                                                 │
//!  │   ▢ <post card>                                       │
//!  │   ▢ <post card>                                       │
//!  │                                                       │
//!  │           [ Add search column for "alice" ]           │
//!  └───────────────────────────────────────────────────────┘
//! ```
//!
//! - Clicking a user row opens their profile sheet.
//! - The per-user "+ column" button adds an author-feed column.
//! - Clicking a post opens the thread sheet.
//! - The footer button materialises the current query as a search
//!   column (the old behaviour, now opt-in).

use crate::auth_refresh::fresh_client;
use crate::components::post::PostCard;
use crate::demo;
use crate::icons;
use crate::state::{add_or_focus_column, ColumnSpec, FocusColumn, ProfileFocus, ThreadFocus};
use dioxus::prelude::*;
use smooblue_atproto::{ActorProfile, FeedItem};
use smooblue_oauth::Session;

/// What the live search returns — actors + posts in parallel.
/// Public so demo mode can construct one without re-deriving the
/// fields (same pattern as engagement::Loaded).
#[derive(Clone, PartialEq, Default)]
pub struct SearchResults {
    pub actors: Vec<ActorProfile>,
    pub posts: Vec<FeedItem>,
}

#[component]
pub fn SearchSheet(open: Signal<bool>) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let mut focus_col = use_context::<Signal<FocusColumn>>();
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let mut thread_focus = use_context::<Signal<ThreadFocus>>();

    let mut query = use_signal(String::new);

    // Debounced version of `query` so we don't fire an XRPC call on
    // every keystroke. Updated by the spawn below after 250ms of
    // typing-quiet, which is enough to feel "live" without spamming
    // the AppView.
    let mut debounced = use_signal(String::new);

    // Whenever query changes, schedule a delayed copy into
    // `debounced`. The Pending counter cancels prior schedules by
    // making any stale closure no-op on wakeup (counter advanced).
    let mut pending = use_signal(|| 0u64);
    use_effect(move || {
        let q = query.read().trim().to_string();
        let my_token = pending.write().wrapping_add(1);
        *pending.write() = my_token;
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if *pending.read() != my_token {
                return;
            }
            if *debounced.peek() != q {
                debounced.set(q);
            }
        });
    });

    // Fetch results when the debounced query changes. Empty query
    // resets to an empty result set (no spinner shown).
    let results = use_resource(move || {
        let session_sig = session;
        let q = debounced.read().clone();
        async move {
            if q.is_empty() {
                return Ok::<SearchResults, String>(SearchResults::default());
            }
            if demo::is_active() {
                return Ok(demo::search_results_for(&q));
            }
            let Some(client) = fresh_client(session_sig).await else {
                return Err("not signed in".into());
            };
            // Run both lookups in parallel — search is interactive,
            // we want the UI to populate as soon as either side
            // returns rather than serialising.
            let (actors_res, posts_res) = tokio::join!(
                client.search_actors(&q, 8),
                client.search_posts(&q, None, 12),
            );
            let actors = actors_res.unwrap_or_default();
            let posts = posts_res.map(|r| r.feed).unwrap_or_default();
            Ok(SearchResults { actors, posts })
        }
    });

    if !*open.read() {
        return rsx! { Fragment {} };
    }

    let close = move |_| open.set(false);

    // Footer "Add search column" — works whether or not the live
    // results have populated. Signal<T> is Copy so we can capture
    // `debounced` directly into the closure.
    let add_search_column = move |_| {
        let q = debounced.read().trim().to_string();
        if q.is_empty() {
            return;
        }
        add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::search(q));
        query.set(String::new());
        open.set(false);
    };

    let debounced_snap = debounced.read().clone();
    let snap = results.read();

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet search__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "compose__head",
                    span { class: "compose__title", "Search" }
                    button { class: "compose__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                input {
                    class: "input input--lg search__input",
                    placeholder: "Search users + posts…",
                    autofocus: true,
                    value: "{query}",
                    oninput: move |e| query.set(e.value()),
                }

                div { class: "search__body",
                    match &*snap {
                        None => rsx! {
                            // First render — nothing fetched yet.
                            // Empty placeholder; the input has focus
                            // and the user is typing.
                            div { class: "search__hint",
                                "Type to search across users + posts. Each result opens directly; \"+\" pins one as a column."
                            }
                        },
                        Some(Err(msg)) => rsx! {
                            div { class: "search__error", "Search failed: {msg}" }
                        },
                        Some(Ok(SearchResults { actors, posts })) if actors.is_empty() && posts.is_empty() => rsx! {
                            if debounced_snap.is_empty() {
                                div { class: "search__hint",
                                    "Type to search across users + posts. Each result opens directly; \"+\" pins one as a column."
                                }
                            } else {
                                div { class: "search__hint",
                                    "No matches for \"{debounced_snap}\". Try a different term, or pin a search column below."
                                }
                            }
                        },
                        Some(Ok(SearchResults { actors, posts })) => rsx! {
                            if !actors.is_empty() {
                                section { class: "search__section",
                                    h3 { class: "search__section-title", "Users" }
                                    for actor in actors.iter() {
                                        ActorRow {
                                            key: "{actor.did}",
                                            actor: actor.clone(),
                                            on_open_profile: {
                                                let did = actor.did.clone();
                                                move |_| {
                                                    profile_focus.set(ProfileFocus(Some(did.clone())));
                                                    open.set(false);
                                                }
                                            },
                                            on_pin_column: {
                                                let did = actor.did.clone();
                                                let handle = actor.handle.clone();
                                                move |_| {
                                                    add_or_focus_column(
                                                        &mut cols,
                                                        &mut focus_col,
                                                        ColumnSpec::author(did.clone(), format!("@{handle}")),
                                                    );
                                                }
                                            },
                                        }
                                    }
                                }
                            }
                            if !posts.is_empty() {
                                section { class: "search__section",
                                    h3 { class: "search__section-title", "Posts" }
                                    for item in posts.iter() {
                                        div { key: "{item.post.uri}",
                                            class: "search__post-row",
                                            onclick: {
                                                let uri = item.post.uri.clone();
                                                move |_| {
                                                    thread_focus.set(ThreadFocus(Some(uri.clone())));
                                                    open.set(false);
                                                }
                                            },
                                            PostCard { post: item.post.clone() }
                                        }
                                    }
                                }
                            }
                        },
                    }
                }

                div { class: "compose__bar",
                    button {
                        class: "btn btn--primary",
                        disabled: debounced_snap.trim().is_empty(),
                        onclick: add_search_column,
                        icons::Search { size: icons::Size::Sm }
                        if debounced_snap.trim().is_empty() {
                            "Add as search column"
                        } else {
                            "Add search column for \"{debounced_snap}\""
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ActorRow(
    actor: ActorProfile,
    on_open_profile: EventHandler<MouseEvent>,
    on_pin_column: EventHandler<MouseEvent>,
) -> Element {
    let display = actor
        .display_name
        .clone()
        .unwrap_or_else(|| actor.handle.clone());
    let avatar = actor.avatar.clone().unwrap_or_default();
    rsx! {
        div { class: "search__actor-row",
            onclick: move |e| on_open_profile.call(e),
            if !avatar.is_empty() {
                img { class: "search__actor-avatar",
                    loading: "lazy",
                    decoding: "async",
                    src: "{avatar}",
                    alt: "{actor.handle}"
                }
            } else {
                div { class: "search__actor-avatar search__actor-avatar--empty" }
            }
            div { class: "search__actor-text",
                div { class: "search__actor-name", "{display}" }
                div { class: "search__actor-handle", "@{actor.handle}" }
            }
            button { class: "search__actor-pin",
                title: "Add @{actor.handle} as a column",
                onclick: move |e: MouseEvent| {
                    e.stop_propagation();
                    on_pin_column.call(e);
                },
                icons::Plus { size: icons::Size::Sm }
            }
        }
    }
}
