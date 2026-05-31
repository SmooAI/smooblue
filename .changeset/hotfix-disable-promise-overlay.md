---
"smooblue": patch
---

**Hotfix**: disable the v1.5.0 file-promise drop overlay — it crashed Smooblue on launch with `__rust_foreign_exception` → `abort()`. My AppKit code in `file_promise::install_on_main_window` threw an ObjC exception during initial render that propagated through Dioxus' component creation path and aborted the Rust runtime. Module stays compiled and wired so a proper fix can land without a wider revert. Tracked as th-78d25c.

This reverts the user-visible behavior to v1.4.1 (paste works, drag-from-Finder works, screenshot-floater drag still requires ⌘C → ⌘V workaround). The proper fix needs GUI-test verification before reshipping — I shouldn't have shipped v1.5.0 without it.
