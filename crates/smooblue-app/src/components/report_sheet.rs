//! Report-account / report-post dialog.
//!
//! Opened via a ProfileSheet action (or a future per-post menu). Two
//! steps:
//!
//! 1. Pick a reason from the canonical bsky moderation enum.
//! 2. Optional free-text context box.
//! 3. Submit → `com.atproto.moderation.createReport`.
//!
//! Confirmation is a tiny toast; the user doesn't need to know the
//! report ID. Bsky's mod team handles routing from there.

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::ReportFocus;
use dioxus::prelude::*;
use smooblue_oauth::Session;

/// Bsky's canonical reasonType values. Strings come straight from
/// the `com.atproto.moderation.defs#reasonType` lexicon enum.
const REASONS: &[(&str, &str)] = &[
    ("com.atproto.moderation.defs#reasonSpam", "Spam"),
    ("com.atproto.moderation.defs#reasonViolation", "Community guidelines violation"),
    ("com.atproto.moderation.defs#reasonMisleading", "Misleading content"),
    ("com.atproto.moderation.defs#reasonSexual", "Unwanted sexual content"),
    ("com.atproto.moderation.defs#reasonRude", "Anti-social / rude behavior"),
    ("com.atproto.moderation.defs#reasonOther", "Something else"),
];

#[component]
pub fn ReportSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut focus = use_context::<Signal<ReportFocus>>();
    let snap = focus.read().0.clone();

    let mut selected = use_signal(|| 0usize);
    let mut detail = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let mut confirmation = use_signal(|| None::<String>);

    let Some(target) = snap.clone() else {
        return rsx! { Fragment {} };
    };

    let close = move |_| {
        focus.set(ReportFocus(None));
        selected.set(0);
        detail.set(String::new());
        confirmation.set(None);
    };

    let target_clone = target.clone();
    let submit = move |_| {
        if *sending.read() {
            return;
        }
        if session.read().is_none() {
            return;
        }
        let reason_type = REASONS[*selected.read()].0.to_string();
        let detail_text = detail.read().clone();
        let target_now = target_clone.clone();
        sending.set(true);
        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                sending.set(false);
                return;
            };
            let result = match target_now {
                ReportTarget::Account { did } => {
                    client.create_report_account(&did, &reason_type, &detail_text).await
                }
                ReportTarget::Post { uri, cid } => {
                    client.create_report_post(&uri, &cid, &reason_type, &detail_text).await
                }
            };
            sending.set(false);
            match result {
                Ok(_) => confirmation.set(Some("Report sent — thanks for flagging.".into())),
                Err(e) => {
                    tracing::warn!(error = %e, "smooblue: report failed");
                    confirmation.set(Some(format!("Couldn't send report: {e}")));
                }
            }
        });
    };

    let header = match &target {
        ReportTarget::Account { .. } => "Report account",
        ReportTarget::Post { .. } => "Report post",
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet report__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "report__head",
                    span { class: "report__title", "{header}" }
                    button { class: "report__close", onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "report__body",
                    if let Some(msg) = &*confirmation.read() {
                        p { class: "report__confirmation", "{msg}" }
                        button { class: "btn btn--primary report__done",
                            onclick: close,
                            "Done"
                        }
                    } else {
                        p { class: "report__hint",
                            "Pick the most accurate reason — bsky moderation reviews each."
                        }
                        // Reason picker (radio-ish — single select).
                        div { class: "report__reasons",
                            for (i, (_, label)) in REASONS.iter().enumerate() {
                                button {
                                    key: "{i}",
                                    class: if *selected.read() == i {
                                        "report__reason report__reason--selected"
                                    } else {
                                        "report__reason"
                                    },
                                    onclick: move |_| selected.set(i),
                                    "{label}"
                                }
                            }
                        }
                        textarea {
                            class: "input report__detail",
                            placeholder: "Add context (optional)…",
                            value: "{detail}",
                            oninput: move |e| detail.set(e.value()),
                        }
                        button { class: "btn btn--primary report__submit",
                            disabled: *sending.read(),
                            onclick: submit,
                            if *sending.read() { "Sending…" } else { "Send report" }
                        }
                    }
                }
            }
        }
    }
}

/// What the report sheet is reporting. Two shapes because the
/// lexicon expects a different `subject` schema per type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportTarget {
    Account { did: String },
    Post { uri: String, cid: String },
}
