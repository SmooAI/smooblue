---
"smooblue": minor
---

Re-enable the macOS NSFilePromise drop overlay (pearl th-78d25c). The v1.5.0 crash root cause was a self-inflicted msg_send! with snake_case "selectors" (`set_autoresizing_mask:`, `register_for_dragged_types:`) that don't exist in Cocoa — ObjC threw `NSInvalidArgumentException: unrecognized selector sent to instance`, which propagated through Rust's FFI boundary as a foreign exception and aborted the process before the first frame rendered. Two-part fix in `file_promise.rs`:

1. **Use the typed objc2-app-kit methods directly** (`setAutoresizingMask`, `registerForDraggedTypes`, `addSubview_positioned_relativeTo`) instead of hand-rolled msg_send wrappers — the bindings know the real Cocoa selectors.
2. **Defense in depth**: wrap install in `std::panic::catch_unwind` + `objc2::exception::catch` + `objc2::rc::autoreleasepool`, so any future installation bug degrades to a logged error rather than aborting the process. Every step `eprintln!`s its progress, so the CI smoke-launch job + user terminal launches expose exactly where any future install failure happens (the previous `tracing::warn!` calls were silent because Smooblue has no tracing subscriber).

Behavior for the user: screenshot floater drag onto the Smooblue window should now resolve the promise via `NSFilePromiseReceiver.receivePromisedFilesAtDestination:`, write the image to the OS temp dir, open compose with it attached. The new CI smoke-launch job verifies launch survives before this ships.
