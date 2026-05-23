//! Curated wrapper around `dioxus-free-icons`'s Lucide pack.
//!
//! Everything in the UI that draws a glyph goes through here so we can
//! keep stroke width, size, and color consistent. No emojis anywhere.
//!
//! Each icon is a tiny standalone component because `Icon<T>`'s generic
//! parameter doesn't propagate cleanly through `macro_rules!` (the rsx
//! macro re-parses the body and loses the type info). Repetition is the
//! price of compile-time-checked icon names.

use dioxus::prelude::*;
use dioxus_free_icons::{icons::ld_icons, Icon};

/// Three standard sizes — anything else and we're being inconsistent.
#[derive(Clone, Copy, PartialEq)]
pub enum Size {
    /// 14px — inline with text (post actions, header buttons)
    Sm,
    /// 18px — sidebar nav + column header icon
    Md,
    /// 22px — login screen + larger affordances
    Lg,
}

impl Size {
    fn px(self) -> u32 {
        match self {
            Self::Sm => 14,
            Self::Md => 18,
            Self::Lg => 22,
        }
    }
}

// ── Sidebar navigation ──

#[component]
pub fn Home(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdHome } }
}

#[component]
pub fn Search(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdSearch } }
}

#[component]
pub fn Bell(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdBell } }
}

#[component]
pub fn Compass(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdCompass } }
}

#[component]
pub fn User(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdUser } }
}

#[component]
pub fn Settings(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdSettings } }
}

#[component]
pub fn Plus(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdPlus } }
}

// ── Column header ──

#[component]
pub fn GripVertical(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdGripVertical } }
}

#[component]
pub fn ListFilter(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdListFilter } }
}

#[component]
pub fn Settings2(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdSettings2 } }
}

// ── Post actions ──

#[component]
pub fn MessageCircle(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdMessageCircle } }
}

#[component]
pub fn Repeat2(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdRepeat2 } }
}

#[component]
pub fn Heart(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdHeart } }
}

#[component]
pub fn MoreHorizontal(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdEllipsis } }
}

// ── Notification reasons ──

#[component]
pub fn UserPlus(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdUserPlus } }
}

#[component]
pub fn AtSign(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdAtSign } }
}

#[component]
pub fn Quote(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdQuote } }
}

#[component]
pub fn Package(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdPackage } }
}
