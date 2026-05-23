//! Single post card.

use dioxus::prelude::*;
use smooblue_atproto::feed::PostView;

#[component]
pub fn PostCard(post: PostView) -> Element {
    let name = post.display_name().to_string();
    let handle = post.author.handle.clone();
    let time = post.relative_time();
    let text = post.record.text.clone();
    let avatar = post.author.avatar.clone();
    let thumb = post.first_image_thumb().map(String::from);
    let likes = post.like_count;
    let reposts = post.repost_count;
    let replies = post.reply_count;

    rsx! {
        article { class: "post",
            div { class: "post__avatar",
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
                    span { class: "post__action", "💬 {replies}" }
                    span { class: "post__action", "🔁 {reposts}" }
                    span { class: "post__action", "❤ {likes}" }
                }
            }
        }
    }
}
