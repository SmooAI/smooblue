//! Smooblue UI library — the binary in `main.rs` is a thin entry that
//! constructs the Dioxus window and calls [`App`]. Everything testable
//! (components, view models, persistence helpers) lives here.

pub mod alt_text;
pub mod analytics;
pub mod analytics_ingest;
pub mod analytics_score;
pub mod auth_refresh;
pub mod automation;
pub mod components;
pub mod demo;
pub mod diag_log;
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

    // Accessibility — font_scale + column_width. Loaded once from
    // disk; mutated by Cmd+= / Cmd+- / Cmd+0, Cmd+wheel, and the
    // Settings sliders. Applied as CSS custom properties on the
    // document root so every text rule (which now uses
    // `calc(Npx * var(--font-scale))`) reflows naturally. Earlier
    // v1.12.0 used the `zoom` property — works but breaks scroll
    // and clips content past the viewport. Pearl th-459511.
    let ui_prefs = use_context_provider(|| Signal::new(crate::persistence::load_ui_prefs()));
    use_effect(move || {
        let prefs = ui_prefs.read().clone();
        let js = format!(
            r#"
            const html = document.documentElement;
            html.style.setProperty("--font-scale", "{}");
            html.style.setProperty("--column-width", "{}px");
            "#,
            prefs.font_scale, prefs.column_width_px,
        );
        let _ = dioxus::document::eval(&js);
        spawn(async move {
            let _ = tokio::task::spawn_blocking(move || crate::persistence::save_ui_prefs(&prefs))
                .await;
        });
    });

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

    // Inbox ingestion task. Polls listNotifications + listConvos every
    // 30s, classifies + upserts into the inbox SQLite store so the
    // Inbox column has triage rows to read. Spawned ONCE at App-mount
    // and lives the lifetime of the process. Re-evaluates session
    // freshness internally via fresh_client so OAuth-token rotation
    // is transparent. v1.11-v1.14 shipped the ingester but forgot to
    // call this — silent empty-inbox until now (pearl: filed below).
    use_hook(move || {
        crate::inbox_ingest::spawn_ingestion_task(session);
    });

    // Account-analytics background tasks. Mirrors the inbox ingester:
    // spawned ONCE at App-mount, lives the process lifetime, re-derives
    // auth via fresh_client each cycle. Runs the daily metric snapshot
    // and the one-time resumable backfill phase machine (posts /
    // following / followers / engagement → complete). Guarded internally
    // on a present session + non-demo mode; the backfill resumes from its
    // last checkpoint after a restart and is a no-op once complete (the
    // daily snapshot keeps ticking).
    use_hook(move || {
        crate::analytics_ingest::spawn_analytics_tasks(session);
    });

    // Opt-in UI-automation bridge (SMOOBLUE_AUTOMATION=<port>). Off by
    // default. When set, a local socket accepts lines of JS and runs
    // them against the live webview via document::eval — the closest
    // thing to Playwright for a wry app (which can't be driven over
    // CDP). The TCP listener runs on a plain tokio task; the eval drain
    // loop runs via dioxus `spawn` so it's inside the runtime that
    // `document::eval` requires.
    //
    // `automation_kick` exists only to wake the event loop. On macOS a
    // fully-parked Cocoa run loop won't process the eval IPC from a
    // background thread (request_redraw cross-thread is unreliable
    // there), but a Signal write goes through the dioxus scheduler's
    // EventLoopProxy waker — the same path the inbox poll uses to update
    // the idle UI — which reliably wakes the loop. App reads it below so
    // the write actually schedules a re-render.
    let mut automation_kick = use_signal(|| 0u64);
    use_hook(move || {
        if let Some(port) = crate::automation::automation_port() {
            let (tx, mut rx) =
                tokio::sync::mpsc::unbounded_channel::<crate::automation::EvalRequest>();
            tokio::spawn(crate::automation::serve(port, tx));
            spawn(async move {
                while let Some(req) = rx.recv().await {
                    let mut eval = dioxus::document::eval(&crate::automation::wrap_eval(&req.code));
                    // The tao event loop parks when the app is idle and
                    // won't pump the eval IPC round-trip without input
                    // (confirmed: a pending eval only resolved once the
                    // window was focused/scrolled). Poke a redraw every
                    // frame until the result lands, with a hard timeout
                    // so a wedged eval can't spin forever.
                    let recv_fut = eval.recv::<String>();
                    tokio::pin!(recv_fut);
                    let deadline = tokio::time::sleep(std::time::Duration::from_secs(5));
                    tokio::pin!(deadline);
                    let result = loop {
                        tokio::select! {
                            r = &mut recv_fut => {
                                break r.unwrap_or_else(|e| format!("ERR:eval failed: {e}"));
                            }
                            _ = &mut deadline => break "ERR:eval timed out".to_string(),
                            _ = tokio::time::sleep(std::time::Duration::from_millis(16)) => {
                                // Bump the kick signal → scheduler wakes
                                // the event loop → poll_vdom pumps the
                                // eval IPC round-trip.
                                automation_kick.with_mut(|k| *k = k.wrapping_add(1));
                            }
                        }
                    };
                    let _ = req.reply.send(result);
                }
            });
        }
    });

    rsx! {
        style { "{STYLES}" }
        script { "{INLINE_VIDEO_AUTOPAUSE_JS}" }
        div {
            id: "main",
            // Subscribes App to the automation kick signal so its writes
            // from the eval drain loop schedule a re-render (and thus
            // wake the event loop). No-op visually. `0` when the bridge
            // is disabled.
            "data-automation-kick": "{automation_kick}",
            if session.read().is_some() {
                components::deck::DeckShell {}
            } else {
                views::login::LoginView {}
            }
        }
    }
}
