//! Left rail navigation. Mirrors deck.blue: smoo logo + a stack of icons
//! (home, search, notifications, etc.). For Session 1 only Home is wired.

use dioxus::prelude::*;

#[component]
pub fn Sidebar() -> Element {
    rsx! {
        nav { class: "deck-sidebar",
            div { class: "deck-sidebar__logo", title: "Smooblue",
                // Brand monogram — gradient fills come from the .deck-sidebar__logo class.
                svg {
                    width: "20",
                    height: "20",
                    view_box: "0 0 135 135",
                    fill: "white",
                    fill_rule: "evenodd",
                    path { d: "M45.63,15.38c-12.39,5.21-22.54,14.64-28.65,26.61-6.12,11.97-7.8,25.72-4.77,38.81,3.04,13.09,10.6,24.69,21.36,32.75,10.76,8.06,24.02,12.05,37.44,11.28,13.42-.77,26.13-6.26,35.9-15.5,9.76-9.24,15.95-21.63,17.46-34.99,1.51-13.36-1.74-26.82-9.19-38.01-1.07-1.61-.64-3.78.97-4.85,1.61-1.07,3.78-.64,4.85.97,8.36,12.56,12.02,27.68,10.32,42.67-1.7,15-8.64,28.91-19.61,39.28-10.96,10.37-25.24,16.54-40.31,17.4-15.07.87-29.96-3.62-42.04-12.66-12.08-9.05-20.58-22.07-23.99-36.77-3.41-14.7-1.51-30.14,5.35-43.58,6.87-13.44,18.26-24.02,32.17-29.87,13.91-5.85,29.44-6.6,43.85-2.11,1.85.57,2.88,2.54,2.3,4.38-.57,1.85-2.54,2.88-4.38,2.3-12.83-4-26.67-3.33-39.06,1.88Z" }
                }
            }
            SidebarBtn { label: "Home", active: true, icon: SidebarIcon::Home }
            SidebarBtn { label: "Search", active: false, icon: SidebarIcon::Search }
            SidebarBtn { label: "Notifications", active: false, icon: SidebarIcon::Bell }
            SidebarBtn { label: "Discover", active: false, icon: SidebarIcon::Bsky }
            div { class: "deck-sidebar__spacer" }
            SidebarBtn { label: "Profile", active: false, icon: SidebarIcon::User }
            SidebarBtn { label: "Settings", active: false, icon: SidebarIcon::Cog }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SidebarIcon {
    Home,
    Search,
    Bell,
    Bsky,
    User,
    Cog,
}

#[component]
fn SidebarBtn(label: String, active: bool, icon: SidebarIcon) -> Element {
    let class = if active {
        "deck-sidebar__btn deck-sidebar__btn--active"
    } else {
        "deck-sidebar__btn"
    };
    rsx! {
        button { class: "{class}", title: "{label}",
            match icon {
                SidebarIcon::Home => rsx! { Icon { path: "M3 11.5L12 4l9 7.5V20a1 1 0 01-1 1h-5v-6h-6v6H4a1 1 0 01-1-1v-8.5z".to_string() } },
                SidebarIcon::Search => rsx! { Icon { path: "M11 4a7 7 0 015.29 11.55l4.08 4.08-1.42 1.42-4.08-4.08A7 7 0 1111 4zm0 2a5 5 0 100 10 5 5 0 000-10z".to_string() } },
                SidebarIcon::Bell => rsx! { Icon { path: "M12 3a6 6 0 016 6v3.59l1.7 1.7A1 1 0 0119 16H5a1 1 0 01-.7-1.7L6 12.59V9a6 6 0 016-6zm-2 16h4a2 2 0 11-4 0z".to_string() } },
                SidebarIcon::Bsky => rsx! { Icon { path: "M5.85 4.6C8.62 6.66 11.6 10.81 12 13.2c.4-2.39 3.38-6.54 6.15-8.6 2-1.49 5.23-2.65 5.23.99 0 .73-.41 6.1-.65 6.97-.83 3.03-3.9 3.8-6.62 3.32 4.77.81 5.97 3.5 3.36 6.19-4.94 5.11-7.1-1.27-7.65-2.91-.1-.3-.15-.44-.15-.32 0-.12-.04.02-.15.32-.55 1.64-2.71 8.02-7.65 2.91-2.61-2.69-1.4-5.38 3.36-6.19-2.71.48-5.78-.29-6.61-3.32C.4 11.69 0 6.32 0 5.59c0-3.64 3.23-2.48 5.23-.99H5.85z".to_string() } },
                SidebarIcon::User => rsx! { Icon { path: "M12 12a5 5 0 100-10 5 5 0 000 10zm0 2c-4.42 0-8 2.69-8 6v1h16v-1c0-3.31-3.58-6-8-6z".to_string() } },
                SidebarIcon::Cog => rsx! { Icon { path: "M19.43 12.98c.04-.32.07-.65.07-.98 0-.33-.03-.66-.07-.98l2.11-1.65a.5.5 0 00.12-.64l-2-3.46a.5.5 0 00-.6-.22l-2.49 1a7.03 7.03 0 00-1.69-.98l-.38-2.65A.5.5 0 0014 2h-4a.5.5 0 00-.5.42l-.38 2.65c-.61.25-1.17.58-1.69.98l-2.49-1a.5.5 0 00-.6.22l-2 3.46a.5.5 0 00.12.64l2.11 1.65c-.04.32-.07.65-.07.98 0 .33.03.66.07.98l-2.11 1.65a.5.5 0 00-.12.64l2 3.46c.14.24.43.34.7.22l2.49-1c.52.4 1.08.73 1.69.98l.38 2.65c.04.24.25.42.49.42h4c.24 0 .45-.18.49-.42l.38-2.65c.61-.25 1.17-.58 1.69-.98l2.49 1c.27.12.56.02.7-.22l2-3.46a.5.5 0 00-.12-.64l-2.11-1.65zM12 15.5a3.5 3.5 0 110-7 3.5 3.5 0 010 7z".to_string() } },
            }
        }
    }
}

#[component]
fn Icon(path: String) -> Element {
    rsx! {
        svg {
            width: "18",
            height: "18",
            view_box: "0 0 24 24",
            fill: "currentColor",
            path { d: "{path}" }
        }
    }
}
