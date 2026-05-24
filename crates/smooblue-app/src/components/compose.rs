//! Compose sheet — modal post / reply composition.
//!
//! Two modes:
//! - *Top-level post* — what the FAB opens.
//! - *Reply* — opens via the reply icon on a PostCard. Same sheet,
//!   shows the parent text as quoted context above the textarea and
//!   submits with a reply ref attached.
//!
//! UX niceties beyond a bare textarea:
//! - **Progress ring** counter around the remaining-chars number.
//!   Goes from teal → orange → red as the post approaches the 300
//!   limit. Tabular-numeric digits so the number doesn't jitter.
//! - **⌘↵ / Ctrl↵** submits without leaving the textarea.
//! - **Draft persistence** — the in-progress text survives closing
//!   the sheet, only clearing on successful submit.
//! - Bigger textarea + smoo-orange focus ring (in CSS).

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

    // Submit flow (shared by button click + ⌘↵ keyboard shortcut).
    let do_submit = move || {
        if text.read().trim().is_empty() {
            return;
        }
        let len_now = text.read().chars().count();
        if len_now > MAX_LEN {
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
            let result = client.create_post_with_reply(&body, reply.as_ref()).await;
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

    let mut do_submit_btn = do_submit;
    let mut do_submit_kbd = do_submit;

    let close = move |_evt| {
        let mut w = ctx.write();
        w.reply_to = None;
        w.open = false;
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

    let textarea_class = if over {
        "input input--lg compose__textarea compose__textarea--over"
    } else {
        "input input--lg compose__textarea"
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet compose__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "compose__head",
                    span { class: "compose__title", "{title_text}" }
                    button { class: "compose__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
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
                    class: "{textarea_class}",
                    placeholder: "{placeholder}",
                    autofocus: true,
                    value: "{text}",
                    oninput: move |e| text.set(e.value()),
                    onkeydown: move |e| {
                        // ⌘↵ on macOS, Ctrl↵ on Linux/Win — submit without
                        // having to leave the textarea.
                        if e.key() == Key::Enter && (e.modifiers().meta() || e.modifiers().ctrl()) {
                            do_submit_kbd();
                        }
                    },
                }
                div { class: "compose__bar",
                    ProgressRing { used: len, max: MAX_LEN }
                    span {
                        class: if over { "compose__counter compose__counter--over" } else { "compose__counter" },
                        "{remaining}"
                    }
                    span { class: "compose__hint",
                        if cfg!(target_os = "macos") { "⌘↵" } else { "Ctrl↵" }
                        " to post"
                    }
                    button {
                        class: "btn btn--primary compose__post",
                        disabled: empty || over || *posting.read(),
                        onclick: move |_| do_submit_btn(),
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

/// SVG progress ring for the character counter. As `used` approaches
/// `max`, the ring fills and shifts hue from teal → orange → red.
#[component]
fn ProgressRing(used: usize, max: usize) -> Element {
    // Circle geometry — kept small so it sits inline with the counter text.
    const R: f32 = 9.0;
    const STROKE: f32 = 2.2;
    let cx = R + STROKE;
    let circumference = 2.0 * std::f32::consts::PI * R;

    let ratio = (used as f32 / max as f32).min(1.5);
    let filled = (circumference * ratio.min(1.0)).min(circumference);
    let dash = format!("{filled} {circumference}");

    // Color stops:
    //   <80%  teal       (calm)
    //   80-93 orange     (approaching limit)
    //   93+   red        (last 20 chars)
    //   >100  red        (over)
    let stroke = if ratio >= 1.0 {
        "var(--color-smooai-red)"
    } else if ratio >= 0.93 {
        "var(--color-smooai-red)"
    } else if ratio >= 0.80 {
        "var(--color-smooai-orange)"
    } else {
        "var(--color-smooai-teal, #00a6a6)"
    };

    let size = (R + STROKE) * 2.0;
    rsx! {
        svg {
            class: "compose__ring",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 {size} {size}",
            // Track
            circle {
                cx: "{cx}",
                cy: "{cx}",
                r: "{R}",
                fill: "none",
                stroke: "var(--border)",
                stroke_width: "{STROKE}",
            }
            // Filled portion — rotate -90deg so 0% starts at the top.
            circle {
                cx: "{cx}",
                cy: "{cx}",
                r: "{R}",
                fill: "none",
                stroke: "{stroke}",
                stroke_width: "{STROKE}",
                stroke_linecap: "round",
                stroke_dasharray: "{dash}",
                transform: "rotate(-90 {cx} {cx})",
            }
        }
    }
}
