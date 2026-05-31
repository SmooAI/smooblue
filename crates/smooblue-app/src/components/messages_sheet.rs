//! Inline DM conversation view.
//!
//! Mirrors the [`ThreadSheet`] pattern: opens as a modal sheet when a
//! `ConvoRow` (in column.rs's `ColumnKind::Messages`) sets
//! [`MessagesFocus`] to the convo id. While open, the sheet:
//!
//! 1. Loads message history via `chat.bsky.convo.getMessages`.
//! 2. Marks the convo as read via `chat.bsky.convo.updateRead` on
//!    open (so the column-side unread badge clears next poll).
//! 3. Polls for new messages every [`POLL_SECS`] seconds — Bluesky
//!    chat doesn't push, so this is how "realtime" works for now.
//! 4. Renders bubbles right-aligned for the viewer's own messages,
//!    left-aligned for the other member's, with a thin "(message
//!    deleted)" placeholder for tombstones.
//! 5. Compose input at the bottom — ⌘↵ / Ctrl↵ submits; `chat_send_message`
//!    appends the server-canonical message to the on-screen list
//!    immediately (no wait for the next poll).
//!
//! Send failures surface as a small error strip above the input. We
//! don't optimistically render the outbound message until the server
//! accepts it — bsky's chat service has tight rate limits and the
//! "your message was rejected" path matters more than the latency
//! win from optimistic rendering.

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::MessagesFocus;
use dioxus::prelude::*;
use smooblue_atproto::{Message, MessageInput};
use smooblue_oauth::Session;
use std::time::Duration;

/// How often to refetch messages while the sheet is open. 10s is the
/// sweet spot: fast enough that "did they reply yet?" feels live,
/// slow enough that a tab left open all day doesn't hammer the API.
const POLL_SECS: u64 = 10;

/// Messages-per-page on the initial load. The chat lexicon paginates
/// newest-first; 100 covers most active convos in a single fetch.
const PAGE_LIMIT: u32 = 100;

#[component]
pub fn MessagesSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut focus = use_context::<Signal<MessagesFocus>>();
    let snap = focus.read().0.clone();

    // Local state: the rendered message list + send-in-flight flag
    // + last error. Re-fetched whenever focus changes (use_effect
    // observes focus + tick).
    let mut messages = use_signal::<Vec<Message>>(Vec::new);
    let mut load_error = use_signal::<Option<String>>(|| None);
    let mut send_error = use_signal::<Option<String>>(|| None);
    let mut draft = use_signal(String::new);
    let mut sending = use_signal(|| false);
    // Tick that bumps every POLL_SECS while the sheet is open; the
    // load effect depends on it so the poll triggers a refetch.
    let mut poll_tick = use_signal(|| 0u64);

    // Snapshot the viewer's own DID so message rendering can decide
    // left-vs-right alignment per bubble. Defaults to empty string
    // pre-session (the sheet is gated on open below anyway).
    let me_did = session
        .read()
        .as_ref()
        .map(|s| s.did.clone())
        .unwrap_or_default();

    // Polling tick — only runs while the sheet is open. Stopped by
    // returning early when focus is None on each iteration.
    {
        let focus_for_poll = focus;
        use_future(move || async move {
            loop {
                tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
                if focus_for_poll.read().0.is_none() {
                    continue;
                }
                poll_tick.with_mut(|t| *t = t.wrapping_add(1));
            }
        });
    }

    // Load / refresh messages when (a) focus changes, (b) poll tick
    // fires. Also marks the convo as read on first load.
    {
        let session_for_load = session;
        let focus_for_load = focus;
        use_effect(move || {
            let convo_id = focus_for_load.read().0.clone();
            let _tick = *poll_tick.read();
            let Some(convo_id) = convo_id else {
                // Sheet closed — clear local state so reopening a
                // different convo doesn't flash stale messages.
                messages.set(Vec::new());
                load_error.set(None);
                send_error.set(None);
                draft.set(String::new());
                return;
            };
            spawn(async move {
                let Some(client) = fresh_client(session_for_load).await else {
                    load_error.set(Some("not signed in".into()));
                    return;
                };
                match client.chat_get_messages(&convo_id, None, PAGE_LIMIT).await {
                    Ok(resp) => {
                        // Server returns newest-first; reverse so the
                        // bubble list reads oldest→newest top-to-bottom
                        // and the latest message lands at the bottom.
                        let mut rev = resp.messages;
                        rev.reverse();
                        messages.set(rev);
                        load_error.set(None);
                    }
                    Err(e) => load_error.set(Some(e.to_string())),
                }
                // Mark-as-read in the background — failure here is
                // cosmetic (unread badge takes longer to clear).
                let convo_for_read = convo_id.clone();
                spawn(async move {
                    let Some(client) = fresh_client(session_for_load).await else {
                        return;
                    };
                    let _ = client.chat_update_read(&convo_for_read, None).await;
                });
            });
        });
    }

    // Closed: render nothing. Hooks above must run unconditionally
    // (Dioxus rule), so the early-return goes here.
    if snap.is_none() {
        return rsx! { Fragment {} };
    }

    let close = move |_| focus.set(MessagesFocus(None));

    // Submit handler. Hoisted so we can call it from both the button
    // click and the ⌘↵ keyboard handler without duplicating the
    // spawn+state plumbing.
    let convo_id_for_send = snap.clone();
    let do_send = move || {
        let Some(convo_id) = convo_id_for_send.clone() else {
            return;
        };
        if *sending.read() {
            return;
        }
        let text = draft.read().trim().to_string();
        if text.is_empty() {
            return;
        }
        sending.set(true);
        send_error.set(None);
        let session_for_send = session;
        spawn(async move {
            let Some(client) = fresh_client(session_for_send).await else {
                send_error.set(Some("not signed in".into()));
                sending.set(false);
                return;
            };
            let input = MessageInput {
                text,
                facets: None,
                embed: None,
            };
            match client.chat_send_message(&convo_id, &input).await {
                Ok(view) => {
                    // Append the canonical server message + clear
                    // the draft. The next poll will dedupe (it'll
                    // see the same id) — we use the same MessageView
                    // shape here so the bubble renders identically.
                    messages.write().push(Message::Live(view));
                    draft.set(String::new());
                    send_error.set(None);
                }
                Err(e) => send_error.set(Some(e.to_string())),
            }
            sending.set(false);
        });
    };

    // Spawn-friendly clones so the button + textarea handlers each
    // get their own callable.
    let mut do_send_button = do_send.clone();
    let on_send_click = move |_| do_send_button();
    let mut do_send_kbd = do_send;
    let on_keydown = move |e: KeyboardEvent| {
        let cmd = e.modifiers().meta() || e.modifiers().ctrl();
        if cmd && e.key() == Key::Enter {
            do_send_kbd();
        }
    };

    let send_disabled = *sending.read() || draft.read().trim().is_empty();
    let messages_snap = messages.read().clone();
    let load_err_snap = load_error.read().clone();
    let send_err_snap = send_error.read().clone();

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet messages__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "messages__head",
                    span { class: "messages__title", "Messages" }
                    button { class: "messages__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "messages__body",
                    if let Some(e) = load_err_snap.as_ref() {
                        div { class: "messages__error", "Couldn't load: {e}" }
                    } else if messages_snap.is_empty() {
                        div { class: "messages__empty", "No messages yet." }
                    } else {
                        for (i, msg) in messages_snap.iter().enumerate() {
                            MessageBubble {
                                key: "{message_key(msg, i)}",
                                msg: msg.clone(),
                                me: me_did.clone(),
                            }
                        }
                    }
                }
                if let Some(e) = send_err_snap.as_ref() {
                    div { class: "messages__send-error", "Send failed: {e}" }
                }
                div { class: "messages__compose",
                    textarea {
                        class: "input messages__input",
                        placeholder: "Write a message…",
                        value: "{draft.read()}",
                        oninput: move |e| draft.set(e.value()),
                        onkeydown: on_keydown,
                    }
                    button {
                        class: "btn btn--primary messages__send",
                        disabled: send_disabled,
                        onclick: on_send_click,
                        if *sending.read() { "Sending…" } else { "Send" }
                    }
                }
            }
        }
    }
}

/// Stable key for a message in the rendered list. Server message ids
/// are globally unique within a convo; combine with the index as a
/// belt-and-suspenders against a server quirk that might repeat ids
/// across a paginated batch.
fn message_key(msg: &Message, idx: usize) -> String {
    match msg {
        Message::Live(v) => format!("{}-{idx}", v.id),
        Message::Deleted(d) => format!("{}-{idx}", d.id),
    }
}

#[component]
fn MessageBubble(msg: Message, me: String) -> Element {
    match msg {
        Message::Live(v) => {
            let is_me = v.sender.did == me;
            let bubble_class = if is_me {
                "messages__bubble messages__bubble--me"
            } else {
                "messages__bubble messages__bubble--other"
            };
            // Format the timestamp as just the time (HH:MM) — the
            // header strip groups them by day in a future polish pass.
            let time = format_short_time(&v.sent_at);
            rsx! {
                div { class: "messages__row",
                    div { class: "{bubble_class}",
                        div { class: "messages__text", "{v.text}" }
                        if let Some(t) = time {
                            div { class: "messages__time", "{t}" }
                        }
                    }
                }
            }
        }
        Message::Deleted(_) => rsx! {
            div { class: "messages__row",
                div { class: "messages__bubble messages__bubble--deleted",
                    "(message deleted)"
                }
            }
        },
    }
}

/// Render an ISO-8601 timestamp as `HH:MM` in the user's locale, or
/// `None` if it doesn't parse. Best-effort — we don't want a malformed
/// server timestamp to take the bubble down.
fn format_short_time(iso: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    let local = parsed.with_timezone(&chrono::Local);
    Some(local.format("%H:%M").to_string())
}
