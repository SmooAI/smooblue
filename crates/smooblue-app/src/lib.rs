//! Smooblue UI library — the binary in `main.rs` is a thin entry that
//! constructs the Dioxus window and calls [`App`]. Everything testable
//! (components, view models, persistence helpers) lives here.

pub mod alt_text;
pub mod auth_refresh;
pub mod components;
pub mod demo;
pub mod file_promise;
pub mod icons;
pub mod image_prep;
pub mod keyboard;
pub mod ocr;
pub mod persistence;
pub mod safe_open;
pub mod state;
pub mod updates;
pub mod views;

use dioxus::prelude::*;
use smooblue_theme::STYLES;

/// JS that pauses any inline `<video>` once it scrolls mostly out of
/// view, and resumes it (if it was playing when it left) when it
/// scrolls back in. Without this, a user who clicks play on an
/// inline embed and then scrolls keeps hearing audio from a video
/// they can no longer see — and there's no obvious way to find it
/// again short of refreshing the column.
///
/// Pure DOM + IntersectionObserver + MutationObserver, no Rust ↔ JS
/// round-trips. Runs once on first mount thanks to the
/// `data-smooblue-video-observer-installed` flag.
const INLINE_VIDEO_AUTOPAUSE_JS: &str = r#"
(function() {
    if (window.__smooblueVideoObserverInstalled) return;
    window.__smooblueVideoObserverInstalled = true;

    // Track which videos were playing when they scrolled out so we
    // can decide whether to resume on re-entry. Don't auto-resume
    // for videos the user explicitly paused.
    const wasPlaying = new WeakSet();

    const io = new IntersectionObserver((entries) => {
        for (const entry of entries) {
            const v = entry.target;
            if (entry.intersectionRatio < 0.25) {
                if (!v.paused) {
                    wasPlaying.add(v);
                    v.pause();
                }
            } else if (entry.intersectionRatio > 0.6 && wasPlaying.has(v)) {
                wasPlaying.delete(v);
                // play() returns a promise; ignore the rejection that
                // happens when the user has navigated again before it
                // resolves.
                v.play().catch(() => {});
            }
        }
    }, { threshold: [0, 0.25, 0.6, 1.0] });

    const observe = (root) => {
        root.querySelectorAll('video').forEach((v) => {
            if (!v.__smooblueObserved) {
                v.__smooblueObserved = true;
                io.observe(v);
            }
        });
    };

    // Observe existing videos + newly mounted ones (Dioxus mounts
    // them as feeds load + scroll-extend).
    observe(document);
    const mo = new MutationObserver((mutations) => {
        for (const m of mutations) {
            for (const node of m.addedNodes) {
                if (node.nodeType === 1) {
                    if (node.tagName === 'VIDEO') {
                        if (!node.__smooblueObserved) {
                            node.__smooblueObserved = true;
                            io.observe(node);
                        }
                    } else if (node.querySelectorAll) {
                        observe(node);
                    }
                }
            }
        }
    });
    mo.observe(document.body, { childList: true, subtree: true });
})();
"#;

/// Top-level Dioxus component.
///
/// Branches on the persisted session:
/// - `Some(session)` → render the deck shell with the Home column
/// - `None` → render the login view
#[component]
pub fn App() -> Element {
    // Bootstrap global state on first render.
    state::use_bootstrap();

    // Install the macOS NSFilePromiseProvider drop overlay on the main
    // window so screenshot-floater drags resolve into compose. No-op
    // on other platforms (linux/windows use the standard drag path).
    // Runs exactly once for the lifetime of the App component, which
    // is the lifetime of the app itself.
    use_hook(file_promise::install_on_main_window);

    // Queue of resolved file-promise drops waiting to be picked up by
    // compose. ComposeSheet drains this on every render; if the sheet
    // isn't open when a path arrives, the path stays queued AND we
    // flip compose open so the user sees their dropped image.
    let pending_drops = use_context_provider(|| {
        Signal::new(std::collections::VecDeque::<std::path::PathBuf>::new())
    });

    // Drain the file-promise receiver into the queue. Spawned once at
    // App-mount on the tokio runtime; lives for the app's lifetime.
    // Take-once semantics on the receiver guard against double-spawn.
    use_hook(move || {
        if let Some(mut rx) = file_promise::take_receiver() {
            let mut pending_drops = pending_drops;
            spawn(async move {
                while let Some(path) = rx.recv().await {
                    pending_drops.write().push_back(path);
                }
            });
        }
    });

    let session = use_context::<Signal<Option<smooblue_oauth::Session>>>();

    rsx! {
        style { "{STYLES}" }
        script { "{INLINE_VIDEO_AUTOPAUSE_JS}" }
        div {
            id: "main",
            if session.read().is_some() {
                components::deck::DeckShell {}
            } else {
                views::login::LoginView {}
            }
        }
    }
}
