//! Single post card.

use crate::auth_refresh::fresh_client;
use crate::components::embed::EmbedView;
use crate::icons;
use crate::state::{
    ComposeContext, Engagement, EngagementFocus, OptimisticMap, ProfileFocus, ReplyTarget,
    ThreadFocus,
};
use dioxus::prelude::*;
use smooblue_atproto::feed::PostView;
use smooblue_oauth::Session;

#[component]
pub fn PostCard(post: PostView) -> Element {
    // NOTE: this component does NOT subscribe to the global Tick
    // signal — the relative timestamp ("11s" → "12s") is rendered
    // via icons::TimeAgo, which has its own tick subscription. With
    // a 500-post column, full-card re-renders every second pegged
    // a CPU core; lifting the subscription into the tiny text node
    // dropped steady-state CPU to ~0% (scale=large smoke-tested
    // 2026-05-24).
    let mut optimistic = use_context::<Signal<OptimisticMap>>();
    let mut compose_ctx = use_context::<Signal<ComposeContext>>();
    let mut thread_focus = use_context::<Signal<ThreadFocus>>();
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let mut engagement_focus = use_context::<Signal<EngagementFocus>>();
    let session = use_context::<Signal<Option<Session>>>();

    let post_uri = post.uri.clone();
    let post_cid = post.cid.clone();
    let server_like_uri = post.viewer.as_ref().and_then(|v| v.like.clone());
    let server_repost_uri = post.viewer.as_ref().and_then(|v| v.repost.clone());

    // Combine server state + optimistic intent. Optimistic always wins
    // for `liked`/`reposted` while it has an explicit Some(_) — that's the
    // whole point of the optimistic flip. The next poll cycle reconciles.
    let opt_state = optimistic
        .read()
        .get(&post_uri)
        .cloned()
        .unwrap_or_default();

    let is_liked = match opt_state.liked {
        Some(b) => b,
        None => server_like_uri.is_some(),
    };
    let is_reposted = match opt_state.reposted {
        Some(b) => b,
        None => server_repost_uri.is_some(),
    };

    // Adjust displayed counts to reflect optimistic deltas.
    let server_was_liked = server_like_uri.is_some();
    let like_delta: i64 = match (server_was_liked, opt_state.liked) {
        (false, Some(true)) => 1,
        (true, Some(false)) => -1,
        _ => 0,
    };
    let display_likes = (post.like_count + like_delta).max(0);

    let server_was_reposted = server_repost_uri.is_some();
    let repost_delta: i64 = match (server_was_reposted, opt_state.reposted) {
        (false, Some(true)) => 1,
        (true, Some(false)) => -1,
        _ => 0,
    };
    let display_reposts = (post.repost_count + repost_delta).max(0);

    let name = post.display_name().to_string();
    let handle = post.author.handle.clone();
    let time_initial = post.relative_time();
    let time_source = post
        .indexed_at
        .clone()
        .or_else(|| post.record.created_at.clone());
    let text = post.record.text.clone();
    let avatar = post.author.avatar.clone();
    let embed = post.embed.clone();
    let replies = post.reply_count;
    let quote_count = post.quote_count;
    let actor_did = post.author.did.clone();

    // Click handlers for the tap-the-count modals. Each one stashes
    // the post URI in the EngagementFocus signal — the EngagementSheet
    // (mounted at the deck level) picks it up and runs the right
    // lexicon call. Stop propagation so the click doesn't ALSO open
    // the thread via the wider post-body click target.
    let uri_for_likes = post_uri.clone();
    let open_likes = move |evt: MouseEvent| {
        evt.stop_propagation();
        engagement_focus.set(EngagementFocus(Some(Engagement::Likes(
            uri_for_likes.clone(),
        ))));
    };
    let uri_for_reposters = post_uri.clone();
    let open_reposters = move |evt: MouseEvent| {
        evt.stop_propagation();
        engagement_focus.set(EngagementFocus(Some(Engagement::Reposters(
            uri_for_reposters.clone(),
        ))));
    };
    let uri_for_quotes = post_uri.clone();
    let open_quotes = move |evt: MouseEvent| {
        evt.stop_propagation();
        engagement_focus.set(EngagementFocus(Some(Engagement::Quotes(
            uri_for_quotes.clone(),
        ))));
    };

    // Click an avatar → open the profile sheet (modal with banner +
    // bio + follow + recent posts). The sheet itself has an
    // "Add as column" button if the user wants a permanent column.
    let open_profile = move |_evt: MouseEvent| {
        profile_focus.set(ProfileFocus(Some(actor_did.clone())));
    };

    // Click anywhere on the body (text / timestamp / empty space, but
    // not on the action buttons — they stop_propagation) opens this
    // post in the thread sheet.
    let post_uri_thread = post_uri.clone();
    let open_thread = move |_evt: MouseEvent| {
        thread_focus.set(ThreadFocus(Some(post_uri_thread.clone())));
    };

    // ── Like ────────────────────────────────────────────────────────
    let post_uri_l = post_uri.clone();
    let post_cid_l = post_cid.clone();
    let server_like_uri_l = server_like_uri.clone();
    let opt_state_l = opt_state.clone();
    let mut toggle_like = move |_evt: MouseEvent| {
        // Just need to know we *have* a session here — fresh_client
        // inside the spawn will re-read + refresh if needed.
        if session.read().is_none() {
            return;
        };
        let want_liked = !is_liked;
        let known_like_uri = opt_state_l
            .like_uri
            .clone()
            .or_else(|| server_like_uri_l.clone());
        {
            let mut map = optimistic.write();
            let entry = map.entry(post_uri_l.clone()).or_default();
            entry.liked = Some(want_liked);
            entry.like_uri = known_like_uri.clone();
        }
        let post_uri_owned = post_uri_l.clone();
        let post_cid_owned = post_cid_l.clone();
        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                return;
            };
            if want_liked {
                match client.create_like(&post_uri_owned, &post_cid_owned).await {
                    Ok(rec) => {
                        let mut map = optimistic.write();
                        let entry = map.entry(post_uri_owned.clone()).or_default();
                        entry.liked = Some(true);
                        entry.like_uri = Some(rec.uri);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "smooblue: create_like failed");
                        let mut map = optimistic.write();
                        if let Some(entry) = map.get_mut(&post_uri_owned) {
                            entry.liked = None;
                        }
                    }
                }
            } else if let Some(uri) = known_like_uri {
                match client.delete_record(&uri).await {
                    Ok(_) => {
                        let mut map = optimistic.write();
                        let entry = map.entry(post_uri_owned.clone()).or_default();
                        entry.liked = Some(false);
                        entry.like_uri = None;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "smooblue: delete_record (unlike) failed");
                        let mut map = optimistic.write();
                        if let Some(entry) = map.get_mut(&post_uri_owned) {
                            entry.liked = None;
                        }
                    }
                }
            }
        });
    };

    // ── Repost ─────────────────────────────────────────────────────
    let post_uri_r = post_uri.clone();
    let post_cid_r = post_cid.clone();
    let server_repost_uri_r = server_repost_uri.clone();
    let opt_state_r = opt_state.clone();
    let mut toggle_repost = move |_evt: MouseEvent| {
        if session.read().is_none() {
            return;
        };
        let want_reposted = !is_reposted;
        let known_repost_uri = opt_state_r
            .repost_uri
            .clone()
            .or_else(|| server_repost_uri_r.clone());
        {
            let mut map = optimistic.write();
            let entry = map.entry(post_uri_r.clone()).or_default();
            entry.reposted = Some(want_reposted);
            entry.repost_uri = known_repost_uri.clone();
        }
        let post_uri_owned = post_uri_r.clone();
        let post_cid_owned = post_cid_r.clone();
        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                return;
            };
            if want_reposted {
                match client.create_repost(&post_uri_owned, &post_cid_owned).await {
                    Ok(rec) => {
                        let mut map = optimistic.write();
                        let entry = map.entry(post_uri_owned.clone()).or_default();
                        entry.reposted = Some(true);
                        entry.repost_uri = Some(rec.uri);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "smooblue: create_repost failed");
                        let mut map = optimistic.write();
                        if let Some(entry) = map.get_mut(&post_uri_owned) {
                            entry.reposted = None;
                        }
                    }
                }
            } else if let Some(uri) = known_repost_uri {
                match client.delete_record(&uri).await {
                    Ok(_) => {
                        let mut map = optimistic.write();
                        let entry = map.entry(post_uri_owned.clone()).or_default();
                        entry.reposted = Some(false);
                        entry.repost_uri = None;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "smooblue: delete_record (unrepost) failed");
                        let mut map = optimistic.write();
                        if let Some(entry) = map.get_mut(&post_uri_owned) {
                            entry.reposted = None;
                        }
                    }
                }
            }
        });
    };

    // ── Reply ──────────────────────────────────────────────────────
    // Opens the compose sheet with the parent post in context. The
    // ComposeSheet itself is mounted at the deck level, so reply just
    // needs to set the compose context + flip the open signal there.
    let post_uri_reply = post_uri.clone();
    let post_cid_reply = post_cid.clone();
    let handle_reply = handle.clone();
    let text_reply = text.clone();
    let mut open_reply = move |_evt: MouseEvent| {
        let mut w = compose_ctx.write();
        w.reply_to = Some(ReplyTarget {
            uri: post_uri_reply.clone(),
            cid: post_cid_reply.clone(),
            handle: handle_reply.clone(),
            text: text_reply.clone(),
        });
        w.open = true;
    };

    let like_class = if is_liked {
        "post__action post__action--clickable post__action--liked"
    } else {
        "post__action post__action--clickable"
    };
    let repost_class = if is_reposted {
        "post__action post__action--clickable post__action--reposted"
    } else {
        "post__action post__action--clickable"
    };

    rsx! {
        article { class: "post",
            div { class: "post__avatar post__avatar--clickable",
                onclick: open_profile,
                title: "Open profile column",
                if let Some(url) = avatar {
                    img { loading: "lazy", decoding: "async", src: "{url}", alt: "{handle}" }
                }
            }
            div { class: "post__body",
                div { class: "post__body--clickable",
                    onclick: open_thread,
                    title: "Open thread",
                    div { class: "post__head",
                        span { class: "post__name", "{name}" }
                        span { class: "post__handle", "@{handle}" }
                        span { class: "post__time",
                            icons::TimeAgo { text_at_render: time_initial.clone(), source_ts: time_source.clone() }
                        }
                    }
                    if !text.is_empty() {
                        p { class: "post__text", "{text}" }
                    }
                }
                if let Some(e) = embed {
                    div { class: "post__embed",
                        EmbedView { embed: e }
                    }
                }
                div { class: "post__actions",
                    button { class: "post__action post__action--clickable",
                        onclick: move |evt: MouseEvent| { evt.stop_propagation(); open_reply(evt); },
                        title: "Reply",
                        icons::MessageCircle { size: icons::Size::Sm }
                        span { "{replies}" }
                    }
                    // Repost: icon toggles, count opens the reposters
                    // sheet. Two separate buttons sharing the visual
                    // grouping via CSS.
                    button { class: "{repost_class}",
                        onclick: move |evt: MouseEvent| { evt.stop_propagation(); toggle_repost(evt); },
                        title: if is_reposted { "Undo repost" } else { "Repost" },
                        icons::Repeat2 { size: icons::Size::Sm }
                    }
                    if display_reposts > 0 {
                        button { class: "post__action-count",
                            onclick: open_reposters,
                            title: "See who reposted",
                            "{display_reposts}"
                        }
                    } else {
                        span { class: "post__action-count post__action-count--zero", "0" }
                    }
                    // Quote count — bsky's lexicon exposes this; we
                    // show it inline if any quotes exist. Click opens
                    // the quotes list.
                    if quote_count > 0 {
                        button { class: "post__action post__action--quote",
                            onclick: open_quotes,
                            title: "See who quoted",
                            icons::Quote { size: icons::Size::Sm }
                            span { "{quote_count}" }
                        }
                    }
                    // Like: same split — icon toggles, count opens the likers sheet.
                    button { class: "{like_class}",
                        onclick: move |evt: MouseEvent| { evt.stop_propagation(); toggle_like(evt); },
                        title: if is_liked { "Unlike" } else { "Like" },
                        icons::Heart { size: icons::Size::Sm }
                    }
                    if display_likes > 0 {
                        button { class: "post__action-count",
                            onclick: open_likes,
                            title: "See who liked",
                            "{display_likes}"
                        }
                    } else {
                        span { class: "post__action-count post__action-count--zero", "0" }
                    }
                    span { class: "post__action post__action--right",
                        icons::MoreHorizontal { size: icons::Size::Sm }
                    }
                }
            }
        }
    }
}
