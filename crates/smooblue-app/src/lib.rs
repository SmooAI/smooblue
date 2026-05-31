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
pub mod inbox;
pub mod inbox_ingest;
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

    // v1.5.2: re-enabling the macOS NSFilePromise drop overlay (pearl
    // th-78d25c). The v1.5.0 crash was a self-inflicted msg_send! with
    // a snake_case "selector" (e.g. `set_autoresizing_mask:`) that
    // doesn't exist in Cocoa — ObjC threw NSInvalidArgumentException,
    // which propagated as a foreign exception and aborted the process.
    // Fix in file_promise.rs: use the typed objc2-app-kit methods
    // directly, plus defense-in-depth wrappers (catch_unwind +
    // exception::catch + autoreleasepool) so any future bug degrades
    // to a logged-but-running app instead of an aborted one.
    // CI smoke-launch (.github/workflows/ci.yml) verifies launch
    // survives the first 10s on every push, so a repeat of the v1.5.0
    // incident can't land on main.
    use_hook(file_promise::install_on_main_window);

    // Queue of resolved file-promise drops waiting to be picked up by
    // compose. ComposeSheet drains this on every render; if the sheet
    // isn't open when a path arrives, the path stays queued AND we
    // flip compose open so the user sees their dropped image.
    let pending_drops = use_context_provider(|| {
        Signal::new(std::collections::VecDeque::<std::path::PathBuf>::new())
    });

    // Mirror of the AppKit overlay's drag-tracking state — true between
    // draggingEntered and draggingExited (or drop). ComposeSheet ORs
    // this with its native HTML5 dragover signal to drive the yellow
    // textarea highlight, so floater drags get the same visual
    // feedback as Finder drags.
    let promise_drag_active = use_context_provider(|| Signal::new(false));

    // Translate FilePromiseEvent stream from the overlay into the two
    // signals above. Spawned once at App-mount; lives for the app's
    // lifetime. take_receiver returns None if the install didn't
    // initialize the channel (non-macOS or install bailed early).
    use_hook(move || {
        if let Some(mut rx) = file_promise::take_receiver() {
            let mut pending_drops = pending_drops;
            let mut promise_drag_active = promise_drag_active;
            spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        file_promise::FilePromiseEvent::DragEnter => {
                            promise_drag_active.set(true);
                        }
                        file_promise::FilePromiseEvent::DragExit => {
                            promise_drag_active.set(false);
                        }
                        file_promise::FilePromiseEvent::Drop(path) => {
                            promise_drag_active.set(false);
                            pending_drops.write().push_back(path);
                        }
                    }
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
