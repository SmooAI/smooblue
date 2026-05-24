//! A single notification card. Uses Lucide icons for the reason glyph.

use crate::icons;
use crate::state::Tick;
use dioxus::prelude::*;
use smooblue_atproto::Notification;

#[component]
pub fn NotificationCard(notif: Notification) -> Element {
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
            }
        }
    }
}
