//! A single notification card. Uses Lucide icons for the reason glyph.
//!
//! When a `subject` is provided, the card shows the post that gives
//! the notification context — e.g. for "Alice liked your post", the
//! subject is YOUR post (so you can see WHICH post was liked). For
//! replies and mentions, the subject is THEIR post (so you can read
//! what they said).

use crate::icons;
use crate::state::Tick;
use dioxus::prelude::*;
use smooblue_atproto::{Notification, PostView};

#[component]
pub fn NotificationCard(notif: Notification, subject: Option<PostView>) -> Element {
    // Subscribe to the global tick so `relative_time()` text refreshes.
    let _tick = use_context::<Signal<Tick>>().read().0;
    let name = notif
        .author
        .display_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| notif.author.handle.clone());
    let handle = notif.author.handle.clone();
    let time = notif.relative_time();
    let avatar = notif.author.avatar.clone();
    let phrase = notif.reason_phrase();
    let reason = notif.reason.clone();
    let unread_class = if notif.is_read {
        "notif"
    } else {
        "notif notif--unread"
    };
    let icon_color_class = match reason.as_str() {
        "like" => "notif__icon notif__icon--like",
        "repost" => "notif__icon notif__icon--repost",
        "follow" => "notif__icon notif__icon--follow",
        _ => "notif__icon",
    };

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
            div { class: "notif__avatar",
                if let Some(url) = avatar {
                    img { src: "{url}", alt: "{handle}" }
                }
            }
            div { class: "notif__body",
                div { class: "notif__head",
                    span { class: "notif__name", "{name}" }
                    span { class: "notif__time", "{time}" }
                }
                p { class: "notif__phrase", "{phrase}" }
                if let Some(post) = subject {
                    SubjectQuote { post: post, reason: reason.clone() }
                }
            }
        }
    }
}

/// The quoted-post block shown under a notification.
///
/// Visual hierarchy mirrors the reason: for "like" / "repost" we're
/// echoing YOUR post (muted, no avatar — you know it's yours); for
/// "reply" / "mention" / "quote" we're showing THEIR post text (with
/// a thin orange left border to mark it as inbound).
#[component]
fn SubjectQuote(post: PostView, reason: String) -> Element {
    let text = post.record.text.clone();
    if text.trim().is_empty() {
        // Image-only or otherwise text-less post — skip rendering so
        // we don't show an empty quote block. (Rich-media renderer
        // will fill this in via a follow-up pearl.)
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
            p { class: "notif__quote-text", "{text}" }
        }
    }
}
