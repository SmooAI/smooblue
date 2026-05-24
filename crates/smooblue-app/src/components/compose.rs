//! Compose sheet — modal text-only post creation.
//!
//! Two modes:
//! - *Top-level post* — what the FAB opens.
//! - *Reply* — what the reply icon on a PostCard opens. Same sheet,
//!   shows the parent text as quoted context above the textarea and
//!   posts with a reply ref attached.
//!
//! Uses the shared `.modal__backdrop` + `.modal__sheet` chrome from
//! smooai-ui and adds compose-specific extensions on top.

use crate::icons;
use crate::state::ComposeContext;
use dioxus::prelude::*;
use smooblue_atproto::{AtClient, ReplyRef, StrongRef};
use smooblue_oauth::Session;
use url::Url;

/// Bluesky's hard post length cap (graphemes, but we count chars as a proxy).
pub const MAX_LEN: usize = 300;

#[component]
pub fn ComposeSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut ctx = use_context::<Signal<ComposeContext>>();
    let mut text = use_signal(String::new);
    let mut posting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let snap = ctx.read().clone();
    if !snap.open {
        return rsx! { Fragment {} };
    }

    let reply_to = snap.reply_to.clone();

    let len = text.read().chars().count();
    let remaining = MAX_LEN as i64 - len as i64;
    let over = remaining < 0;
    let empty = text.read().trim().is_empty();

    let close = move |_evt| {
        let mut w = ctx.write();
        w.reply_to = None;
        w.open = false;
    };

    let post = move |_evt: MouseEvent| {
        if empty || over {
            return;
        }
        let body = text.read().clone();
        let sess = session.read().clone();
        let reply = ctx.read().reply_to.as_ref().map(|p| ReplyRef {
            root: StrongRef {
                uri: p.uri.clone(),
                cid: p.cid.clone(),
            },
            parent: StrongRef {
                uri: p.uri.clone(),
                cid: p.cid.clone(),
            },
        });
        posting.set(true);
        error.set(None);
        let mut posting = posting;
        let mut text = text;
        let mut error = error;
        let mut ctx = ctx;
        spawn(async move {
            if crate::demo::is_active() || sess.is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                posting.set(false);
                text.set(String::new());
                let mut w = ctx.write();
                w.reply_to = None;
                w.open = false;
                return;
            }
            let s = sess.unwrap();
            let base = match Url::parse(&s.pds) {
                Ok(u) => u,
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Bad PDS URL: {e}")));
                    return;
                }
            };
            let client = AtClient::new(s, base);
            let result = client
                .create_post_with_reply(&body, reply.as_ref())
                .await;
            match result {
                Ok(_record) => {
                    posting.set(false);
                    text.set(String::new());
                    let mut w = ctx.write();
                    w.reply_to = None;
                    w.open = false;
                }
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Couldn't post: {e}")));
                }
            }
        });
    };

    let placeholder = if reply_to.is_some() {
        "Write your reply…"
    } else {
        "What's up?"
    };
    let title_text = if reply_to.is_some() {
        "Reply"
    } else {
        "New post"
    };
    let button_text = if reply_to.is_some() { "Reply" } else { "Post" };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "compose__head",
                    span { class: "compose__title", "{title_text}" }
                    button { class: "compose__close",
                        title: "Close",
                        onclick: close,
                        "✕"
                    }
                }
                if let Some(parent) = reply_to.as_ref() {
                    div { class: "compose__reply-context",
                        div { class: "compose__reply-author",
                            "Replying to "
                            span { class: "compose__reply-handle", "@{parent.handle}" }
                        }
                        p { class: "compose__reply-text", "{parent.text}" }
                    }
                }
                textarea {
                    class: "input input--lg compose__textarea",
                    placeholder: "{placeholder}",
                    autofocus: true,
                    value: "{text}",
                    oninput: move |e| text.set(e.value()),
                }
                div { class: "compose__bar",
                    span {
                        class: if over { "compose__counter compose__counter--over" } else { "compose__counter" },
                        "{remaining}"
                    }
                    button {
                        class: "btn btn--primary compose__post",
                        disabled: empty || over || *posting.read(),
                        onclick: post,
                        icons::Plus { size: icons::Size::Sm }
                        if *posting.read() { "Posting…" } else { "{button_text}" }
                    }
                }
                if let Some(msg) = &*error.read() {
                    div { class: "compose__error", "{msg}" }
                }
            }
        }
    }
}
