//! Opt-in UI-automation bridge for headless / scripted testing.
//!
//! ## Why this exists (and why it isn't "CDP")
//!
//! Smooblue renders through **wry** — WKWebView on macOS, WebKitGTK on
//! Linux. Neither speaks the Chrome DevTools Protocol, so a Playwright /
//! puppeteer client can't attach the way it would to a Chromium tab.
//! What we *do* have is Dioxus' `document::eval`, a Rust↔JS channel into
//! the live webview. This module exposes that channel over a tiny local
//! socket so an external script can drive the real DOM: query elements,
//! click them, read text, assert visibility — the primitives UI tests
//! are built from.
//!
//! ## Protocol
//!
//! Line-oriented over TCP on `127.0.0.1:$SMOOBLUE_AUTOMATION`:
//!   - **Request**: one line of JavaScript (an expression or `;`-joined
//!     statements). No embedded newlines — use `;`.
//!   - **Response**: one line. The JSON-stringified value of the
//!     expression (`document.querySelectorAll('.post').length` → `8`),
//!     or `ERR:<message>` if it threw. Promises are awaited first, so
//!     `fetch(...).then(r=>r.status)` works.
//!
//! Example session (after launching with `SMOOBLUE_AUTOMATION=9223`):
//! ```text
//! $ printf 'document.querySelectorAll(".post").length\n' | nc 127.0.0.1 9223
//! 8
//! $ printf 'document.querySelector(".compose__fab").click()\n' | nc 127.0.0.1 9223
//! null
//! ```
//!
//! ## Safety
//!
//! Off unless `SMOOBLUE_AUTOMATION` is set, and it only ever binds
//! `127.0.0.1` — no remote attack surface for a normal user. It runs
//! arbitrary JS in the app's own webview by design; that's the whole
//! point, and it's gated behind a deliberate env var.
//!
//! ## Known limitation: the idle macOS event loop
//!
//! On macOS the tao/Cocoa run loop **parks when the app is idle** and is
//! only woken by real OS input (mouse / keyboard / focus). dioxus-desktop
//! 0.6 doesn't expose its `EventLoopProxy`, so a background task can't
//! wake it cross-thread, which means a queued eval won't be serviced
//! until the window next receives input. Two practical consequences:
//!
//! - **Drive with the window focused**, or nudge it between commands
//!   (e.g. `osascript -e 'tell application "System Events" to tell
//!   process "smooblue" to set frontmost to true'` plus a synthetic
//!   scroll). A pending eval resolves the instant the loop ticks.
//! - **Headless CI is Linux** (WebKitGTK under Xvfb), whose glib main
//!   loop doesn't have this idle-park behavior, so the bridge services
//!   requests without the nudge there.
//!
//! Fully hands-off macOS automation would need either a dioxus API to
//! wake the loop from another thread, or posting an application-defined
//! `NSEvent` from the socket thread (deliberately not hand-rolled here —
//! that's the same fragile objc surface that crashed `file_promise` in
//! v1.5.0). Tracked as a follow-up.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

/// One eval request from a socket client. `reply` carries the
/// JSON-stringified result (or `ERR:…`) back to the connection task.
pub struct EvalRequest {
    pub code: String,
    pub reply: oneshot::Sender<String>,
}

/// Port to listen on, from `SMOOBLUE_AUTOMATION`. `None` (the default)
/// disables the bridge entirely. A non-numeric value is treated as
/// disabled rather than panicking the app on boot.
pub fn automation_port() -> Option<u16> {
    std::env::var("SMOOBLUE_AUTOMATION")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
}

/// Wrap a line of user JS so it always resolves to a single
/// `dioxus.send(...)` of a JSON string — awaiting a promise result and
/// turning a throw into an `ERR:` string. `undefined` becomes `null` so
/// fire-and-forget actions (e.g. `.click()`) get a clean response.
pub fn wrap_eval(code: &str) -> String {
    // `code` is injected as a JS string literal and run through eval(),
    // so quotes / backslashes are escaped by serde_json — no injection
    // into the surrounding template.
    let lit = serde_json::to_string(code).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "(function(){{try{{var __r=eval({lit});\
         Promise.resolve(__r)\
         .then(function(v){{dioxus.send(JSON.stringify(v===undefined?null:v))}})\
         .catch(function(e){{dioxus.send('ERR:'+e)}});}}\
         catch(e){{dioxus.send('ERR:'+e);}}}})()"
    )
}

/// Accept connections and forward each request line to `tx`. Runs until
/// the process exits. Bind failures are logged and the task ends (the
/// app keeps running without the bridge).
pub async fn serve(port: u16, tx: mpsc::UnboundedSender<EvalRequest>) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(error = %e, port, "automation: failed to bind — bridge disabled");
            return;
        }
    };
    tracing::info!(port, "automation: eval bridge listening on 127.0.0.1");
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "automation: accept failed");
                continue;
            }
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let code = line.trim();
                if code.is_empty() {
                    continue;
                }
                tracing::debug!(code = %code, "automation: queuing eval");
                let (reply_tx, reply_rx) = oneshot::channel();
                if tx
                    .send(EvalRequest {
                        code: code.to_string(),
                        reply: reply_tx,
                    })
                    .is_err()
                {
                    break; // dioxus-side drain gone → app shutting down
                }
                let result = reply_rx
                    .await
                    .unwrap_or_else(|_| "ERR:eval channel closed".to_string());
                if write_half
                    .write_all(format!("{result}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_parses_only_valid_values() {
        std::env::remove_var("SMOOBLUE_AUTOMATION");
        assert_eq!(automation_port(), None);
        std::env::set_var("SMOOBLUE_AUTOMATION", "9223");
        assert_eq!(automation_port(), Some(9223));
        std::env::set_var("SMOOBLUE_AUTOMATION", "  9223 ");
        assert_eq!(automation_port(), Some(9223));
        std::env::set_var("SMOOBLUE_AUTOMATION", "nope");
        assert_eq!(automation_port(), None);
        std::env::set_var("SMOOBLUE_AUTOMATION", "0");
        assert_eq!(automation_port(), None);
        std::env::remove_var("SMOOBLUE_AUTOMATION");
    }

    #[test]
    fn wrap_eval_escapes_and_wraps() {
        let w = wrap_eval(r#"document.querySelector(".x \"y\"")"#);
        // The user code is embedded as a JSON string literal, so the
        // inner quotes are backslash-escaped and can't break out.
        assert!(w.contains(r#"eval("document.querySelector(\".x \\\"y\\\"\")")"#));
        assert!(w.contains("dioxus.send"));
        assert!(w.contains("JSON.stringify"));
    }

    #[test]
    fn wrap_eval_handles_empty() {
        let w = wrap_eval("");
        assert!(w.contains("eval(\"\")"));
    }
}
