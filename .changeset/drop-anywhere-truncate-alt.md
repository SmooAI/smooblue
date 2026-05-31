---
"smooblue": minor
---

**Drag an image anywhere onto the Smooblue window** and compose opens with it attached. Previously only the compose sheet's textarea accepted drops — anywhere else on the window was a no-op. Now the deck-shell root has its own drop handler that routes accepted images through the same `FilePromiseEvent::Drop` channel the screenshot-floater overlay uses, so the App-level listener opens compose + attaches in the same flow regardless of where the drop landed. Compose's own drop handler now calls `stop_propagation` so drops landing INSIDE the open sheet don't double-attach.

**Alt-text now truncates to 2000 characters** (Bluesky's `app.bsky.embed.images#image.alt` lexicon cap). The LLM auto-suggestion path was producing 3-4k-char scene descriptions that would have been rejected at submit time with a validation error; we now cap proactively in `AttachedImage::computed_alt`, in both alt-input `oninput` handlers (image + video), and via a `maxlength="2000"` on the textarea so user typing is also bounded.
