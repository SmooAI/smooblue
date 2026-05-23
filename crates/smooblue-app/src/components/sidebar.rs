//! Left rail navigation. Uses the shared `.rail` / `.rail__btn` classes
//! from smooai-ui plus a few smooblue-only positioning extensions
//! (`.rail__logo`, `.rail__divider`, `.rail__spacer`).

use crate::icons;
use dioxus::prelude::*;

#[component]
pub fn Sidebar() -> Element {
    rsx! {
        nav { class: "rail",
            // Smooblue product mark (smoo monogram + bluesky butterfly).
            // Self-contained SVG with its own backdrop, so no .brand-badge
            // gradient pill behind it.
            div { class: "rail__logo", title: "Smooblue",
                dangerous_inner_html: "{smooblue_theme::BRAND_SVG}",
            }
            RailBtn { label: "Home", active: true, kind: RailKind::Home }
            RailBtn { label: "Search", active: false, kind: RailKind::Search }
            RailBtn { label: "Notifications", active: false, kind: RailKind::Bell }
            RailBtn { label: "Discover", active: false, kind: RailKind::Compass }
            div { class: "rail__divider" }
            RailBtn { label: "Add column", active: false, kind: RailKind::Add }
            div { class: "rail__spacer" }
            RailBtn { label: "Profile", active: false, kind: RailKind::Profile }
            RailBtn { label: "Settings", active: false, kind: RailKind::Settings }
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
fn RailBtn(label: String, active: bool, kind: RailKind) -> Element {
    let class = if active {
        "rail__btn rail__btn--active"
    } else {
        "rail__btn"
    };
    rsx! {
        button { class: "{class}", title: "{label}",
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
