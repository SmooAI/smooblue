//! Single post card.

use crate::icons;
use crate::state::{add_column_unique, ColumnSpec, Tick};
use dioxus::prelude::*;
use smooblue_atproto::feed::PostView;

#[component]
pub fn PostCard(post: PostView) -> Element {
    // Subscribe to the global tick so the relative timestamp re-renders
    // every second ("11s" → "12s" → "1m"). The read itself does the work
    // — Dioxus tracks the signal access as a render dependency.
    let _tick = use_context::<Signal<Tick>>().read().0;
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let name = post.display_name().to_string();
    let handle = post.author.handle.clone();
    let time = post.relative_time();
    let text = post.record.text.clone();
    let avatar = post.author.avatar.clone();
    let thumb = post.first_image_thumb().map(String::from);
    let likes = post.like_count;
    let reposts = post.repost_count;
    let replies = post.reply_count;
    let actor_did = post.author.did.clone();
    let actor_handle = post.author.handle.clone();
    let actor_name = post.display_name().to_string();
    let open_profile = move |_evt: MouseEvent| {
        let title = if actor_name.is_empty() {
            format!("@{}", actor_handle)
        } else {
            actor_name.clone()
        };
        add_column_unique(&mut cols, ColumnSpec::author(actor_did.clone(), title));
    };

    rsx! {
        article { class: "post",
            div { class: "post__avatar post__avatar--clickable",
                onclick: open_profile,
                title: "Open profile column",
                if let Some(url) = avatar {
                    img { src: "{url}", alt: "{handle}" }
                }
            }
            div { class: "post__body",
                div { class: "post__head",
                    span { class: "post__name", "{name}" }
                    span { class: "post__handle", "@{handle}" }
                    span { class: "post__time", "{time}" }
                }
                if !text.is_empty() {
                    p { class: "post__text", "{text}" }
                }
                if let Some(url) = thumb {
                    div { class: "post__embed",
                        img { src: "{url}", alt: "embed" }
                    }
                }
                div { class: "post__actions",
                    span { class: "post__action",
                        icons::MessageCircle { size: icons::Size::Sm }
                        span { "{replies}" }
                    }
                    span { class: "post__action",
                        icons::Repeat2 { size: icons::Size::Sm }
                        span { "{reposts}" }
                    }
                    span { class: "post__action",
                        icons::Heart { size: icons::Size::Sm }
                        span { "{likes}" }
                    }
                    span { class: "post__action post__action--right",
                        icons::MoreHorizontal { size: icons::Size::Sm }
                    }
                }
            }
        }
    }
}
