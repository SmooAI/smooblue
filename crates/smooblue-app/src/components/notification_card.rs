//! A single notification card — but the unit is a [`NotificationGroup`],
//! not a raw Notification. Groups collapse e.g. 20 likes on the same
//! post into one card: "Alice, Bob and 18 others liked your post"
//! with stacked avatars on the left. Singletons (replies, mentions,
//! quotes) render the same way they did pre-grouping: avatar + name
//! + quoted subject post.
//!
//! Reading the unread state per-group: any unread item in the group
//! marks the whole card unread, since the user hasn't seen all of
//! it yet.

use crate::components::embed::EmbedView;
use crate::icons;
use crate::state::{ProfileFocus, Tick};
use dioxus::prelude::*;
use smooblue_atproto::{NotificationGroup, PostAuthor, PostView};

#[component]
pub fn NotificationCard(group: NotificationGroup, subject: Option<PostView>) -> Element {
    // Subscribe to the global tick so `relative_time()` text refreshes.
    let _tick = use_context::<Signal<Tick>>().read().0;

    let first = group.items.first().cloned().unwrap_or_else(|| {
        // Defensive: an empty group shouldn't reach the renderer, but
        // if it does, fall back to a blank card rather than panicking.
        // Build a placeholder so the rsx still compiles.
        unreachable!("NotificationGroup::items is invariant non-empty")
    });
    let reason = group.reason.clone();
    let unread = group.any_unread();
    let unread_class = if unread { "notif notif--unread" } else { "notif" };
    let icon_color_class = match reason.as_str() {
        "like" => "notif__icon notif__icon--like",
        "repost" => "notif__icon notif__icon--repost",
        "follow" => "notif__icon notif__icon--follow",
        _ => "notif__icon",
    };
    let time = first.relative_time();

    rsx! {
        article { class: "{unread_class}",
            div { class: "{icon_color_class}",
                match reason.as_str() {
                    "like" => rsx! { icons::Heart { size: icons::Size::Sm } },
                    "repost" => rsx! { icons::Repeat2 { size: icons::Size::Sm } },
                    "follow" => rsx! { icons::UserPlus { size: icons::Size::Sm } },
                    "mention" => rsx! { icons::AtSign { size: icons::Size::Sm } },
                    "reply" => rsx! { icons::MessageCircle { size: icons::Size::Sm } },
                    "quote" => rsx! { icons::Quote { size: icons::Size::Sm } },
                    "starterpack-joined" => rsx! { icons::Package { size: icons::Size::Sm } },
                    _ => rsx! { icons::Bell { size: icons::Size::Sm } },
                }
            }
            // Avatar block: for singletons it's the one actor; for
            // groups it's a stack of up to three overlapping avatars
            // (newest-first because `group.items` is in input order).
            div { class: "notif__avatars",
                AvatarStack { actors: group.items.iter().take(3).map(|n| n.author.clone()).collect::<Vec<_>>() }
            }
            div { class: "notif__body",
                div { class: "notif__head",
                    NotifPhrase { group: group.clone() }
                    span { class: "notif__time", "{time}" }
                }
                // Subject quote (for likes/reposts of your post, or
                // for the inbound text of a reply/mention/quote).
                if let Some(post) = subject {
                    SubjectQuote { post: post, reason: reason.clone() }
                }
            }
        }
    }
}

/// Header line — "Alice liked your post" for singletons,
/// "Alice, Bob and 18 others liked your post" for groups. Actor names
/// are clickable (open ProfileSheet).
#[component]
fn NotifPhrase(group: NotificationGroup) -> Element {
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let reason = group.reason.clone();
    let count = group.count();
    let verb = match reason.as_str() {
        "like" => "liked your post",
        "repost" => "reposted your post",
        "follow" => "followed you",
        "mention" => "mentioned you",
        "reply" => "replied to your post",
        "quote" => "quoted your post",
        "starterpack-joined" => "joined via your starter pack",
        _ => "interacted with you",
    };

    if count == 1 {
        let actor = group.items[0].author.clone();
        let did_for_click = actor.did.clone();
        let name = actor
            .display_name
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| actor.handle.clone());
        return rsx! {
            span { class: "notif__phrase",
                button { class: "notif__name-link",
                    onclick: move |_| profile_focus.set(ProfileFocus(Some(did_for_click.clone()))),
                    "{name}"
                }
                " {verb}"
            }
        };
    }

    // Grouped — name the first two actors, then "and N others".
    let first = group.items[0].author.clone();
    let second = group.items.get(1).map(|n| n.author.clone());
    let extra = count.saturating_sub(2);

    let first_did = first.did.clone();
    let first_name = first
        .display_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| first.handle.clone());

    let second_did = second.as_ref().map(|s| s.did.clone());
    let second_name = second.as_ref().map(|s| {
        s.display_name
            .clone()
            .filter(|x| !x.is_empty())
            .unwrap_or_else(|| s.handle.clone())
    });

    rsx! {
        span { class: "notif__phrase",
            button { class: "notif__name-link",
                onclick: move |_| profile_focus.set(ProfileFocus(Some(first_did.clone()))),
                "{first_name}"
            }
            if let (Some(sname), Some(sdid)) = (second_name, second_did) {
                if extra == 0 {
                    " and "
                } else {
                    ", "
                }
                button { class: "notif__name-link",
                    onclick: move |_| profile_focus.set(ProfileFocus(Some(sdid.clone()))),
                    "{sname}"
                }
            }
            if extra > 0 {
                " and {extra} other"
                if extra != 1 { "s" }
            }
            " {verb}"
        }
    }
}

/// Stack of up to N avatars, overlapping with slight offsets — same
/// visual idiom as the profile mutuals row.
#[component]
fn AvatarStack(actors: Vec<PostAuthor>) -> Element {
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    rsx! {
        div { class: "notif__avatar-stack",
            for (i, a) in actors.iter().enumerate() {
                {
                    let did = a.did.clone();
                    let on_click = move |_| profile_focus.set(ProfileFocus(Some(did.clone())));
                    if let Some(url) = a.avatar.as_ref() {
                        rsx! {
                            button { key: "{i}",
                                class: "notif__avatar-btn",
                                title: "{a.handle}",
                                onclick: on_click,
                                img { class: "notif__avatar-img", src: "{url}", alt: "{a.handle}" }
                            }
                        }
                    } else {
                        rsx! {
                            button { key: "{i}",
                                class: "notif__avatar-btn notif__avatar-btn--placeholder",
                                title: "{a.handle}",
                                onclick: on_click,
                                icons::User { size: icons::Size::Sm }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The quoted-post block shown under a notification — unchanged from
/// the pre-grouping renderer.
#[component]
fn SubjectQuote(post: PostView, reason: String) -> Element {
    let text = post.record.text.clone();
    let embed = post.embed.clone();
    if text.trim().is_empty() && embed.is_none() {
        return rsx! { Fragment {} };
    }
    let is_inbound = matches!(reason.as_str(), "reply" | "mention" | "quote");
    let class = if is_inbound {
        "notif__quote notif__quote--inbound"
    } else {
        "notif__quote notif__quote--own"
    };
    rsx! {
        div { class: "{class}",
            if !text.is_empty() {
                p { class: "notif__quote-text", "{text}" }
            }
            if let Some(e) = embed {
                div { class: "notif__quote-embed",
                    EmbedView { embed: e }
                }
            }
        }
    }
}
