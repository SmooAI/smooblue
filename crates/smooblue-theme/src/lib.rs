//! Smooblue stylesheet — the shared SmooAI design system plus smooblue's
//! deck-specific component CSS (columns, posts, notification cards, login).
//!
//! Anything cross-app (tokens, buttons, FAB, modal, rail, brand badge,
//! scrollbars, base typography) lives in [`smooai_ui`] and is consumed
//! from there. Anything smooblue-specific lives in `assets/styles.css`
//! next door.

/// CSS string embedded into the Dioxus desktop window. Concatenates
/// [`smooai_ui::STYLES`] (shared tokens + base components) and our
/// smooblue-only component CSS (deck/columns/posts/notifs/login).
///
/// The order matters: the shared sheet defines the tokens (`--foreground`,
/// `--card`, etc) that our component CSS references.
pub const STYLES: &str = concatcp!(smooai_ui::STYLES, "\n", APP_STYLES);

/// Smoo monogram SVG, re-exported from [`smooai_ui::MONOGRAM_SVG`] so
/// every component reaches for the same thing.
pub const MONOGRAM_SVG: &str = smooai_ui::MONOGRAM_SVG;

/// Smooblue-only component CSS — the bits no other app needs.
const APP_STYLES: &str = include_str!("../../../assets/styles.css");

/// Tiny compile-time str concatenation. Avoids pulling `const_format` for
/// this single use.
macro_rules! concatcp {
    ($a:expr, $b:expr, $c:expr) => {{
        const A: &str = $a;
        const B: &str = $b;
        const C: &str = $c;
        const LEN: usize = A.len() + B.len() + C.len();
        const fn cp(out: &mut [u8; LEN]) {
            let a = A.as_bytes();
            let b = B.as_bytes();
            let c = C.as_bytes();
            let mut i = 0;
            while i < a.len() {
                out[i] = a[i];
                i += 1;
            }
            let mut j = 0;
            while j < b.len() {
                out[a.len() + j] = b[j];
                j += 1;
            }
            let mut k = 0;
            while k < c.len() {
                out[a.len() + b.len() + k] = c[k];
                k += 1;
            }
        }
        const OUT: [u8; LEN] = {
            let mut o = [0u8; LEN];
            cp(&mut o);
            o
        };
        // SAFETY: All three inputs are valid UTF-8 (compile-time str literals),
        // and we only copy bytes 1:1 in order, so the concatenated buffer
        // remains valid UTF-8.
        match core::str::from_utf8(&OUT) {
            Ok(s) => s,
            Err(_) => panic!("smooai-ui + app CSS concat produced invalid UTF-8"),
        }
    }};
}
use concatcp;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_tokens_are_present() {
        // Tokens come from smooai-ui, not our local CSS.
        assert!(STYLES.contains("--color-smooai-orange"));
        assert!(STYLES.contains("--color-smooai-green"));
        assert!(STYLES.contains("--color-smooai-dark-blue"));
        assert!(STYLES.contains("--gradient-brand"));
    }

    #[test]
    fn shared_base_components_are_present() {
        // Buttons / FAB / modal / rail / brand-badge come from smooai-ui.
        for cls in [
            ".btn",
            ".btn--primary",
            ".fab",
            ".modal__sheet",
            ".modal__backdrop",
            ".rail",
            ".rail__btn",
            ".brand-badge",
        ] {
            assert!(STYLES.contains(cls), "missing class {cls}");
        }
    }

    #[test]
    fn app_specific_components_are_present() {
        for cls in [".deck-shell", ".deck-column", ".post", ".notif", ".login"] {
            assert!(STYLES.contains(cls), "missing class {cls}");
        }
    }

    #[test]
    fn monogram_re_exports_correctly() {
        assert_eq!(MONOGRAM_SVG, smooai_ui::MONOGRAM_SVG);
        assert!(MONOGRAM_SVG.starts_with("<svg"));
    }
}
