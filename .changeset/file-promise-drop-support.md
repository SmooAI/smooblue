---
"smooblue": minor
---

Add macOS file-promise drop support — the screenshot floater (the thumbnail bottom-right after ⇧⌘4) can now be dragged directly onto the Smooblue window to attach to a post. Previously this did nothing because Wry's drag handler can only resolve `public.file-url` items, and the floater drags an unresolved `NSFilePromiseProvider` (the file doesn't exist on disk until the floater dismisses).

The fix attaches an invisible overlay NSView to the main window's content view, registered for `com.apple.NSFilePromiseProvider` drag types **only** — so existing drag UX (Finder file drops, web-content drag/drop) is untouched. When a promise drop lands, the overlay resolves the file via `NSFilePromiseReceiver.receivePromisedFilesAtDestination:` into the OS temp directory and forwards the path through a tokio channel into the same image-attachment pipeline that drag-drop and the file picker use. If you drop while the compose sheet is closed, it flips open with the image attached. Linux + Windows: no-op stub (paths there don't go through NSFilePromise).

Pearl: th-2c71e1.
