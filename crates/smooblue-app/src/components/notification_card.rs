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
use crate::state::{ProfileFocus, ThreadFocus};
use dioxus::prelude::*;
use smooblue_atproto::{NotificationGroup, PostAuthor, PostView};

#[component]
pub fn NotificationCard(group: NotificationGroup, subject: Option<PostView>) -> Element {
    // Same pattern as PostCard: relative-time text is rendered by
    // icons::TimeAgo (which has its own tick subscription) so a
    // long Notifications column doesn't full-card re-render once
    // a second. See post.rs for the cost analysis.

    // group.items SHOULD be non-empty by invariant of group_notifications,
    // but render nothing rather than panic if a future refactor violates it.
    let Some(first) = group.items.first().cloned() else {
        return rsx! { Fragment {} };
    };
    let reason = group.reason.clone();
    let unread = group.any_unread();
    let unread_class = if unread {
        "notif notif--unread"
    } else {
        "notif"
    };
    let icon_color_class = match reason.as_str() {
        "like" | "like-via-repost" => "notif__icon notif__icon--like",
        "repost" | "repost-via-repost" => "notif__icon notif__icon--repost",
        "follow" => "notif__icon notif__icon--follow",
        _ => "notif__icon",
    };
    let time_initial = first.relative_time();
    let time_source = first.indexed_at.clone();

    // Which post the user lands on when they click the card.
    // For inbound interactions (reply / mention / quote) the *event*
    // post is the conversation — first.uri. For likes / reposts /
    // starterpack-joined the *subject* post is the one they care
    // about (their own post that just got engagement); we fall back
    // to first.reason_subject which is bsky's lexicon hook for that.
    // Follows don't have a post at all — click target stays empty.
    let mut thread_focus = use_context::<Signal<ThreadFocus>>();
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let click_target: Option<String> = match reason.as_str() {
        "reply" | "mention" | "quote" => Some(first.uri.clone()),
        "like" | "like-via-repost" | "repost" | "repost-via-repost" | "subscribed-post" => {
            first.reason_subject.clone()
        }
        _ => None,
    };
    let click_target_for_profile = first.author.did.clone();
    let click_card = move |_| {
        if let Some(uri) = &click_target {
            thread_focus.set(ThreadFocus(Some(uri.clone())));
        } else {
            // Pure follow / unknown — fall through to opening the
            // actor's profile so the click still does something.
            profile_focus.set(ProfileFocus(Some(click_target_for_profile.clone())));
        }
    };

    rsx! {
        article { class: "{unread_class}",
            onclick: click_card,
            // Head row: reaction icon + avatar stack + phrase + time
            // in a single row at the top. Subject quote (the post
            // you're being notified about) spans the full card
            // width below — same layout deck.blue uses, reclaims
            // the ~50px the old avatar-rail layout reserved.
            //
            // PostCards keep the rail-style layout (avatar column +
            // body) because that's the conventional bsky.app feed
            // look; notifications get this treatment because the
            // payload (the subject quote) needs the extra room.
            div { class: "notif__head-row",
                div { class: "{icon_color_class}",
                    match reason.as_str() {
                        "like" | "like-via-repost" => rsx! { icons::Heart { size: icons::Size::Sm } },
                        "repost" | "repost-via-repost" => rsx! { icons::Repeat2 { size: icons::Size::Sm } },
                        "follow" => rsx! { icons::UserPlus { size: icons::Size::Sm } },
                        "mention" => rsx! { icons::AtSign { size: icons::Size::Sm } },
                        "reply" => rsx! { icons::MessageCircle { size: icons::Size::Sm } },
                        "quote" => rsx! { icons::Quote { size: icons::Size::Sm } },
                        "starterpack-joined" => rsx! { icons::Package { size: icons::Size::Sm } },
                        _ => rsx! { icons::Bell { size: icons::Size::Sm } },
                    }
                }
                div { class: "notif__avatars",
                    AvatarStack { actors: group.items.iter().take(3).map(|n| n.author.clone()).collect::<Vec<_>>() }
                }
                div { class: "notif__phrase-block",
                    NotifPhrase { group: group.clone() }
                }
                span { class: "notif__time",
                    icons::TimeAgo { text_at_render: time_initial.clone(), source_ts: time_source.clone() }
                }
            }
            // Subject quote — full card width.
            if let Some(post) = subject {
                SubjectQuote { post: post, reason: reason.clone() }
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
    let count = group.count();
    // Single source of truth for the verb phrase — delegated to
    // group.reason_phrase() (which itself wraps the canonical
    // Notification::reason_phrase()) so adding a new lexicon reason
    // only requires editing notifications.rs.
    let verb = group.reason_phrase();

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
                    onclick: move |e: MouseEvent| {
                    e.stop_propagation();
                    profile_focus.set(ProfileFocus(Some(did_for_click.clone())));
                },
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
                onclick: move |e: MouseEvent| {
                    e.stop_propagation();
                    profile_focus.set(ProfileFocus(Some(first_did.clone())));
                },
                "{first_name}"
            }
            if let (Some(sname), Some(sdid)) = (second_name, second_did) {
                if extra == 0 {
                    " and "
                } else {
                    ", "
                }
                button { class: "notif__name-link",
                    onclick: move |e: MouseEvent| {
                    e.stop_propagation();
                    profile_focus.set(ProfileFocus(Some(sdid.clone())));
                },
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
                    let on_click = move |e: MouseEvent| {
        e.stop_propagation();
        profile_focus.set(ProfileFocus(Some(did.clone())));
    };
                    if let Some(url) = a.avatar.as_ref() {
                        rsx! {
                            button { key: "{i}",
                                class: "notif__avatar-btn",
                                title: "{a.handle}",
                                onclick: on_click,
                                img { loading: "lazy", decoding: "async", class: "notif__avatar-img", src: "{url}", alt: "{a.handle}" }
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

/// The quoted-post block shown under a notification.
///
/// Inbound reasons (reply / mention / quote) get a full [`PostCard`]
/// so the user can like / repost / quote / reply / open-thread on
/// the post directly from the Notifications column. The orange
/// left-border wrapper stays to mark "this is the inbound thing."
///
/// Outbound reasons (like / repost / starterpack-joined on your OWN
/// post) get the lighter display-only block — actions on your own
/// post in a notification context aren't useful and would clutter
/// the row.
#[component]
fn SubjectQuote(post: PostView, reason: String) -> Element {
    let text = post.record.text.clone();
    let embed = post.embed.clone();
    if text.trim().is_empty() && embed.is_none() {
        return rsx! { Fragment {} };
    }
    let is_inbound = matches!(reason.as_str(), "reply" | "mention" | "quote");
    if is_inbound {
        return rsx! {
            div { class: "notif__quote notif__quote--inbound notif__quote--rich",
                crate::components::post::PostCard { post }
            }
        };
    }
    // For -via-repost reasons the subject isn't the user's OWN
    // post — it's a post they reposted that someone else then
    // interacted with. Show the original author's handle so the
    // user knows whose post they're looking at + a small "from
    // your repost" caption to explain the relationship.
    let is_via_repost = matches!(reason.as_str(), "like-via-repost" | "repost-via-repost");
    let original_handle = if is_via_repost {
        Some(post.author.handle.clone())
    } else {
        None
    };
    rsx! {
        div { class: "notif__quote notif__quote--own",
            if let Some(handle) = original_handle {
                div { class: "notif__quote-caption",
                    "From your repost of "
                    span { class: "notif__quote-handle", "@{handle}" }
                }
            }
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
