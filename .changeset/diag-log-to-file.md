---
"smooblue": patch
---

Add file-based diagnostic logging. macOS's unified log drops plain `eprintln!`/stderr from GUI apps launched via Finder — only `os_log`/`NSLog` make it through. That left remote debugging stuck unless the user relaunched from terminal.

Now every diagnostic line (currently from the inbox ingestion task; more sites to convert opportunistically) appends to `directories::data_dir/smooblue/diag.log`, rotated when it crosses 1 MB. Still mirrors to stderr so terminal launches show output too. Per-line write is open-append-close behind a parking_lot mutex — safe across threads, no buffer that loses content on crash.

Practical effect: when the inbox shows empty for a user, we can ask them to `cat ~/Library/Application\ Support/ai.smoo.smooblue/diag.log | tail` and immediately see whether `pages=N ingested=M` or `listNotifications failed: …` is in the log.
