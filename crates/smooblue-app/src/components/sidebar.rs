//! Left rail navigation. Uses the shared `.rail` / `.rail__btn` classes
//! from smooai-ui plus a few smooblue-only positioning extensions
//! (`.rail__logo`, `.rail__divider`, `.rail__spacer`).

use crate::icons;
use crate::state::{add_column_unique, ColumnSpec};
use dioxus::prelude::*;
use smooblue_oauth::Session;

#[component]
pub fn Sidebar(search_open: Signal<bool>) -> Element {
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let session = use_context::<Signal<Option<Session>>>();

    let add_home = move |_| add_column_unique(&mut cols, ColumnSpec::home());
    let add_notif = move |_| add_column_unique(&mut cols, ColumnSpec::notifications());
    let add_discover = move |_| add_column_unique(&mut cols, ColumnSpec::discover());
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

    rsx! {
        nav { class: "rail",
            // Smooblue product mark (smoo monogram + cartoon butterfly).
            div { class: "rail__logo", title: "Smooblue",
                dangerous_inner_html: "{smooblue_theme::BRAND_SVG}",
            }
            RailBtn { label: "Home", active: true, kind: RailKind::Home, onclick: add_home }
            RailBtn { label: "Search", active: false, kind: RailKind::Search, onclick: open_search }
            RailBtn { label: "Notifications", active: false, kind: RailKind::Bell, onclick: add_notif }
            RailBtn { label: "Discover", active: false, kind: RailKind::Compass, onclick: add_discover }
            div { class: "rail__divider" }
            RailBtn { label: "Add column", active: false, kind: RailKind::Add, onclick: open_search }
            div { class: "rail__spacer" }
            RailBtn { label: "Profile", active: false, kind: RailKind::Profile, onclick: add_self_profile }
            RailBtn { label: "Settings", active: false, kind: RailKind::Settings, onclick: move |_| {} }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum RailKind {
    Home,
    Search,
    Bell,
    Compass,
    Add,
    Profile,
    Settings,
}

#[component]
fn RailBtn(
    label: String,
    active: bool,
    kind: RailKind,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let class = if active {
        "rail__btn rail__btn--active"
    } else {
        "rail__btn"
    };
    rsx! {
        button { class: "{class}", title: "{label}",
            onclick: move |evt| onclick.call(evt),
            match kind {
                RailKind::Home => rsx! { icons::Home { size: icons::Size::Md } },
                RailKind::Search => rsx! { icons::Search { size: icons::Size::Md } },
                RailKind::Bell => rsx! { icons::Bell { size: icons::Size::Md } },
                RailKind::Compass => rsx! { icons::Compass { size: icons::Size::Md } },
                RailKind::Add => rsx! { icons::Plus { size: icons::Size::Md } },
                RailKind::Profile => rsx! { icons::User { size: icons::Size::Md } },
                RailKind::Settings => rsx! { icons::Settings { size: icons::Size::Md } },
            }
        }
    }
}
