//! History sheet (⌘Y / rail clock) — recently viewed threads and
//! profiles, newest first, so a thread you clicked away from yesterday
//! is one click away. Plus the two small "get back to where you were"
//! affordances that live over the deck: the reopen toast shown after a
//! sheet closes, and the resume-draft chip above the compose FAB.

use crate::history::{self, HistoryEntry, NavHistory, NavKind, View};
use crate::icons;
use crate::state::{ComposeContext, DraftsIndex, ProfileFocus, ThreadFocus};
use dioxus::prelude::*;

/// Rows loaded into the sheet. The table holds more; nobody scrolls
/// past a few hundred.
const SHEET_LIMIT: usize = 300;
/// How long the "Reopen" toast stays up after a sheet closes.
const REOPEN_TOAST_SECS: u64 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum KindFilter {
    All,
    Threads,
    Profiles,
}

#[component]
pub fn HistorySheet(open: Signal<bool>) -> Element {
    let thread = use_context::<Signal<ThreadFocus>>();
    let profile = use_context::<Signal<ProfileFocus>>();
    let mut entries = use_signal(Vec::<HistoryEntry>::new);
    let mut loaded = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut kind_filter = use_signal(|| KindFilter::All);
    let mut confirm_clear = use_signal(|| false);

    // (Re)load every time the sheet opens, so it reflects what was
    // viewed since it was last shown.
    use_effect(move || {
        if !*open.read() {
            loaded.set(false);
            return;
        }
        spawn(async move {
            match tokio::task::spawn_blocking(|| history::list(SHEET_LIMIT)).await {
                Ok(Ok(list)) => entries.set(list),
                Ok(Err(e)) => tracing::warn!(error = %e, "history: list failed"),
                Err(e) => tracing::warn!(error = %e, "history: list task panicked"),
            }
            loaded.set(true);
        });
    });

    if !*open.read() {
        return rsx! { Fragment {} };
    }

    let mut close_sig = open;
    let close = move |_| close_sig.set(false);
    let all = entries.read().clone();
    let q = query.read().clone();
    let filter = *kind_filter.read();
    let shown: Vec<HistoryEntry> = history::filter(&all, &q)
        .into_iter()
        .filter(|e| match filter {
            KindFilter::All => true,
            KindFilter::Threads => e.kind == NavKind::Thread,
            KindFilter::Profiles => e.kind == NavKind::Profile,
        })
        .cloned()
        .collect();

    let tab = |f: KindFilter| {
        if filter == f {
            "history__tab history__tab--active"
        } else {
            "history__tab"
        }
    };

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet history__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "history__head",
                    span { class: "history__title", "History" }
                    button { class: "thread__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "history__controls",
                    input {
                        class: "input history__search",
                        placeholder: "Filter by name, handle, or post text",
                        value: "{query}",
                        autofocus: true,
                        onmounted: move |evt: Event<MountedData>| {
                            spawn(async move {
                                let _ = evt.data().set_focus(true).await;
                            });
                        },
                        oninput: move |e| query.set(e.value()),
                    }
                    div { class: "history__tabs",
                        button { class: tab(KindFilter::All), onclick: move |_| kind_filter.set(KindFilter::All), "All" }
                        button { class: tab(KindFilter::Threads), onclick: move |_| kind_filter.set(KindFilter::Threads), "Threads" }
                        button { class: tab(KindFilter::Profiles), onclick: move |_| kind_filter.set(KindFilter::Profiles), "Profiles" }
                    }
                }
                div { class: "history__body",
                    if !*loaded.read() {
                        div { class: "history__empty", "Loading…" }
                    } else if all.is_empty() {
                        div { class: "history__empty",
                            "Nothing here yet. Threads and profiles you open show up here, so you can always find your way back."
                        }
                    } else if shown.is_empty() {
                        div { class: "history__empty", "No matches." }
                    } else {
                        for e in shown {
                            {
                                let key = e.key.clone();
                                let kind = e.kind;
                                let row_key = format!("{:?}:{}", e.kind, e.key);
                                let ts = e.viewed_at.to_rfc3339();
                                rsx! {
                                    button { key: "{row_key}",
                                        class: "history__row",
                                        onclick: move |_| {
                                            history::open_entry(kind, key.clone(), thread, profile);
                                            close_sig.set(false);
                                        },
                                        span { class: "history__kind",
                                            if kind == NavKind::Thread {
                                                icons::MessageCircle { size: icons::Size::Sm }
                                            } else {
                                                icons::User { size: icons::Size::Sm }
                                            }
                                        }
                                        span { class: "history__text",
                                            span { class: "history__row-title", "{e.title}" }
                                            span { class: "history__row-sub", "{e.subtitle}" }
                                        }
                                        span { class: "history__time",
                                            icons::TimeAgo { text_at_render: String::new(), source_ts: Some(ts) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !all.is_empty() {
                    div { class: "history__foot",
                        button {
                            class: if *confirm_clear.read() { "history__clear history__clear--armed" } else { "history__clear" },
                            onclick: move |_| {
                                if !*confirm_clear.peek() {
                                    confirm_clear.set(true);
                                    return;
                                }
                                confirm_clear.set(false);
                                entries.set(Vec::new());
                                spawn(async move {
                                    let _ = tokio::task::spawn_blocking(history::clear).await;
                                });
                            },
                            if *confirm_clear.read() { "Click again to clear history" } else { "Clear history" }
                        }
                    }
                }
            }
        }
    }
}

/// "Closed thread — Reopen" toast. Appears for a few seconds after a
/// thread or profile sheet closes, because the usual way to lose a long
/// thread is a stray click on the backdrop. ⌘⇧T does the same thing
/// any time until something else is closed.
#[component]
pub fn ReopenToast() -> Element {
    let nav = use_context::<Signal<NavHistory>>();
    let thread = use_context::<Signal<ThreadFocus>>();
    let profile = use_context::<Signal<ProfileFocus>>();
    let mut shown_seq = use_signal(|| 0u64);
    let mut visible = use_signal(|| false);

    use_effect(move || {
        let seq = nav.read().closed_seq;
        if seq == 0 || seq == *shown_seq.peek() {
            return;
        }
        shown_seq.set(seq);
        visible.set(true);
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(REOPEN_TOAST_SECS)).await;
            if *shown_seq.peek() == seq {
                visible.set(false);
            }
        });
    });

    let closed = nav.read().last_closed.clone();
    let Some(closed) = closed.filter(|_| *visible.read()) else {
        return rsx! { Fragment {} };
    };
    // Don't offer to reopen what's already open again.
    let (reopened, what) = match closed {
        View::Thread(_) => (thread.read().0.is_some(), "thread"),
        View::Profile(_) => (profile.read().0.is_some(), "profile"),
        View::Deck => (true, ""),
    };
    if reopened {
        return rsx! { Fragment {} };
    }
    rsx! {
        div { class: "reopen-toast",
            span { class: "reopen-toast__label", "Closed {what}" }
            button { class: "reopen-toast__btn",
                onclick: move |_| {
                    visible.set(false);
                    history::reopen_last(nav, thread, profile);
                },
                icons::RotateCcw { size: icons::Size::Sm }
                "Reopen"
                span { class: "reopen-toast__kbd", "⌘⇧T" }
            }
            button { class: "update-toast__dismiss",
                title: "Dismiss",
                onclick: move |_| visible.set(false),
                icons::X { size: icons::Size::Sm }
            }
        }
    }
}

/// Resume-draft chip above the compose FAB. Shown whenever compose is
/// closed and there's an unsent draft, so closing the composer — on
/// purpose or by a stray click — never hides work in progress.
#[component]
pub fn DraftResumeChip() -> Element {
    let index = use_context::<Signal<DraftsIndex>>();
    let mut ctx = use_context::<Signal<ComposeContext>>();
    if ctx.read().open {
        return rsx! { Fragment {} };
    }
    let (id, label, preview, more) = {
        let drafts = index.read();
        let Some(d) = drafts.0.first() else {
            return rsx! { Fragment {} };
        };
        (d.id.clone(), d.label(), d.preview(48), drafts.0.len() - 1)
    };
    rsx! {
        button { class: "draft-chip",
            title: "Resume your draft",
            onclick: move |_| ctx.write().open_draft(id.clone()),
            span { class: "draft-chip__icon", icons::PenLine { size: icons::Size::Sm } }
            span { class: "draft-chip__text",
                span { class: "draft-chip__label", "{label}" }
                if !preview.is_empty() {
                    span { class: "draft-chip__preview", "{preview}" }
                }
            }
            if more > 0 {
                span { class: "draft-chip__more", title: "More drafts in the composer's Drafts list", "+{more}" }
            }
        }
    }
}
