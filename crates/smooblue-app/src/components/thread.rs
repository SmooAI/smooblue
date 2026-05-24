//! Thread / post-detail view.
//!
//! Opens as a modal sheet (matching the ComposeSheet pattern) when the
//! user clicks a post in any column. Loads `app.bsky.feed.getPostThread`
//! and renders:
//!
//! 1. **Parent chain** — ancestors of the focused post, ordered top→down
//!    (root first). Renders as smaller cards stacked above the focus,
//!    each with a connecting indent rail.
//! 2. **Focused post** — the one the user clicked, full-width and
//!    highlighted with a smoo-orange left border.
//! 3. **Replies tree** — descendants. Each level indents and shows a
//!    rail; we cap visual depth at 5 levels and collapse anything
//!    deeper into a "continue thread →" affordance.
//!
//! Posts inside the thread are real PostCard instances, so likes /
//! reposts / replies / avatar-click-opens-profile all work the same as
//! in feed columns. Clicking a post inside the thread re-focuses to
//! that post (mutates the same `ThreadFocus` signal).

use crate::auth_refresh::fresh_client;
use crate::components::post::PostCard;
use crate::demo;
use crate::icons;
use crate::state::ThreadFocus;
use dioxus::prelude::*;
use smooblue_atproto::ThreadView;
use smooblue_oauth::Session;

/// Indent (px) per reply depth — cumulative left-padding on the
/// replies tree.
const REPLY_INDENT_PX: u32 = 14;
/// Hard cap on visible reply depth. Anything deeper collapses into
/// a "continue thread" link rather than running off-screen.
const MAX_VISIBLE_DEPTH: usize = 5;
/// How many ancestors to ask for from the AppView. The lexicon caps
/// this at 1000; 80 covers any thread the user is likely to want to
/// read end-to-end.
const PARENT_HEIGHT: u32 = 80;
/// Replies depth we ask for. Bluesky's UI defaults to 6.
const DEPTH: u32 = 6;

#[component]
pub fn ThreadSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut focus = use_context::<Signal<ThreadFocus>>();
    let snap = focus.read().0.clone();
    // Closed: render nothing. Hooks below run unconditionally per
    // Dioxus rules, so we put the early-return after them.
    let uri_opt = snap.clone();

    // use_resource keys on the focused URI — clicking a different
    // post inside the thread re-fires the fetch automatically.
    let key = uri_opt.clone();
    let thread = use_resource(move || {
        let uri = key.clone();
        let session_sig = session;
        async move {
            let Some(uri) = uri else {
                return Err::<ThreadView, String>("no focus".into());
            };
            if demo::is_active() {
                return Ok(demo::thread_for(&uri));
            }
            let Some(client) = fresh_client(session_sig).await else {
                return Err("not signed in".into());
            };
            client
                .get_post_thread(&uri, DEPTH, PARENT_HEIGHT)
                .await
                .map_err(|e| e.to_string())
        }
    });

    if uri_opt.is_none() {
        return rsx! { Fragment {} };
    }

    let close = move |_| {
        focus.set(ThreadFocus(None));
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet thread__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "thread__head",
                    span { class: "thread__title", "Thread" }
                    button { class: "thread__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "thread__body",
                    match &*thread.read_unchecked() {
                        Some(Ok(t)) => rsx! { ThreadBody { thread: t.clone() } },
                        Some(Err(e)) => rsx! {
                            div { class: "thread__error", "Couldn't load thread: {e}" }
                        },
                        None => rsx! {
                            div { class: "thread__loading", "Loading thread…" }
                        },
                    }
                }
            }
        }
    }
}

/// Render parents (top→down) + focused post + replies tree.
#[component]
fn ThreadBody(thread: ThreadView) -> Element {
    let parents = thread.parent_chain();
    // The chain comes out closest-first; reverse to put root at the top.
    let mut parents_topdown: Vec<&ThreadView> = parents.into_iter().collect();
    parents_topdown.reverse();

    rsx! {
        // Ancestors above the focused post.
        for (i, anc) in parents_topdown.iter().enumerate() {
            ParentRow { key: "{i}", node: (*anc).clone() }
        }
        // Focused post — highlighted.
        FocusedRow { node: thread.clone() }
        // Replies tree.
        if let ThreadView::Post { replies, .. } = &thread {
            if let Some(rs) = replies {
                for (i, r) in rs.iter().enumerate() {
                    ReplyTree { key: "{i}", node: r.clone(), depth: 0 }
                }
            }
        }
    }
}

#[component]
fn ParentRow(node: ThreadView) -> Element {
    match node {
        ThreadView::Post { post, .. } => rsx! {
            div { class: "thread__parent",
                div { class: "thread__rail" }
                div { class: "thread__parent-card",
                    PostCard { post }
                }
            }
        },
        ThreadView::NotFound { .. } | ThreadView::Blocked { .. } | ThreadView::Other => rsx! {
            div { class: "thread__parent",
                div { class: "thread__rail" }
                div { class: "thread__placeholder", "Parent post unavailable" }
            }
        },
    }
}

#[component]
fn FocusedRow(node: ThreadView) -> Element {
    match node {
        ThreadView::Post { post, .. } => rsx! {
            div { class: "thread__focused",
                PostCard { post }
            }
        },
        _ => rsx! {
            div { class: "thread__placeholder thread__placeholder--focused",
                "This post is unavailable"
            }
        },
    }
}

#[component]
fn ReplyTree(node: ThreadView, depth: usize) -> Element {
    if depth >= MAX_VISIBLE_DEPTH {
        return rsx! {
            div { class: "thread__continue",
                style: "margin-left: {REPLY_INDENT_PX * MAX_VISIBLE_DEPTH as u32}px;",
                "Continue thread →"
            }
        };
    }
    let indent = REPLY_INDENT_PX * depth as u32;
    match node {
        ThreadView::Post { post, replies, .. } => {
            let post_for_card = post.clone();
            let post_for_focus = post.clone();
            let mut focus = use_context::<Signal<ThreadFocus>>();
            // Wrapping the reply card in a clickable shell — click
            // re-focuses the thread on THIS reply. Inner action
            // buttons stop_propagation, so likes/replies still work.
            let refocus = move |_| {
                focus.set(ThreadFocus(Some(post_for_focus.uri.clone())));
            };
            rsx! {
                div { class: "thread__reply",
                    style: "margin-left: {indent}px;",
                    div { class: "thread__rail" }
                    div { class: "thread__reply-card",
                        onclick: refocus,
                        PostCard { post: post_for_card }
                    }
                }
                if let Some(rs) = replies {
                    for (i, r) in rs.iter().enumerate() {
                        ReplyTree { key: "{i}", node: r.clone(), depth: depth + 1 }
                    }
                }
            }
        }
        ThreadView::NotFound { .. } | ThreadView::Blocked { .. } | ThreadView::Other => rsx! {
            div { class: "thread__placeholder",
                style: "margin-left: {indent}px;",
                "Reply unavailable"
            }
        },
    }
}
