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
use dioxus::document;
use dioxus::prelude::*;
use smooblue_atproto::{ChatProfile, Message, MessageInput};
use smooblue_oauth::Session;
use std::time::Duration;

/// How often to refetch messages while the sheet is open. 10s is the
/// sweet spot: fast enough that "did they reply yet?" feels live,
/// slow enough that a tab left open all day doesn't hammer the API.
const POLL_SECS: u64 = 10;

/// Messages-per-page on the initial load. The chat lexicon paginates
/// newest-first; 100 covers most active convos in a single fetch.
const PAGE_LIMIT: u32 = 100;

/// Scroll-distance from the top (px) at which we trigger an older-
/// messages fetch. 150 gives the user a moment to actually see they
/// triggered something; lower would fire only after a flick, higher
/// would fire while they were still well inside the visible window.
const SCROLL_TRIGGER_PX: i32 = 150;

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
    // The other-party profile (avatar / display name / handle) for
    // the header strip. Filled on first load via chat_get_convo —
    // its members[] includes us + the partner; we strip us out.
    let mut partner = use_signal::<Option<ChatProfile>>(|| None);
    // Tick that bumps every POLL_SECS while the sheet is open; the
    // load effect depends on it so the poll triggers a refetch.
    let mut poll_tick = use_signal(|| 0u64);
    // Scroll-based "load older" state. After the initial top-page
    // fetch, the server returns a cursor pointing at the NEXT page
    // of older messages. When the user scrolls close to the top of
    // the bubble list, we fire chat_get_messages with that cursor
    // and prepend the result. `loading_older` gates against
    // concurrent fetches (scroll events fire many times per second);
    // `no_more_older` latches when the server returns no cursor.
    let mut older_cursor = use_signal::<Option<String>>(|| None);
    let mut loading_older = use_signal(|| false);
    let mut no_more_older = use_signal(|| false);
    // Counter that increments each time `messages` changes via append
    // (new message arrives) — the auto-scroll-to-bottom effect
    // observes it so the view sticks to the latest message after a
    // send or a poll-discovered new arrival. Distinct from a prepend
    // (older-message load), which must NOT scroll the view.
    let mut tail_grew = use_signal(|| 0u64);

    // Snapshot the viewer's own DID so message rendering can decide
    // left-vs-right alignment per bubble. Defaults to empty string
    // pre-session (the sheet is gated on open below anyway).
    let me_did = session
        .read()
        .as_ref()
        .map(|s| s.did.clone())
        .unwrap_or_default();

    // Polling tick — only bumps while the sheet is open. Skips the
    // bump (but keeps sleeping) when focus is None so the resource
    // doesn't churn on a closed sheet. Pearl: adversarial-review
    // found that an unconditional `continue` here was a perpetual
    // 10s wakeup; the gate is fine, the skip-the-bump path is cheap.
    {
        let focus_for_poll = focus;
        use_future(move || async move {
            loop {
                tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
                if focus_for_poll.read().0.is_some() {
                    poll_tick.with_mut(|t| *t = t.wrapping_add(1));
                }
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
                older_cursor.set(None);
                loading_older.set(false);
                no_more_older.set(false);
                partner.set(None);
                return;
            };
            // Convo-switch race guard (adversarial-review P1): the
            // user can click convo A, then convo B, while A's fetch
            // is still in flight. Capture the id we fired for; on
            // landing, abort if focus has since moved elsewhere.
            // Without this, A's response can land after B's and
            // overwrite B's bubble list with A's messages.
            let convo_id_for_check = convo_id.clone();
            spawn(async move {
                let Some(client) = fresh_client(session_for_load).await else {
                    if focus_for_load.peek().0.as_deref() == Some(&convo_id_for_check) {
                        load_error.set(Some("not signed in".into()));
                    }
                    return;
                };
                match client.chat_get_messages(&convo_id, None, PAGE_LIMIT).await {
                    Ok(resp) => {
                        // Stale-result guard. If the user has moved
                        // to a different convo while we were awaiting,
                        // drop this entire result silently — the
                        // current-focus fetch is already in flight
                        // from the next use_effect run.
                        if focus_for_load.peek().0.as_deref() != Some(&convo_id_for_check) {
                            return;
                        }
                        // Server returns newest-first; reverse so the
                        // bubble list reads oldest→newest top-to-bottom
                        // and the latest message lands at the bottom.
                        let mut rev = resp.messages;
                        rev.reverse();
                        let cursor_for_older = resp.cursor.clone();
                        let cursor_was_none = cursor_for_older.is_none();
                        messages.set(rev);
                        older_cursor.set(cursor_for_older);
                        no_more_older.set(cursor_was_none);
                        tail_grew.with_mut(|n| *n = n.wrapping_add(1));
                        load_error.set(None);
                    }
                    Err(e) => {
                        if focus_for_load.peek().0.as_deref() == Some(&convo_id_for_check) {
                            load_error.set(Some(e.to_string()));
                        }
                    }
                }
                // Partner profile lookup — once per convo open
                // (skip if we already have it). Side-fetch via
                // chat_get_convo whose `members` includes both
                // parties; pick the first member whose DID != ours.
                if partner.peek().is_none() {
                    let convo_for_partner = convo_id.clone();
                    let me_for_partner = session_for_load
                        .peek()
                        .as_ref()
                        .map(|s| s.did.clone())
                        .unwrap_or_default();
                    spawn(async move {
                        let Some(client) = fresh_client(session_for_load).await else {
                            return;
                        };
                        if let Ok(resp) = client.chat_get_convo(&convo_for_partner).await {
                            let other = resp
                                .convo
                                .members
                                .into_iter()
                                .find(|m| m.did != me_for_partner);
                            if let Some(p) = other {
                                partner.set(Some(p));
                            }
                        }
                    });
                }
                // Mark-as-read in the background — also stale-guarded
                // so we don't update_read for a convo the user left.
                let convo_for_read = convo_id.clone();
                spawn(async move {
                    if focus_for_load.peek().0.as_deref() != Some(&convo_for_read) {
                        return;
                    }
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
                    tail_grew.with_mut(|n| *n = n.wrapping_add(1));
                }
                Err(e) => send_error.set(Some(e.to_string())),
            }
            sending.set(false);
        });
    };

    // Scroll handler — when the user scrolls within SCROLL_TRIGGER_PX
    // of the top of the bubble list, fetch the next older-message
    // page and prepend. Inlined (rather than dispatching to a
    // separate closure) because both the eval-scrollTop call and the
    // load-older fetch are async and need to share the same captured
    // state without juggling FnMut clones.
    //
    // Self-guards against the per-frame fire rate of scroll events:
    // `loading_older` latches once a fetch is in flight, so the next
    // ~60 evals/sec all bail at the gate check.
    let convo_id_for_scroll = snap.clone();
    let on_scroll = move |_: ScrollEvent| {
        let convo_id = match convo_id_for_scroll.clone() {
            Some(c) => c,
            None => return,
        };
        if *loading_older.read() || *no_more_older.read() {
            return;
        }
        let cursor = match older_cursor.read().clone() {
            Some(c) => c,
            None => return,
        };
        let session_for_older = session;
        spawn(async move {
            // 1. Ask the webview where the scroll position is. dioxus
            //    eval returns a JSON value; we want f64.
            let mut eval = document::eval(
                r#"
                const el = document.querySelector('.messages__body');
                dioxus.send(el ? el.scrollTop : 9999);
                "#,
            );
            let Ok(top) = eval.recv::<f64>().await else {
                return;
            };
            if top >= SCROLL_TRIGGER_PX as f64 {
                return;
            }
            // 2. Atomic check-and-set on the gate. Naive `peek`-then-
            //    `set` lets two concurrent scroll events both pass the
            //    check before either sets the flag, then both fire the
            //    fetch — duplicate older pages stack up. `with_mut`
            //    runs the closure under the signal's exclusive write
            //    lock, so the {observe, mutate} pair is one critical
            //    section. Adversarial-review P2.
            let acquired = loading_older.with_mut(|v| {
                if *v {
                    false
                } else {
                    *v = true;
                    true
                }
            });
            if !acquired {
                return;
            }
            // 3. Fetch + prepend.
            let Some(client) = fresh_client(session_for_older).await else {
                loading_older.set(false);
                return;
            };
            match client
                .chat_get_messages(&convo_id, Some(&cursor), PAGE_LIMIT)
                .await
            {
                Ok(resp) => {
                    let mut older: Vec<Message> = resp.messages;
                    older.reverse(); // server gives newest-first; we want oldest-top
                                     // Prepend. CSS uses overflow-anchor: auto on
                                     // .messages__body so WebKit holds the visible
                                     // content in place while items insert above
                                     // the current viewport — the user keeps their
                                     // place mid-conversation rather than getting
                                     // yanked back to the new top.
                    let mut current = messages.write();
                    let mut combined = Vec::with_capacity(older.len() + current.len());
                    combined.extend(older);
                    combined.extend(current.drain(..));
                    *current = combined;
                    let next_cursor = resp.cursor;
                    if next_cursor.is_none() {
                        no_more_older.set(true);
                    }
                    older_cursor.set(next_cursor);
                }
                Err(e) => {
                    // Soft-fail older loads — the user has the recent
                    // page; a transient blip shouldn't nuke the list.
                    // Next scroll-to-top retries.
                    eprintln!("[messages_sheet] load-older failed: {e}");
                }
            }
            loading_older.set(false);
        });
    };

    // Auto-scroll-to-bottom whenever `tail_grew` bumps (initial load,
    // poll-discovered new message, successful send). Skipped on
    // prepend (older-message load) because the prepend path doesn't
    // touch tail_grew. Use document::eval since scroll geometry
    // lives in the webview, not the Rust render tree.
    let _ = use_resource(move || {
        let _trigger = *tail_grew.read();
        async move {
            // One frame of slop so the freshly-appended bubble is in
            // the DOM by the time we scroll.
            tokio::time::sleep(Duration::from_millis(16)).await;
            let _ = document::eval(
                r#"
                const el = document.querySelector('.messages__body');
                if (el) { el.scrollTop = el.scrollHeight; }
                "#,
            );
        }
    });

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
    let partner_snap = partner.read().clone();

    // Pre-compute per-message group flags. A "group" is a run of
    // consecutive messages from the same sender (within 5 min). The
    // first row in a group gets the avatar (other-party only) + top
    // gap; the last row in a group keeps its directional tail
    // (border-radius asymmetry) + shows the time chip.
    let groups = compute_groups(&messages_snap);

    let header_display = partner_snap.as_ref().and_then(|p| {
        p.display_name
            .clone()
            .or_else(|| Some(format!("@{}", p.handle)))
    });
    let header_handle = partner_snap.as_ref().map(|p| format!("@{}", p.handle));
    let header_avatar = partner_snap.as_ref().and_then(|p| p.avatar.clone());

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet messages__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "messages__head",
                    div { class: "messages__head-avatar",
                        if let Some(url) = header_avatar.as_ref() {
                            img { src: "{url}", alt: "" }
                        }
                    }
                    div { class: "messages__head-text",
                        if let Some(d) = header_display.as_ref() {
                            div { class: "messages__head-display", "{d}" }
                        } else {
                            div { class: "messages__head-display", "Messages" }
                        }
                        if let Some(h) = header_handle.as_ref() {
                            div { class: "messages__head-handle", "{h}" }
                        }
                    }
                    button { class: "messages__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "messages__body",
                    onscroll: on_scroll,
                    if *loading_older.read() {
                        div { class: "messages__loading-older", "Loading older messages…" }
                    } else if *no_more_older.read() && !messages_snap.is_empty() {
                        div { class: "messages__history-end", "Start of conversation." }
                    }
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
                                first_in_group: groups[i].first,
                                last_in_group: groups[i].last,
                                partner_avatar: header_avatar.clone(),
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

/// Per-message grouping flags. Computed once per render so each
/// MessageBubble knows whether to show the avatar / round corners /
/// time chip without doing its own neighbor-lookup.
#[derive(Clone, Copy, Debug, Default)]
struct GroupFlags {
    first: bool,
    last: bool,
}

/// Window over consecutive messages from the same sender, splitting
/// into separate groups if the time gap exceeds [`GROUP_GAP_SECS`].
/// Returns one GroupFlags per input message.
fn compute_groups(msgs: &[Message]) -> Vec<GroupFlags> {
    const GROUP_GAP_SECS: i64 = 5 * 60;
    if msgs.is_empty() {
        return Vec::new();
    }
    let sender_of = |m: &Message| -> Option<String> {
        match m {
            Message::Live(v) => Some(v.sender.did.clone()),
            Message::Deleted(d) => Some(d.sender.did.clone()),
        }
    };
    let ts_of = |m: &Message| -> Option<chrono::DateTime<chrono::Utc>> {
        let s = match m {
            Message::Live(v) => &v.sent_at,
            Message::Deleted(d) => &d.sent_at,
        };
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|t| t.with_timezone(&chrono::Utc))
    };
    let mut out = vec![GroupFlags::default(); msgs.len()];
    for i in 0..msgs.len() {
        let is_first = if i == 0 {
            true
        } else {
            let prev_sender = sender_of(&msgs[i - 1]);
            let cur_sender = sender_of(&msgs[i]);
            let prev_ts = ts_of(&msgs[i - 1]);
            let cur_ts = ts_of(&msgs[i]);
            let sender_changed = prev_sender != cur_sender;
            let big_gap = match (prev_ts, cur_ts) {
                (Some(a), Some(b)) => (b - a).num_seconds() > GROUP_GAP_SECS,
                _ => false,
            };
            sender_changed || big_gap
        };
        let is_last = if i + 1 == msgs.len() {
            true
        } else {
            let cur_sender = sender_of(&msgs[i]);
            let next_sender = sender_of(&msgs[i + 1]);
            let cur_ts = ts_of(&msgs[i]);
            let next_ts = ts_of(&msgs[i + 1]);
            let sender_changes = cur_sender != next_sender;
            let big_gap = match (cur_ts, next_ts) {
                (Some(a), Some(b)) => (b - a).num_seconds() > GROUP_GAP_SECS,
                _ => false,
            };
            sender_changes || big_gap
        };
        out[i] = GroupFlags {
            first: is_first,
            last: is_last,
        };
    }
    out
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
fn MessageBubble(
    msg: Message,
    me: String,
    first_in_group: bool,
    last_in_group: bool,
    partner_avatar: Option<String>,
) -> Element {
    let row_class = if first_in_group && last_in_group {
        "messages__row messages__row--first-in-group messages__row--last-in-group"
    } else if first_in_group {
        "messages__row messages__row--first-in-group"
    } else if last_in_group {
        "messages__row messages__row--last-in-group"
    } else {
        "messages__row"
    };

    match msg {
        Message::Live(v) => {
            let is_me = v.sender.did == me;
            let bubble_class = if is_me {
                "messages__bubble messages__bubble--me"
            } else {
                "messages__bubble messages__bubble--other"
            };
            // Time chip shows only on the LAST bubble in a group —
            // chat-UX standard so the gutter doesn't fill with HH:MM
            // when the same person sends 4 messages in a minute.
            let time = if last_in_group {
                format_short_time(&v.sent_at)
            } else {
                None
            };
            let time_class = if is_me {
                "messages__time messages__time--me"
            } else {
                "messages__time messages__time--other"
            };
            // Other-party rows: show the avatar on the FIRST row of a
            // group (so multi-message bursts don't repeat the avatar);
            // an empty spacer on subsequent rows keeps the bubbles
            // horizontally aligned.
            rsx! {
                div { class: "{row_class}",
                    if !is_me {
                        if first_in_group {
                            div { class: "messages__row-avatar",
                                if let Some(url) = partner_avatar.as_ref() {
                                    img { src: "{url}", alt: "" }
                                }
                            }
                        } else {
                            div { class: "messages__row-avatar-spacer" }
                        }
                    }
                    div { class: "{bubble_class}",
                        div { class: "messages__text", "{v.text}" }
                    }
                }
                if let Some(t) = time {
                    div { class: "{time_class}", "{t}" }
                }
            }
        }
        Message::Deleted(_) => rsx! {
            div { class: "{row_class}",
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
