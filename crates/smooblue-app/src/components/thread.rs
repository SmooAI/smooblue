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
//! that post (mutates the same `ThreadFocus` signal). Each hop is
//! recorded in [`crate::history`], so the header's ← / → (and ⌘[ / ⌘])
//! walk back through the replies you clicked, and ⌘⇧T reopens a thread
//! closed by a stray backdrop click — scrolled to where you left it.

use crate::auth_refresh::fresh_client;
use crate::components::post::PostCard;
use crate::demo;
use crate::history::{NavHistory, NavKind};
use crate::icons;
use crate::state::{PostedTick, ProfileFocus, ThreadFocus};
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
    let profile_focus = use_context::<Signal<ProfileFocus>>();
    let nav = use_context::<Signal<NavHistory>>();
    let posted = use_context::<Signal<PostedTick>>();
    let snap = focus.read().0.clone();
    // Closed: render nothing. Hooks below run unconditionally per
    // Dioxus rules, so we put the early-return after them.
    let uri_opt = snap.clone();

    // Reactive: read focus inside the resource so clicking through
    // to a different post inside the thread re-fires the fetch. The
    // PostedTick read makes a reply you just sent from this thread
    // show up without closing and reopening it.
    let thread = use_resource(move || {
        let session_sig = session;
        let uri = focus.read().0.clone();
        let _ = posted.read();
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

    // Remember what was read, for the History sheet.
    let mut recorded = use_signal(|| None::<String>);
    use_effect(move || {
        let Some(Ok(ThreadView::Post { post, .. })) = &*thread.read() else {
            return;
        };
        if recorded.peek().as_deref() == Some(post.uri.as_str()) {
            return;
        }
        recorded.set(Some(post.uri.clone()));
        let author = post
            .author
            .display_name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("@{}", post.author.handle));
        crate::history::record_in_background(
            NavKind::Thread,
            post.uri.clone(),
            author,
            crate::history::snippet(&post.record.text, 160),
        );
    });

    let Some(uri) = uri_opt else {
        return rsx! { Fragment {} };
    };

    let close = move |_| {
        focus.set(ThreadFocus(None));
    };
    let can_back = nav.read().can_go_back();
    let can_forward = nav.read().can_go_forward();
    let go_back = move |_| {
        crate::history::step(nav, focus, profile_focus, false);
    };
    let go_forward = move |_| {
        crate::history::step(nav, focus, profile_focus, true);
    };
    let author_handle = match &*thread.read_unchecked() {
        Some(Ok(ThreadView::Post { post, .. })) if post.uri == uri => {
            Some(post.author.handle.clone())
        }
        _ => None,
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet thread__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "thread__head",
                    button {
                        class: "thread__nav",
                        title: "Back (⌘[)",
                        disabled: !can_back,
                        onclick: go_back,
                        icons::ArrowLeft { size: icons::Size::Sm }
                    }
                    button {
                        class: "thread__nav",
                        title: "Forward (⌘])",
                        disabled: !can_forward,
                        onclick: go_forward,
                        icons::ArrowRight { size: icons::Size::Sm }
                    }
                    span { class: "thread__title",
                        "Thread"
                        if let Some(h) = author_handle {
                            span { class: "thread__title-handle", " · @{h}" }
                        }
                    }
                    button { class: "thread__close",
                        title: "Close (Esc) — ⌘⇧T reopens",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                // Keyed by URI (a one-item keyed list, since rsx only
                // allows keys on list items) so each thread gets a fresh
                // scroll box: reusing one would carry the previous
                // thread's scroll offset over, and the scroll-memory
                // listener would file it under the new URI.
                for body_uri in std::iter::once(uri.clone()) {
                div { key: "{body_uri}",
                    class: "thread__body",
                    "data-uri": "{body_uri}",
                    match &*thread.read_unchecked() {
                        Some(Ok(t)) => rsx! { ThreadBody { thread: t.clone() } },
                        Some(Err(e)) => {
                            if e == "no focus" {
                                rsx! { div { class: "thread__loading", "Loading thread…" } }
                            } else {
                                rsx! { div { class: "thread__error", "Couldn't load thread: {e}" } }
                            }
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
    // Scroll the focused post into view as soon as the row mounts.
    // Without this, opening a thread on a deeply-nested reply lands
    // the scroll position at the top (root post) and the user has
    // to hunt for the post they actually clicked. `Smooth` looks
    // right because there's already a visible content swap on
    // sheet-open — the smooth glide reads as "the app is taking
    // you to the post," not as a janky reflow.
    //
    // Coming BACK to a thread (back button, ⌘⇧T, History) restores the
    // scroll position you left it at instead — that's the whole point
    // of returning to a long thread. Positions are kept by the
    // `THREAD_SCROLL_MEMORY_JS` listener installed in `App`.
    let on_mount = move |_evt: Event<MountedData>| {
        let _ = dioxus::document::eval(RESTORE_OR_FOCUS_JS);
    };
    match node {
        ThreadView::Post { post, .. } => rsx! {
            div {
                class: "thread__focused",
                onmounted: on_mount,
                PostCard { post }
            }
        },
        _ => rsx! {
            div {
                class: "thread__placeholder thread__placeholder--focused",
                onmounted: on_mount,
                "This post is unavailable"
            }
        },
    }
}

/// JS for a freshly-mounted focused post: restore the saved scroll
/// offset for this thread (keyed by the body's `data-uri`) if there is
/// one, else glide the focused post into view. Re-applies the restore
/// once after images above have had a moment to load and shift layout.
const RESTORE_OR_FOCUS_JS: &str = r#"(function() {
    const body = document.querySelector('.thread__body');
    const mem = window.__smoobThreadScroll;
    const saved = body && mem ? mem.get(body.dataset.uri) : undefined;
    if (saved !== undefined && saved > 0) {
        body.scrollTop = saved;
        setTimeout(() => {
            if (Math.abs(body.scrollTop - saved) > 4) body.scrollTop = saved;
        }, 350);
        return true;
    }
    const el = document.querySelector('.thread__focused, .thread__placeholder--focused');
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    return false;
})()"#;

#[component]
fn ReplyTree(node: ThreadView, depth: usize) -> Element {
    if depth >= MAX_VISIBLE_DEPTH {
        // Too deep to indent further: re-root the sheet on this reply
        // so its own subtree gets the full width. (This used to be a
        // dead "Continue thread →" label with no click handler.)
        let target = match &node {
            ThreadView::Post { post, .. } => Some(post.uri.clone()),
            _ => None,
        };
        let mut focus = use_context::<Signal<ThreadFocus>>();
        return rsx! {
            if let Some(uri) = target {
                button { class: "thread__continue",
                    style: "margin-left: {REPLY_INDENT_PX * MAX_VISIBLE_DEPTH as u32}px;",
                    onclick: move |_| focus.set(ThreadFocus(Some(uri.clone()))),
                    "Continue thread →"
                }
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
