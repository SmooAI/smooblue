//! Deck shell — rail + horizontally-scrolling columns + floating "+"
//! (compose) + the global compose sheet + the search-column add sheet.

use crate::components::{
    column::Column, compose::ComposeSheet, engagement::EngagementSheet, lightbox::LightboxSheet,
    profile::ProfileSheet, profile_edit_sheet::ProfileEditSheet, report_sheet::ReportSheet,
    saved_feeds_sheet::SavedFeedsSheet, search_sheet::SearchSheet, settings_sheet::SettingsSheet,
    sidebar::Sidebar, thread::ThreadSheet,
};
use crate::icons;
use crate::keyboard::{self, KeyContext};
use crate::state::{
    ColumnSpec, ComposeContext, EngagementFocus, FocusedItem, KeyboardHelp, LightboxFocus,
    PendingChord, ProfileFocus, ThemeMode, ThreadFocus, Tick, UpdateBanner,
};
use dioxus::prelude::*;
use smooblue_oauth::Session;
use std::time::Duration;

#[component]
pub fn DeckShell() -> Element {
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let columns = cols.read().clone();
    let mut compose_ctx = use_context::<Signal<ComposeContext>>();
    let search_open = use_signal(|| false);
    let saved_feeds_open = use_signal(|| false);
    let settings_open = use_signal(|| false);

    // Bundle every signal the vim keyboard handler needs so we
    // don't have to thread a dozen args through the onkeydown
    // closure each render.
    let mut key_ctx = KeyContext {
        focus: use_context::<Signal<FocusedItem>>(),
        help: use_context::<Signal<KeyboardHelp>>(),
        chord: use_context::<Signal<PendingChord>>(),
        cols,
        compose: compose_ctx,
        thread: use_context::<Signal<ThreadFocus>>(),
        profile: use_context::<Signal<ProfileFocus>>(),
        engagement: use_context::<Signal<EngagementFocus>>(),
        session: use_context::<Signal<Option<Session>>>(),
        search_open,
        saved_feeds_open,
        settings_open,
        lightbox: use_context::<Signal<LightboxFocus>>(),
    };

    // Chord-timeout: clear PendingChord after 1.5s so a stray `g`
    // doesn't sit there waiting forever for the second key. Bsky's
    // own web client uses the same idle.
    let chord_sig = key_ctx.chord;
    use_future(move || {
        let mut chord = chord_sig;
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                // If the chord has been hanging > 1.5s, drop it.
                // (We don't track press time precisely — the polling
                // cadence is the chord-timeout granularity.)
                if chord.read().prefix.is_some() {
                    // Sleep one more interval then clear if still set.
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    if chord.peek().prefix.is_some() {
                        chord.set(PendingChord { prefix: None });
                    }
                }
            }
        }
    });

    // Self-update check — single GitHub releases API call on boot.
    // Stays silent on failure. Skipped in demo mode (would always
    // claim an update against the synthetic version).
    let mut update_banner = use_context::<Signal<UpdateBanner>>();
    use_future(move || async move {
        if crate::demo::is_active() {
            return;
        }
        // Single-shot: delay 5s so the boot animation finishes
        // before we maybe show a toast.
        tokio::time::sleep(Duration::from_secs(5)).await;
        let http = reqwest::Client::new();
        if let Some(update) = crate::updates::check_for_updates(&http).await {
            update_banner.set(UpdateBanner(Some(update)));
        }
    });

    // 1-second tick that drives time-relative re-renders (post timestamps
    // ticking "11s" → "12s" etc.). Reading the Tick context in
    // PostCard/NotificationCard subscribes them; the signal bump here
    // triggers their re-render.
    let tick = use_context::<Signal<Tick>>();
    use_future(move || {
        let mut tick = tick;
        async move {
            let mut counter: u64 = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                counter = counter.wrapping_add(1);
                tick.set(Tick(counter));
            }
        }
    });

    let open_compose = move |_| {
        let mut w = compose_ctx.write();
        w.reply_to = None;
        w.open = true;
    };

    // "Update available" toast. Bottom-left, dismissible, links to
    // the GitHub release page. Auto-installer is a future pearl —
    // for now the user reads the changelog and downloads the new
    // .app on their schedule.
    #[component]
    fn UpdateToast() -> Element {
        let mut banner = use_context::<Signal<UpdateBanner>>();
        let snap = banner.read().0.clone();
        let Some(update) = snap else {
            return rsx! { Fragment {} };
        };
        let url = update.url.clone();
        let open_url = move |_| {
            // GitHub release URL — scheme-allowlisted defensively
            // even though the source is github.com/SmooAI/smooblue.
            let _ = crate::safe_open::open_in_browser(&url);
        };
        let dismiss = move |_| banner.set(UpdateBanner(None));
        rsx! {
            div { class: "update-toast",
                div { class: "update-toast__body",
                    span { class: "update-toast__label", "Update available" }
                    button { class: "update-toast__tag", onclick: open_url,
                        "{update.tag} ↗"
                    }
                }
                button { class: "update-toast__dismiss",
                    onclick: dismiss,
                    icons::X { size: icons::Size::Sm }
                }
            }
        }
    }

    // Keyboard help overlay — shown with `?` or `<space>?`. Stays
    // out of the main deck render to keep the keymap close to its
    // documentation, which is also where users will look first.
    #[component]
    fn KeyboardHelpSheet() -> Element {
        let mut help = use_context::<Signal<KeyboardHelp>>();
        if !help.read().0 {
            return rsx! { Fragment {} };
        }
        let close = move |_| help.set(KeyboardHelp(false));
        rsx! {
            div { class: "modal__backdrop", onclick: close,
                div { class: "modal__sheet kbd-help__sheet",
                    onclick: move |e| e.stop_propagation(),
                    div { class: "kbd-help__head",
                        span { class: "kbd-help__title", "Keyboard shortcuts" }
                        button { class: "kbd-help__close",
                            onclick: close,
                            icons::X { size: icons::Size::Sm }
                        }
                    }
                    div { class: "kbd-help__body",
                        // Section: Navigation
                        h3 { class: "kbd-help__section", "Navigation" }
                        KbdRow { keys: "j / k", action: "Next / previous post" }
                        KbdRow { keys: "h / l", action: "Previous / next column" }
                        KbdRow { keys: "g g", action: "Top of column" }
                        KbdRow { keys: "G", action: "Bottom of column" }
                        KbdRow { keys: "Space + 1–9", action: "Focus column N" }

                        // Section: Add / open columns
                        h3 { class: "kbd-help__section", "Go to column" }
                        KbdRow { keys: "g h", action: "Home" }
                        KbdRow { keys: "g n", action: "Notifications" }
                        KbdRow { keys: "g d", action: "Discover" }
                        KbdRow { keys: "g s", action: "Suggested follows" }
                        KbdRow { keys: "g p", action: "Your profile" }

                        // Section: Compose
                        h3 { class: "kbd-help__section", "Compose" }
                        KbdRow { keys: "n  ·  Space + n", action: "New post" }
                        KbdRow { keys: "⌘↵", action: "Submit post (inside compose)" }
                        KbdRow { keys: "Esc", action: "Close any sheet" }

                        // Section: Quick actions
                        h3 { class: "kbd-help__section", "Quick actions" }
                        KbdRow { keys: "⌘K  ·  Space + /", action: "Search" }
                        KbdRow { keys: "Space + s", action: "Settings" }
                        KbdRow { keys: "Space + f", action: "Saved feeds & lists" }
                        KbdRow { keys: "?", action: "Toggle this help" }
                    }
                }
            }
        }
    }

    #[component]
    fn KbdRow(keys: String, action: String) -> Element {
        rsx! {
            div { class: "kbd-help__row",
                span { class: "kbd-help__keys", "{keys}" }
                span { class: "kbd-help__action", "{action}" }
            }
        }
    }

    let theme = use_context::<Signal<ThemeMode>>();
    let theme_attr = theme.read().as_attr();

    let onkeydown = move |evt: KeyboardEvent| {
        // Modifier state for combos (Cmd-K, Ctrl-d, etc.)
        let modifiers = evt.modifiers();
        // Dispatch. If the handler consumed the key, prevent_default
        // so the browser doesn't ALSO scroll the page on arrow keys
        // / steal the keystroke for find-in-page (Cmd-K shadowed
        // browser shortcuts, etc.).
        let key = evt.key();
        if keyboard::dispatch(&mut key_ctx, &key, modifiers) {
            evt.prevent_default();
        }
    };

    rsx! {
        // tabindex=0 so the deck div can receive keyboard focus on
        // launch — otherwise document.activeElement is body and
        // synthetic keydown might miss the bubble.
        div { class: "deck-shell",
            tabindex: "0",
            "data-theme": "{theme_attr}",
            onkeydown: onkeydown,
            Sidebar { search_open, saved_feeds_open, settings_open }
            div { class: "deck-columns",
                for spec in columns {
                    Column { key: "{spec.id}", spec: spec.clone() }
                }
            }
            button {
                class: "fab",
                title: "New post",
                onclick: open_compose,
                icons::Plus { size: icons::Size::Lg }
            }
            ComposeSheet {}
            SearchSheet { open: search_open }
            SavedFeedsSheet { open: saved_feeds_open }
            SettingsSheet { open: settings_open }
            ThreadSheet {}
            ProfileSheet {}
            EngagementSheet {}
            ReportSheet {}
            LightboxSheet {}
            ProfileEditSheet {}
            KeyboardHelpSheet {}
            UpdateToast {}
        }
    }
}
