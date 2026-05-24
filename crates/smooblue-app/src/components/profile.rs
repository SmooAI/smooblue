//! Profile detail view — modal sheet that opens when you click an
//! avatar anywhere in the deck.
//!
//! Layout (top → bottom):
//! - **Banner** image (or a smoo-gradient fallback when the actor
//!   hasn't set one). Avatar overlaps the bottom-left, half-on / half-off.
//! - **Header**: display name + handle + "follows you" badge.
//! - **Stat row**: posts / following / followers, tabular numerics.
//! - **Bio** (description). Preserves linebreaks.
//! - **Actions**: Follow/Following toggle (smoo-orange when not yet
//!   following), and "Add as column" — secondary, opens the actor's
//!   AuthorFeed as a permanent deck column.
//! - **Recent posts** — the actor's `getAuthorFeed` rendered with the
//!   same PostCard component used everywhere else, so likes /
//!   reposts / replies / thread-open all work inside the modal.

use crate::auth_refresh::fresh_client;
use crate::components::post::PostCard;
use crate::demo;
use crate::icons;
use crate::state::{add_column_unique, ColumnSpec, ProfileFocus};
use dioxus::prelude::*;
use smooblue_atproto::{ActorProfile, FeedItem, PostAuthor};
use smooblue_oauth::Session;

/// Combined snapshot loaded on open — one network round-trip for the
/// profile, one for the first page of their posts. Rendered together
/// so the user doesn't see two separate loading states.
#[derive(Clone, PartialEq)]
struct ProfileData {
    profile: ActorProfile,
    feed: Vec<FeedItem>,
}

#[component]
pub fn ProfileSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut focus = use_context::<Signal<ProfileFocus>>();
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let snap = focus.read().0.clone();

    let key = snap.clone();
    let data = use_resource(move || {
        let actor = key.clone();
        let session_sig = session;
        async move {
            let Some(actor) = actor else {
                return Err::<ProfileData, String>("no focus".into());
            };
            if demo::is_active() {
                let (profile, feed) = demo::profile_for(&actor);
                return Ok(ProfileData { profile, feed });
            }
            let Some(client) = fresh_client(session_sig).await else {
                return Err("not signed in".into());
            };
            // Sequential is fine — profile load is one-off, not a poll.
            let profile = client.get_profile(&actor).await.map_err(|e| e.to_string())?;
            let feed = client
                .get_author_feed(&actor, None, 30)
                .await
                .map(|r| r.feed)
                .map_err(|e| e.to_string())?;
            Ok(ProfileData { profile, feed })
        }
    });

    if snap.is_none() {
        return rsx! { Fragment {} };
    }

    let close = move |_| {
        focus.set(ProfileFocus(None));
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet profile__sheet",
                onclick: move |e| e.stop_propagation(),
                button { class: "profile__close",
                    title: "Close (Esc)",
                    onclick: close,
                    icons::X { size: icons::Size::Sm }
                }
                match &*data.read_unchecked() {
                    Some(Ok(d)) => rsx! {
                        ProfileBody {
                            data: d.clone(),
                            on_add_column: move |spec: ColumnSpec| {
                                add_column_unique(&mut cols, spec);
                                focus.set(ProfileFocus(None));
                            },
                        }
                    },
                    Some(Err(e)) => rsx! {
                        div { class: "profile__error", "Couldn't load profile: {e}" }
                    },
                    None => rsx! {
                        div { class: "profile__loading", "Loading profile…" }
                    },
                }
            }
        }
    }
}

#[component]
fn ProfileBody(data: ProfileData, on_add_column: EventHandler<ColumnSpec>) -> Element {
    let p = data.profile.clone();
    let session = use_context::<Signal<Option<Session>>>();
    let name = p
        .display_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&p.handle)
        .to_string();
    let handle = p.handle.clone();
    let did = p.did.clone();
    let banner = p.banner.clone();
    let avatar = p.avatar.clone();
    let description = p.description.clone().unwrap_or_default();
    let posts = p.posts_count.unwrap_or(0);
    let following = p.follows_count.unwrap_or(0);
    let followers = p.followers_count.unwrap_or(0);
    let followed_by = p
        .viewer
        .as_ref()
        .and_then(|v| v.followed_by.as_ref())
        .is_some();

    // Follow state — server truth from viewer.following plus an
    // optimistic flip on click (mirrors the PostCard like/repost pattern).
    let initial_follow_uri = p
        .viewer
        .as_ref()
        .and_then(|v| v.following.as_ref().cloned());
    let mut follow_uri = use_signal(|| initial_follow_uri.clone());
    let mut follow_pending = use_signal(|| false);
    let is_following = follow_uri.read().is_some();

    let did_for_follow = did.clone();
    let toggle_follow = move |_| {
        if *follow_pending.read() {
            return;
        }
        if session.read().is_none() {
            return;
        }
        follow_pending.set(true);
        let did_clone = did_for_follow.clone();
        let currently_following = follow_uri.read().clone();
        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                follow_pending.set(false);
                return;
            };
            if let Some(uri) = currently_following {
                // Unfollow.
                match client.delete_record(&uri).await {
                    Ok(_) => follow_uri.set(None),
                    Err(e) => tracing::warn!(error = %e, "smooblue: unfollow failed"),
                }
            } else {
                match client.create_follow(&did_clone).await {
                    Ok(rec) => follow_uri.set(Some(rec.uri)),
                    Err(e) => tracing::warn!(error = %e, "smooblue: follow failed"),
                }
            }
            follow_pending.set(false);
        });
    };

    let did_for_column = did.clone();
    let name_for_column = name.clone();
    let add_column = move |_| {
        on_add_column.call(ColumnSpec::author(did_for_column.clone(), name_for_column.clone()));
    };

    let banner_style = match banner.as_deref() {
        Some(url) if !url.is_empty() => format!("background-image: url('{url}'); background-size: cover; background-position: center;"),
        _ => "background: var(--gradient-brand);".to_string(),
    };
    let follow_button_class = if is_following {
        "btn btn--secondary profile__follow profile__follow--following"
    } else {
        "btn btn--primary profile__follow"
    };
    let follow_label = if *follow_pending.read() {
        "…"
    } else if is_following {
        "Following"
    } else {
        "Follow"
    };

    rsx! {
        div { class: "profile__banner", style: "{banner_style}" }
        div { class: "profile__head",
            div { class: "profile__avatar-frame",
                if let Some(url) = avatar.as_ref() {
                    img { class: "profile__avatar", src: "{url}", alt: "{handle}" }
                } else {
                    div { class: "profile__avatar profile__avatar--placeholder",
                        icons::User { size: icons::Size::Lg }
                    }
                }
            }
            div { class: "profile__actions",
                if session.read().is_some() {
                    button {
                        class: "{follow_button_class}",
                        disabled: *follow_pending.read(),
                        onclick: toggle_follow,
                        "{follow_label}"
                    }
                }
                button {
                    class: "btn btn--ghost profile__add-column",
                    title: "Add as a deck column",
                    onclick: add_column,
                    "+ Column"
                }
            }
        }
        div { class: "profile__identity",
            div { class: "profile__name-row",
                span { class: "profile__name", "{name}" }
                if followed_by {
                    span { class: "profile__followed-by", "Follows you" }
                }
            }
            span { class: "profile__handle", "@{handle}" }
        }
        if !description.trim().is_empty() {
            p { class: "profile__bio", "{description}" }
        }
        KnownFollowersRow { actor: did.clone() }
        div { class: "profile__stats",
            div { class: "profile__stat",
                span { class: "profile__stat-num", "{format_count(posts)}" }
                span { class: "profile__stat-label", "posts" }
            }
            div { class: "profile__stat",
                span { class: "profile__stat-num", "{format_count(following)}" }
                span { class: "profile__stat-label", "following" }
            }
            div { class: "profile__stat",
                span { class: "profile__stat-num", "{format_count(followers)}" }
                span { class: "profile__stat-label", "followers" }
            }
        }
        div { class: "profile__feed",
            if data.feed.is_empty() {
                div { class: "profile__feed-empty", "No posts yet." }
            } else {
                for item in data.feed.iter() {
                    PostCard { key: "{item.post.uri}", post: item.post.clone() }
                }
            }
        }
    }
}

/// "Followed by alice, bob and 12 others you follow" — the mutuals
/// social-proof row that bsky.app shows under the bio. Loads
/// `app.bsky.graph.getKnownFollowers` lazily. Silent on failure
/// (just renders nothing) so a transient network blip doesn't
/// pollute the profile.
#[component]
fn KnownFollowersRow(actor: String) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let key = actor.clone();
    let followers = use_resource(move || {
        let actor = key.clone();
        let session_sig = session;
        async move {
            if demo::is_active() {
                return Ok::<Vec<PostAuthor>, String>(demo::known_followers_for(&actor));
            }
            let Some(client) = fresh_client(session_sig).await else {
                return Err("not signed in".into());
            };
            client
                .get_known_followers(&actor, None, 12)
                .await
                .map(|r| r.followers)
                .map_err(|e| e.to_string())
        }
    });

    let snap = followers.read_unchecked();
    let Some(Ok(list)) = snap.as_ref() else {
        // Either still loading or errored — render nothing rather
        // than a placeholder; the row is purely additive.
        return rsx! { Fragment {} };
    };
    if list.is_empty() {
        return rsx! { Fragment {} };
    }
    // Show up to 3 avatars + names inline, then "+N others you follow"
    // for the remainder. Counts above 12 are capped by the API call's
    // limit anyway; we surface the visible-count as a rough lower bound.
    let inline_n = list.len().min(3);
    let inline: Vec<PostAuthor> = list.iter().take(inline_n).cloned().collect();
    let extra = list.len().saturating_sub(inline_n);

    rsx! {
        div { class: "profile__mutuals",
            div { class: "profile__mutuals-avatars",
                for (i, a) in inline.iter().enumerate() {
                    if let Some(url) = a.avatar.as_ref() {
                        img {
                            key: "{i}",
                            class: "profile__mutuals-avatar",
                            src: "{url}",
                            alt: "{a.handle}",
                            title: "{a.handle}",
                        }
                    }
                }
            }
            span { class: "profile__mutuals-text",
                "Followed by "
                for (i, a) in inline.iter().enumerate() {
                    span { key: "{i}", class: "profile__mutuals-name",
                        if let Some(name) = a.display_name.as_ref().filter(|s| !s.is_empty()) {
                            "{name}"
                        } else {
                            "@{a.handle}"
                        }
                    }
                    if i + 1 < inline.len() {
                        ", "
                    }
                }
                if extra > 0 {
                    " and {extra} other"
                    if extra != 1 { "s" }
                    " you follow"
                }
            }
        }
    }
}

/// Render large counts compactly: 1234 → "1.2K", 1_234_567 → "1.2M".
fn format_count(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        let v = n as f64 / 1_000.0;
        if v >= 100.0 {
            format!("{v:.0}K")
        } else {
            format!("{v:.1}K")
        }
    } else {
        let v = n as f64 / 1_000_000.0;
        if v >= 100.0 {
            format!("{v:.0}M")
        } else {
            format!("{v:.1}M")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_count_picks_compact_suffix() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(42), "42");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1.0K");
        assert_eq!(format_count(1_234), "1.2K");
        assert_eq!(format_count(99_999), "100.0K");
        assert_eq!(format_count(100_000), "100K");
        assert_eq!(format_count(1_234_567), "1.2M");
        assert_eq!(format_count(123_456_789), "123M");
    }
}
