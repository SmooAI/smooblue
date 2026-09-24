//! Left rail navigation. Uses the shared `.rail` / `.rail__btn` classes
//! from smooai-ui plus a few smooblue-only positioning extensions
//! (`.rail__logo`, `.rail__divider`, `.rail__spacer`) and the
//! navigation group at the top (`.rail__nav`: back / forward / reopen /
//! history).

use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::state::{add_or_focus_column, ColumnSpec, FocusColumn, ProfileFocus};
use dioxus::prelude::*;
use smooblue_oauth::Session;
use std::time::Duration;

/// How often the sidebar polls `notification.getUnreadCount`. Cheap
/// endpoint — counts are cached server-side. 30s feels live without
/// hammering the AppView.
const UNREAD_POLL_SECS: u64 = 30;

#[component]
pub fn Sidebar(
    search_open: Signal<bool>,
    saved_feeds_open: Signal<bool>,
    settings_open: Signal<bool>,
    history_open: Signal<bool>,
) -> Element {
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let session = use_context::<Signal<Option<Session>>>();
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();

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

    // Add-or-focus: if the column already exists, scroll to it
    // and flash its border so the user can see *where* it is.
    let mut focus_col = use_context::<Signal<FocusColumn>>();
    let add_home = move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::home());
    let add_notif =
        move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::notifications());
    let add_discover =
        move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::discover());
    let add_suggestions =
        move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::suggestions());
    let add_messages =
        move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::messages());
    let add_inbox = move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::inbox());
    let add_saved =
        move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::bookmarks());
    let add_analytics =
        move |_| add_or_focus_column(&mut cols, &mut focus_col, ColumnSpec::analytics());
    let open_search = move |_| search_open.set(true);
    let mut sf_open = saved_feeds_open;
    let open_saved_feeds = move |_| sf_open.set(true);
    let mut st_open = settings_open;
    let open_settings = move |_| st_open.set(true);
    // Sidebar Profile button now opens the user's own ProfileSheet
    // (banner + bio + counts + Follow yourself? no) — much richer
    // than just adding an AuthorFeed column. The sheet has an
    // "+ Column" button for users who still want the persistent
    // column behavior.
    let open_self_profile = move |_| {
        if let Some(s) = session.read().clone() {
            profile_focus.set(ProfileFocus(Some(s.did)));
        }
    };

    // Fetch the signed-in user's profile once so we can render
    // their actual avatar in the Profile slot instead of a generic
    // User glyph. Cached in a signal so it sticks across re-renders.
    let mut self_avatar = use_signal::<Option<String>>(|| None);
    let mut self_handle = use_signal::<Option<String>>(|| None);
    use_future(move || async move {
        if crate::demo::is_active() {
            self_avatar.set(Some("https://picsum.photos/seed/you/80".into()));
            self_handle.set(Some("you.smoo.ai".into()));
            return;
        }
        let Some(s) = session.read().clone() else {
            return;
        };
        self_handle.set(Some(s.handle.clone()));
        if let Some(client) = fresh_client(session).await {
            if let Ok(p) = client.get_profile(&s.did).await {
                if let Some(av) = p.avatar {
                    self_avatar.set(Some(av));
                }
            }
        }
    });

    let unread_count = *unread.read();
    let avatar_snap = self_avatar.read().clone();
    let handle_snap = self_handle.read().clone();

    rsx! {
        nav { class: "rail",
            // Smooblue product mark (smoo monogram + cartoon butterfly).
            div { class: "rail__logo", title: "Smooblue",
                dangerous_inner_html: "{smooblue_theme::BRAND_SVG}",
            }
            NavGroup { history_open }
            div { class: "rail__divider" }
            RailBtn { label: "Home", active: true, kind: RailKind::Home, badge: 0, onclick: add_home }
            RailBtn { label: "Search", active: false, kind: RailKind::Search, badge: 0, onclick: open_search }
            RailBtn { label: "Notifications", active: false, kind: RailKind::Bell, badge: unread_count, onclick: add_notif }
            RailBtn { label: "Discover", active: false, kind: RailKind::Compass, badge: 0, onclick: add_discover }
            RailBtn { label: "Suggested follows", active: false, kind: RailKind::Sparkles, badge: 0, onclick: add_suggestions }
            RailBtn { label: "Messages", active: false, kind: RailKind::Messages, badge: 0, onclick: add_messages }
            RailBtn { label: "Inbox", active: false, kind: RailKind::InboxTriage, badge: 0, onclick: add_inbox }
            RailBtn { label: "Saved posts (g b)", active: false, kind: RailKind::Bookmark, badge: 0, onclick: add_saved }
            RailBtn { label: "Analytics", active: false, kind: RailKind::Analytics, badge: 0, onclick: add_analytics }
            div { class: "rail__divider" }
            // "+ Add column" opens the Saved Feeds sheet (which lists
            // your saved feeds, your lists, your *own* feed generators,
            // trending topics, popular feeds, AND a paste-a-URI box).
            // Search is its own button above — they're different intents.
            RailBtn { label: "Add column", active: false, kind: RailKind::Add, badge: 0, onclick: open_saved_feeds }
            div { class: "rail__spacer" }
            // Profile slot — real avatar when we've resolved one,
            // generic User glyph as fallback until the get_profile
            // future settles. Tooltip shows the handle so the user
            // can confirm which account is active in multi-account
            // setups.
            button {
                class: "rail__btn rail__avatar-btn",
                title: handle_snap.as_ref().map(|h| format!("@{h}")).unwrap_or_else(|| "Profile".into()),
                onclick: open_self_profile,
                if let Some(url) = avatar_snap {
                    img {
                        class: "rail__avatar",
                        src: "{url}",
                        alt: handle_snap.as_deref().unwrap_or("profile"),
                        loading: "lazy",
                        decoding: "async",
                    }
                } else {
                    icons::User { size: icons::Size::Md }
                }
            }
            RailBtn { label: "Settings", active: false, kind: RailKind::Settings, badge: 0, onclick: open_settings }
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
    Messages,
    InboxTriage,
    Analytics,
    Bookmark,
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
                RailKind::Messages => rsx! { icons::MessageCircle { size: icons::Size::Md } },
                RailKind::InboxTriage => rsx! { icons::Inbox { size: icons::Size::Md } },
                RailKind::Analytics => rsx! { icons::ChartColumn { size: icons::Size::Md } },
                RailKind::Bookmark => rsx! { icons::Bookmark { size: icons::Size::Md } },
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

/// Back / forward / reopen / history — "get back to where you were",
/// always one click away at the top of the rail. It sits above the
/// sheet backdrop (`.rail__nav` z-index), so it stays crisp and
/// clickable while a thread or profile is open — exactly when you want
/// to step back — but under the compose sheet, so it can't pull a
/// thread out from under a reply in progress. The same actions as the
/// ⌘[ / ⌘] / ⌘⇧T / ⌘Y shortcuts ([`crate::keyboard`]).
#[component]
fn NavGroup(history_open: Signal<bool>) -> Element {
    use crate::history::{self, NavHistory, View};
    use crate::state::ThreadFocus;

    let nav = use_context::<Signal<NavHistory>>();
    let thread = use_context::<Signal<ThreadFocus>>();
    let profile = use_context::<Signal<ProfileFocus>>();
    let mut history_open = history_open;

    let thread_open = thread.read().0.is_some();
    let profile_open = profile.read().0.is_some();
    let n = nav.read();
    // One timeline across the deck and both sheets (crate::history), so
    // ← is live as soon as you've opened anything — and at the deck it
    // steps back into what you just closed.
    let can_back = n.can_go_back();
    let can_forward = n.can_go_forward();
    let back_title = n
        .back_target()
        .map(|v| format!("Back to {} (⌘[)", history::describe(v)))
        .unwrap_or_else(|| "Back (⌘[)".into());
    let forward_title = n
        .forward_target()
        .map(|v| format!("Forward to {} (⌘])", history::describe(v)))
        .unwrap_or_else(|| "Forward (⌘])".into());
    // Reopen: something was closed and it isn't already back open.
    let reopen_title = match n.last_closed.as_ref() {
        Some(View::Thread(_)) if !thread_open => Some("Reopen the thread you closed (⌘⇧T)"),
        Some(View::Profile(_)) if !profile_open => Some("Reopen the profile you closed (⌘⇧T)"),
        _ => None,
    };
    drop(n);
    let can_reopen = reopen_title.is_some();
    let reopen_title = reopen_title.unwrap_or("Reopen — nothing closed yet (⌘⇧T)");
    let group_class = if thread_open || profile_open {
        "rail__nav rail__nav--over-sheet"
    } else {
        "rail__nav"
    };
    let history_class = if *history_open.read() {
        "rail__btn rail__btn--active"
    } else {
        "rail__btn"
    };

    rsx! {
        div { class: "{group_class}",
            div { class: "rail__nav-row",
                button { class: "rail__nav-step",
                    title: "{back_title}",
                    disabled: !can_back,
                    onclick: move |_| {
                        history::step(nav, thread, profile, false);
                    },
                    icons::ArrowLeft { size: icons::Size::Sm }
                }
                button { class: "rail__nav-step",
                    title: "{forward_title}",
                    disabled: !can_forward,
                    onclick: move |_| {
                        history::step(nav, thread, profile, true);
                    },
                    icons::ArrowRight { size: icons::Size::Sm }
                }
            }
            button { class: "rail__btn",
                title: "{reopen_title}",
                disabled: !can_reopen,
                onclick: move |_| {
                    history::reopen_last(nav, thread, profile);
                },
                icons::RotateCcw { size: icons::Size::Md }
            }
            button { class: "{history_class}",
                title: "History — recently viewed threads & profiles (⌘Y)",
                onclick: move |_| {
                    let open = *history_open.peek();
                    history_open.set(!open);
                },
                icons::History { size: icons::Size::Md }
            }
        }
    }
}
