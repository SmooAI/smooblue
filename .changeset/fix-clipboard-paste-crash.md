---
"smooblue": patch
---

Fix a crash when pasting an image into the compose box. The clipboard read ran on a background thread, but macOS's pasteboard is main-thread-only and would trap; the read now happens on the main thread while the heavier PNG encode stays off it.
