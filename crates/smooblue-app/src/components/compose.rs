//! Compose sheet — modal text-only post creation.
//!
//! Uses the shared `.modal__backdrop` + `.modal__sheet` chrome from
//! smooai-ui and adds compose-specific extensions on top.
//!
//! In demo mode (or when not signed in), "Post" closes the sheet without
//! actually creating the record. In live mode it calls
//! `AtClient::create_post` (com.atproto.repo.createRecord) against the
//! user's PDS.

use crate::icons;
use dioxus::prelude::*;
use smooblue_atproto::AtClient;
use smooblue_oauth::Session;
use url::Url;

/// Bluesky's hard post length cap (graphemes, but we count chars as a proxy).
pub const MAX_LEN: usize = 300;

#[component]
pub fn ComposeSheet(open: Signal<bool>) -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut text = use_signal(String::new);
    let mut posting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    if !*open.read() {
        return rsx! { Fragment {} };
    }

    let len = text.read().chars().count();
    let remaining = MAX_LEN as i64 - len as i64;
    let over = remaining < 0;
    let empty = text.read().trim().is_empty();

    let post = move |_evt: MouseEvent| {
        if empty || over {
            return;
        }
        let body = text.read().clone();
        let sess = session.read().clone();
        posting.set(true);
        error.set(None);
        let mut open = open;
        let mut posting = posting;
        let mut text = text;
        let mut error = error;
        spawn(async move {
            // Demo / not-signed-in path — simulate the round-trip + close.
            if crate::demo::is_active() || sess.is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                posting.set(false);
                text.set(String::new());
                open.set(false);
                return;
            }
            let s = sess.unwrap();
            // Writes go to the user's PDS (createRecord lives there, not
            // on the AppView). AtClient handles the route internally; we
            // just supply the PDS as the base URL.
            let base = match Url::parse(&s.pds) {
                Ok(u) => u,
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Bad PDS URL: {e}")));
                    return;
                }
            };
            let client = AtClient::new(s, base);
            match client.create_post(&body).await {
                Ok(_record) => {
                    posting.set(false);
                    text.set(String::new());
                    open.set(false);
                }
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Couldn't post: {e}")));
                }
            }
        });
    };

    rsx! {
        div { class: "modal__backdrop", onclick: move |_| open.set(false),
            div { class: "modal__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "compose__head",
                    span { class: "compose__title", "New post" }
                    button { class: "compose__close",
                        title: "Close",
                        onclick: move |_| open.set(false),
                        "✕"
                    }
                }
                textarea {
                    class: "input input--lg compose__textarea",
                    placeholder: "What's up?",
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
                        if *posting.read() { "Posting…" } else { "Post" }
                    }
                }
                if let Some(msg) = &*error.read() {
                    div { class: "compose__error", "{msg}" }
                }
            }
        }
    }
}
