//! Single post card.

use crate::icons;
use crate::state::{add_column_unique, ColumnSpec, OptimisticMap, OptimisticPostState, Tick};
use dioxus::prelude::*;
use smooblue_atproto::feed::PostView;
use smooblue_atproto::AtClient;
use smooblue_oauth::Session;
use url::Url;

#[component]
pub fn PostCard(post: PostView) -> Element {
    // Subscribe to the global tick so the relative timestamp re-renders
    // every second ("11s" → "12s" → "1m"). The read itself does the work
    // — Dioxus tracks the signal access as a render dependency.
    let _tick = use_context::<Signal<Tick>>().read().0;
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let mut optimistic = use_context::<Signal<OptimisticMap>>();
    let session = use_context::<Signal<Option<Session>>>();

    let post_uri = post.uri.clone();
    let post_cid = post.cid.clone();
    let server_like_uri = post.viewer.as_ref().and_then(|v| v.like.clone());

    // Combine server state + optimistic intent. Optimistic always wins
    // for `liked` while it has an explicit Some(_) — that's the whole
    // point of the optimistic flip. When the next poll catches up, the
    // optimistic entry can be cleared.
    let opt_state = optimistic.read().get(&post_uri).cloned().unwrap_or_default();
    let is_liked = match opt_state.liked {
        Some(b) => b,
        None => server_like_uri.is_some(),
    };
    // Adjust the displayed count to reflect any optimistic delta.
    let server_was_liked = server_like_uri.is_some();
    let count_delta: i64 = match (server_was_liked, opt_state.liked) {
        (false, Some(true)) => 1,
        (true, Some(false)) => -1,
        _ => 0,
    };
    let display_likes = (post.like_count + count_delta).max(0);

    let name = post.display_name().to_string();
    let handle = post.author.handle.clone();
    let time = post.relative_time();
    let text = post.record.text.clone();
    let avatar = post.author.avatar.clone();
    let thumb = post.first_image_thumb().map(String::from);
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

    let toggle_like = move |_evt: MouseEvent| {
        let Some(sess) = session.read().clone() else { return };
        // Compute what we want the new state to be, optimistically.
        let want_liked = !is_liked;
        let known_like_uri = opt_state.like_uri.clone().or_else(|| server_like_uri.clone());

        // Flip locally first.
        {
            let mut map = optimistic.write();
            let entry = map.entry(post_uri.clone()).or_default();
            entry.liked = Some(want_liked);
            // We don't know the new like_uri yet (server hasn't replied);
            // keep whatever we knew so the un-like path still has a URI.
            entry.like_uri = known_like_uri.clone();
        }

        let post_uri_owned = post_uri.clone();
        let post_cid_owned = post_cid.clone();
        spawn(async move {
            let base = match Url::parse(&sess.pds) {
                Ok(u) => u,
                Err(_) => return,
            };
            let client = AtClient::new(sess, base);
            if want_liked {
                match client.create_like(&post_uri_owned, &post_cid_owned).await {
                    Ok(rec) => {
                        let mut map = optimistic.write();
                        map.insert(
                            post_uri_owned.clone(),
                            OptimisticPostState {
                                liked: Some(true),
                                like_uri: Some(rec.uri),
                            },
                        );
                    }
                    Err(e) => {
                        // Revert.
                        tracing::warn!(error = %e, "smooblue: create_like failed");
                        let mut map = optimistic.write();
                        map.remove(&post_uri_owned);
                    }
                }
            } else if let Some(uri) = known_like_uri {
                match client.delete_record(&uri).await {
                    Ok(_) => {
                        let mut map = optimistic.write();
                        map.insert(
                            post_uri_owned.clone(),
                            OptimisticPostState {
                                liked: Some(false),
                                like_uri: None,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "smooblue: delete_record (unlike) failed");
                        let mut map = optimistic.write();
                        map.remove(&post_uri_owned);
                    }
                }
            }
        });
    };

    let like_class = if is_liked {
        "post__action post__action--clickable post__action--liked"
    } else {
        "post__action post__action--clickable"
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
                    button { class: "{like_class}", onclick: toggle_like,
                        title: if is_liked { "Unlike" } else { "Like" },
                        icons::Heart { size: icons::Size::Sm }
                        span { "{display_likes}" }
                    }
                    span { class: "post__action post__action--right",
                        icons::MoreHorizontal { size: icons::Size::Sm }
                    }
                }
            }
        }
    }
}
