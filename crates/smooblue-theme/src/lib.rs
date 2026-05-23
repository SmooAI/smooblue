//! Smoo color tokens exported as a CSS string.
//!
//! Values mirror `packages/ui/globals.css` — the canonical smoo design system.
//! When `globals.css` changes, regenerate by copying the relevant `--color-*`
//! lines here so the desktop app and the web app stay visually aligned.

/// The complete stylesheet embedded into the Dioxus desktop window.
///
/// Includes:
/// - smoo color tokens (palette + semantic mappings)
/// - reset / base typography
/// - deck shell layout (left rail + horizontal column scroller)
/// - column / post / sidebar / avatar / button components
pub const STYLES: &str = include_str!("../../../assets/styles.css");

/// Tailwind-style semantic color names used across components.
pub mod color {
    // Brand palette
    pub const SMOO_ORANGE: &str = "var(--color-smooai-orange)";
    pub const SMOO_RED: &str = "var(--color-smooai-red)";
    pub const SMOO_GREEN: &str = "var(--color-smooai-green)";
    pub const SMOO_BLUE_400: &str = "var(--color-smooai-blue-400)";
    pub const SMOO_DARK_BLUE: &str = "var(--color-smooai-dark-blue)";
    pub const SMOO_DARK_BLUE_850: &str = "var(--color-smooai-dark-blue-850)";

    // Semantic
    pub const BG: &str = "var(--background)";
    pub const FG: &str = "var(--foreground)";
    pub const CARD: &str = "var(--card)";
    pub const BORDER: &str = "var(--border)";
    pub const MUTED_FG: &str = "var(--muted-foreground)";
    pub const SIDEBAR_BG: &str = "var(--sidebar)";
    pub const SIDEBAR_BORDER: &str = "var(--sidebar-border)";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_contains_smoo_tokens() {
        assert!(
            STYLES.contains("--color-smooai-orange"),
            "missing smoo orange token"
        );
        assert!(
            STYLES.contains("--color-smooai-dark-blue"),
            "missing smoo dark blue token"
        );
        assert!(
            STYLES.contains("--color-smooai-green"),
            "missing smoo green token"
        );
    }

    #[test]
    fn stylesheet_defines_deck_layout() {
        assert!(STYLES.contains(".deck-shell"), "missing deck shell layout");
        assert!(STYLES.contains(".deck-column"), "missing column layout");
        assert!(STYLES.contains(".deck-sidebar"), "missing sidebar layout");
    }
}
