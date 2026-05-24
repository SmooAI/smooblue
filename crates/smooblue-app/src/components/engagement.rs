//! Engagement sheet — the modal that opens when you tap a like /
//! repost / quote count on a PostCard.
//!
//! Three flavors, all loaded by the same component:
//! - **Likes** — `app.bsky.feed.getLikes` → list of actors who liked
//! - **Reposters** — `app.bsky.feed.getRepostedBy` → list of actors
//! - **Quotes** — `app.bsky.feed.getQuotes` → list of *posts* that
//!   quote this one, rendered with the usual PostCard so likes /
//!   replies / thread-open all keep working inside the modal.
//!
//! The Likes / Reposters lists render as compact actor rows (small
//! avatar + display name + handle). Click an actor → opens the
//! ProfileSheet for them (replacing the EngagementSheet via the
//! ProfileFocus signal).

use crate::auth_refresh::fresh_client;
use crate::components::post::PostCard;
use crate::demo;
use crate::icons;
use crate::state::{Engagement, EngagementFocus, ProfileFocus};
use dioxus::prelude::*;
use smooblue_atproto::{FeedItem, PostAuthor};
use smooblue_oauth::Session;

/// What the sheet body ends up holding — either a list of actors
/// (for Likes / Reposters) or a list of posts (for Quotes). Public
/// so demo mode can construct one directly without round-tripping
/// through a separate type.
#[derive(Clone, PartialEq)]
pub enum Loaded {
    Actors(Vec<PostAuthor>),
    Posts(Vec<FeedItem>),
}

#[component]
pub fn EngagementSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut focus = use_context::<Signal<EngagementFocus>>();
    let snap = focus.read().0.clone();

    let key = snap.clone();
    let data = use_resource(move || {
        let kind = key.clone();
        let session_sig = session;
        async move {
            let Some(kind) = kind else {
                return Err::<Loaded, String>("no focus".into());
            };
            if demo::is_active() {
                return Ok(demo::engagement_for(&kind));
            }
            let Some(client) = fresh_client(session_sig).await else {
                return Err("not signed in".into());
            };
            match kind {
                Engagement::Likes(uri) => client
                    .get_likes(&uri, None, 50)
                    .await
                    .map(|r| Loaded::Actors(r.likes.into_iter().map(|l| l.actor).collect()))
                    .map_err(|e| e.to_string()),
                Engagement::Reposters(uri) => client
                    .get_reposted_by(&uri, None, 50)
                    .await
                    .map(|r| Loaded::Actors(r.reposted_by))
                    .map_err(|e| e.to_string()),
                Engagement::Quotes(uri) => client
                    .get_quotes(&uri, None, 50)
                    .await
                    .map(|r| {
                        Loaded::Posts(r.posts.into_iter().map(|p| FeedItem { post: p }).collect())
                    })
                    .map_err(|e| e.to_string()),
            }
        }
    });

    // `let Some(kind)` binds + early-returns in one step so a future
    // refactor that moves code above this point can't accidentally
    // reach the `.unwrap()` on None.
    let Some(ref kind) = snap else {
        return rsx! { Fragment {} };
    };
    let title = match kind {
        Engagement::Likes(_) => "Likes",
        Engagement::Reposters(_) => "Reposts",
        Engagement::Quotes(_) => "Quotes",
    };

    let close = move |_| {
        focus.set(EngagementFocus(None));
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet engagement__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "engagement__head",
                    span { class: "engagement__title", "{title}" }
                    button { class: "engagement__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "engagement__body",
                    match &*data.read_unchecked() {
                        Some(Ok(Loaded::Actors(actors))) => rsx! {
                            if actors.is_empty() {
                                div { class: "engagement__empty", "No one yet." }
                            } else {
                                for a in actors.iter() {
                                    ActorRow { key: "{a.did}", actor: a.clone() }
                                }
                            }
                        },
                        Some(Ok(Loaded::Posts(items))) => rsx! {
                            if items.is_empty() {
                                div { class: "engagement__empty", "No quotes yet." }
                            } else {
                                for item in items.iter() {
                                    PostCard { key: "{item.post.uri}", post: item.post.clone() }
                                }
                            }
                        },
                        Some(Err(e)) => rsx! {
                            div { class: "engagement__error", "Couldn't load: {e}" }
                        },
                        None => rsx! {
                            div { class: "engagement__loading", "Loading…" }
                        },
                    }
                }
            }
        }
    }
}

/// One row in a Likes / Reposters list — avatar + display name +
/// handle. Click anywhere on the row opens that actor's ProfileSheet
/// (and closes the engagement sheet so we don't stack two modals).
#[component]
fn ActorRow(actor: PostAuthor) -> Element {
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let mut engagement_focus = use_context::<Signal<EngagementFocus>>();
    let did = actor.did.clone();
    let display = actor
        .display_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&actor.handle)
        .to_string();
    let handle = actor.handle.clone();
    let avatar = actor.avatar.clone();
    let did_for_click = did.clone();
    let onclick = move |_| {
        engagement_focus.set(EngagementFocus(None));
        profile_focus.set(ProfileFocus(Some(did_for_click.clone())));
    };
    rsx! {
        button { class: "actor-row", onclick: onclick,
            div { class: "actor-row__avatar",
                if let Some(url) = avatar {
                    img { loading: "lazy", decoding: "async", src: "{url}", alt: "{handle}" }
                } else {
                    div { class: "actor-row__avatar-placeholder",
                        icons::User { size: icons::Size::Sm }
                    }
                }
            }
            div { class: "actor-row__meta",
                span { class: "actor-row__name", "{display}" }
                span { class: "actor-row__handle", "@{handle}" }
            }
        }
    }
}
