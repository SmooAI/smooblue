//! Compose sheet — modal text-only post creation.
//!
//! Uses the shared `.modal__backdrop` + `.modal__sheet` chrome from
//! smooai-ui and adds compose-specific extensions on top.
//!
//! In demo mode, "Post" closes the sheet without actually creating the
//! record. In live mode it'll call `AtClient::create_post` (com.atproto.repo.createRecord)
//! once that's wired in `smooblue-atproto`.

use crate::icons;
use dioxus::prelude::*;

/// Bluesky's hard post length cap (graphemes, but we count chars as a proxy).
pub const MAX_LEN: usize = 300;

#[component]
pub fn ComposeSheet(open: Signal<bool>) -> Element {
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
        posting.set(true);
        error.set(None);
        let mut open = open;
        let mut posting = posting;
        let mut text = text;
        let mut error = error;
        spawn(async move {
            // Demo / not-yet-implemented: simulate the network round-trip,
            // then succeed and close. Real createRecord wiring lands with
            // the AtClient::create_post follow-up.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            let _ = body;
            posting.set(false);
            text.set(String::new());
            error.set(None);
            open.set(false);
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
