---
"smooblue": minor
---

Added an opt-in UI-automation bridge for scripted/headless testing. Set `SMOOBLUE_AUTOMATION=<port>` and the app opens a local (127.0.0.1-only) socket: send a line of JavaScript, get the JSON result back. It runs against the live webview via Dioxus' `document::eval`, so a test script can query elements, click them, read text, and assert state — the primitives UI tests are built from. This is the realistic equivalent of Playwright for a wry app, which can't be driven over Chrome's CDP (WKWebView / WebKitGTK don't speak it). Off by default; bound to localhost; never touches a normal user's run. Note: on macOS the idle Cocoa event loop only services requests when the window receives input — see the module docs for the focus-nudge workaround and the Linux/CI (Xvfb) note.
