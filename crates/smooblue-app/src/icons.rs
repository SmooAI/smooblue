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

#[component]
pub fn X(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdX } }
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

/// Distinct "compose-a-quote" affordance — different from the bare
/// quotation-marks Quote icon (which we use for the *quote-count*
/// affordance). Pairs with PostCard's "Quote this post" button.
#[component]
pub fn MessageQuote(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdMessageSquareQuote } }
}

#[component]
pub fn Package(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdPackage } }
}

#[component]
pub fn ImageIcon(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdImage } }
}

#[component]
pub fn Sparkles(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdSparkles } }
}

#[component]
pub fn Users(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdUsers } }
}

#[component]
pub fn Bookmark(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdBookmark } }
}

#[component]
pub fn LogOut(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdLogOut } }
}

#[component]
pub fn VolumeOff(size: Size) -> Element {
    let px = size.px();
    // Lucide's "muted" icon is LdVolumeX (the volume symbol with an
    // ×). The pack doesn't ship a "VolumeOff" variant.
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdVolumeX } }
}

#[component]
pub fn Volume(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdVolume2 } }
}

#[component]
pub fn Ban(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdBan } }
}

/// Small relative-time text ("11s", "1h", "3d") that updates every
/// second by subscribing to the global `Tick` signal. Lifted out of
/// PostCard / NotificationCard so the 1Hz tick re-renders only this
/// tiny text node instead of every full card — a 500-post Home
/// column otherwise burns 100% CPU just keeping the timestamps
/// fresh (measured 2026-05-24 in scale=large demo mode).
#[component]
pub fn TimeAgo(text_at_render: String, source_ts: Option<String>) -> Element {
    use crate::state::Tick;
    use dioxus::prelude::*;
    let _tick = use_context::<Signal<Tick>>().read().0;
    // Re-compute relative time from the canonical timestamp on each
    // tick. Falls back to the value the parent passed at render time
    // if the timestamp is missing (e.g., notifications without
    // indexedAt) — that's stable but stale, which is fine.
    let rendered = match source_ts.as_deref() {
        Some(s) => relative_time_from(s).unwrap_or(text_at_render),
        None => text_at_render,
    };
    rsx! { span { "{rendered}" } }
}

/// Compact "Ns/Nm/Nh/Nd/Nmo" formatting matching PostView::relative_time.
/// Lives here so the TimeAgo helper above doesn't have to cross
/// crate boundaries to compute its own text.
fn relative_time_from(rfc3339: &str) -> Option<String> {
    let ts = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let now = chrono::Utc::now();
    let delta = now.signed_duration_since(ts.with_timezone(&chrono::Utc));
    let out = if delta.num_seconds() < 60 {
        format!("{}s", delta.num_seconds().max(0))
    } else if delta.num_minutes() < 60 {
        format!("{}m", delta.num_minutes())
    } else if delta.num_hours() < 24 {
        format!("{}h", delta.num_hours())
    } else if delta.num_days() < 30 {
        format!("{}d", delta.num_days())
    } else {
        format!("{}mo", delta.num_days() / 30)
    };
    Some(out)
}

#[component]
pub fn Play(size: Size) -> Element {
    let px = size.px();
    rsx! { Icon { width: px, height: px, fill: "currentColor", icon: ld_icons::LdPlay } }
}
