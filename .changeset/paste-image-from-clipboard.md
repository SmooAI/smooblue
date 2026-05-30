---
"smooblue": minor
---

Add ⌘V paste-image-from-clipboard support to the compose sheet. The textarea now intercepts ⌘V / Ctrl+V, reads the clipboard via `arboard`, and if there's an image there, PNG-encodes it and funnels it through the same prep / OCR / LLM-alt-text pipeline the file picker and drag-drop use. The textarea's native text-paste behavior still runs, so pasting plain text works unchanged.

Why this matters: the macOS screenshot floater (the thumbnail bottom-right after a ⇧⌘4 capture) drags an `NSFilePromise` — the file hasn't been written to disk yet, and Wry's `DragDrop` event can't resolve promise items, so dropping the floater onto the compose sheet did nothing. The only escape was clicking the floater to dismiss it, then dragging the saved file from Finder. With paste support, ⌘C the floater (or just paste any image from anywhere) and it attaches directly. Same fix benefits Linux + Windows (paste-from-clipboard is expected UX everywhere, drag wasn't covering it).
