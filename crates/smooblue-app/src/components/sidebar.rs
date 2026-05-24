//! Left rail navigation. Uses the shared `.rail` / `.rail__btn` classes
//! from smooai-ui plus a few smooblue-only positioning extensions
//! (`.rail__logo`, `.rail__divider`, `.rail__spacer`).

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::{add_column_unique, ColumnSpec};
use dioxus::prelude::*;
use smooblue_oauth::Session;
use std::time::Duration;

/// How often the sidebar polls `notification.getUnreadCount`. Cheap
/// endpoint — counts are cached server-side. 30s feels live without
/// hammering the AppView.
const UNREAD_POLL_SECS: u64 = 30;

#[component]
pub fn Sidebar(search_open: Signal<bool>) -> Element {
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let session = use_context::<Signal<Option<Session>>>();

    // Polling loop for the unread-notifications badge. Lives at the
    // sidebar level (not per-column) so the badge stays visible even
    // when the user hasn't added a Notifications column yet.
    let mut unread = use_signal(|| 0u32);
    use_future(move || {
        let session_sig = session;
        async move {
            loop {
                if crate::demo::is_active() {
                    // In demo mode show a non-zero unread count so the
                    // badge is on-screen for screenshots.
                    unread.set(3);
                } else if session_sig.read().is_some() {
                    if let Some(client) = fresh_client(session_sig).await {
                        if let Ok(n) = client.get_unread_count().await {
                            unread.set(n);
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(UNREAD_POLL_SECS)).await;
            }
        }
    });

    let add_home = move |_| add_column_unique(&mut cols, ColumnSpec::home());
    let add_notif = move |_| add_column_unique(&mut cols, ColumnSpec::notifications());
    let add_discover = move |_| add_column_unique(&mut cols, ColumnSpec::discover());
    let add_suggestions = move |_| add_column_unique(&mut cols, ColumnSpec::suggestions());
    let open_search = move |_| search_open.set(true);
    let add_self_profile = move |_| {
        if let Some(s) = session.read().clone() {
            let title = if s.handle.is_empty() {
                "Profile".to_string()
            } else {
                format!("@{}", s.handle)
            };
            add_column_unique(&mut cols, ColumnSpec::author(s.did, title));
        }
    };

    let unread_count = *unread.read();

    rsx! {
        nav { class: "rail",
            // Smooblue product mark (smoo monogram + cartoon butterfly).
            div { class: "rail__logo", title: "Smooblue",
                dangerous_inner_html: "{smooblue_theme::BRAND_SVG}",
            }
            RailBtn { label: "Home", active: true, kind: RailKind::Home, badge: 0, onclick: add_home }
            RailBtn { label: "Search", active: false, kind: RailKind::Search, badge: 0, onclick: open_search }
            RailBtn { label: "Notifications", active: false, kind: RailKind::Bell, badge: unread_count, onclick: add_notif }
            RailBtn { label: "Discover", active: false, kind: RailKind::Compass, badge: 0, onclick: add_discover }
            RailBtn { label: "Suggested follows", active: false, kind: RailKind::Sparkles, badge: 0, onclick: add_suggestions }
            div { class: "rail__divider" }
            RailBtn { label: "Add column", active: false, kind: RailKind::Add, badge: 0, onclick: open_search }
            div { class: "rail__spacer" }
            RailBtn { label: "Profile", active: false, kind: RailKind::Profile, badge: 0, onclick: add_self_profile }
            RailBtn { label: "Settings", active: false, kind: RailKind::Settings, badge: 0, onclick: move |_| {} }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum RailKind {
    Home,
    Search,
    Bell,
    Compass,
    Sparkles,
    Add,
    Profile,
    Settings,
}

#[component]
fn RailBtn(
    label: String,
    active: bool,
    kind: RailKind,
    badge: u32,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let class = if active {
        "rail__btn rail__btn--active"
    } else {
        "rail__btn"
    };
    // Compact badge: any 0 hides; 1-99 shows the number; 100+ shows
    // "99+" (matches bsky.app's cap so the pill never blows out).
    let badge_text = if badge == 0 {
        None
    } else if badge < 100 {
        Some(badge.to_string())
    } else {
        Some("99+".to_string())
    };
    rsx! {
        button { class: "{class}", title: "{label}",
            onclick: move |evt| onclick.call(evt),
            match kind {
                RailKind::Home => rsx! { icons::Home { size: icons::Size::Md } },
                RailKind::Search => rsx! { icons::Search { size: icons::Size::Md } },
                RailKind::Bell => rsx! { icons::Bell { size: icons::Size::Md } },
                RailKind::Compass => rsx! { icons::Compass { size: icons::Size::Md } },
                RailKind::Sparkles => rsx! { icons::Sparkles { size: icons::Size::Md } },
                RailKind::Add => rsx! { icons::Plus { size: icons::Size::Md } },
                RailKind::Profile => rsx! { icons::User { size: icons::Size::Md } },
                RailKind::Settings => rsx! { icons::Settings { size: icons::Size::Md } },
            }
            if let Some(text) = badge_text {
                span { class: "rail__badge", "{text}" }
            }
        }
    }
}
