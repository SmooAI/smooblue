//! macOS file-promise drop handling — screenshot-floater drag fix.
//!
//! The macOS screenshot floater (the thumbnail that appears bottom-right
//! after ⇧⌘4) drags an `NSFilePromiseProvider` rather than a
//! `public.file-url` — because the file doesn't exist on disk until the
//! floater dismisses. Wry's `WKWebView` only translates `public.file-url`
//! pasteboard items into HTML5 `DataTransfer.files`, so dropping the
//! floater onto the compose sheet did nothing in v1.4.1 and earlier.
//!
//! Fix: at app launch, attach an invisible overlay `NSView` to the
//! window's content view, registered for `com.apple.NSFilePromiseProvider`
//! drag types **only**. Finder file drops (which carry `public.file-url`
//! but NOT the promise type) don't match our registered types and fall
//! through to the WKWebView untouched — existing drag UX is unaffected.
//! Floater drops (which carry the promise type) hit our overlay; we
//! resolve them via `NSFilePromiseReceiver.receivePromisedFiles…` and
//! forward the resolved file path through a tokio channel to the
//! compose sheet, which feeds it into the same image-attachment
//! pipeline as drag-drop and the file picker.
//!
//! ## Defense in depth (v1.5.2 lesson)
//!
//! v1.5.0 shipped this with hand-rolled `msg_send!` wrappers that used
//! Rust snake_case as the selector (e.g. `set_autoresizing_mask:`
//! instead of the real Cocoa selector `setAutoresizingMask:`). ObjC
//! threw `NSInvalidArgumentException: unrecognized selector sent to
//! instance` — which propagates through the Rust FFI boundary as a
//! foreign exception and aborts the process. The whole app crashed
//! at launch before the first frame.
//!
//! Three layers of guard now:
//! 1. `std::panic::catch_unwind` swallows Rust panics from inside
//!    `install_on_main_window` so an unexpected `.expect()` doesn't
//!    take the app down.
//! 2. `objc2::exception::catch` swallows ObjC exceptions and converts
//!    them to a logged error rather than a foreign-exception abort.
//! 3. `objc2::rc::autoreleasepool` provides a pool for the install
//!    work — Cocoa code on the main thread normally has one from the
//!    run loop, but at component-render time we may be running before
//!    the run loop's pool is established, leaving autoreleased objects
//!    with nowhere to drain.
//!
//! Also: every step `eprintln!`s its progress to stderr. The CI
//! smoke-launch job captures stderr, so if the install ever fails
//! again the log shows exactly which line. `tracing::warn!` etc.
//! would be silent — Smooblue doesn't install a tracing subscriber.
//!
//! Non-macOS builds: stubs that no-op.

use std::path::PathBuf;

/// Events emitted from the AppKit drag-destination overlay to the
/// Dioxus listener. Keeps drag-over UI state (the yellow textarea
/// highlight) in sync with the AppKit drag-tracking lifecycle —
/// otherwise the overlay would intercept the drag before the
/// compose textarea's on_dragover handler could fire, and the user
/// would get no visual feedback during a floater drag.
#[derive(Debug, Clone)]
pub enum FilePromiseEvent {
    /// Drag with a registered promise type entered our overlay.
    /// Open compose (if closed) + light up the over-highlight.
    DragEnter,
    /// Drag left the overlay without dropping. Clear the highlight;
    /// keep compose open (closing on drag-away would be intrusive).
    DragExit,
    /// Drop completed; promise resolved to this file path. Clear
    /// the highlight and attach the image.
    Drop(PathBuf),
}

#[cfg(target_os = "macos")]
pub use macos::{install_on_main_window, take_receiver};

#[cfg(not(target_os = "macos"))]
pub fn install_on_main_window() {}

#[cfg(not(target_os = "macos"))]
pub fn take_receiver() -> Option<tokio::sync::mpsc::UnboundedReceiver<FilePromiseEvent>> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use super::FilePromiseEvent;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use std::ptr::NonNull;
    use std::sync::OnceLock;

    use block2::RcBlock;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyObject, Bool, ProtocolObject};
    use objc2::{define_class, msg_send, ClassType, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSAutoresizingMaskOptions, NSDragOperation, NSDraggingInfo,
        NSFilePromiseReceiver, NSView, NSWindowOrderingMode,
    };
    use objc2_foundation::{
        ns_string, MainThreadMarker, NSArray, NSDictionary, NSError, NSOperationQueue, NSPoint,
        NSRect, NSString, NSURL,
    };
    use parking_lot::Mutex;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

    /// Channel between the AppKit overlay callback (main thread, ObjC
    /// runtime) and the Dioxus listener (tokio task). Carries the
    /// drag-state lifecycle plus the resolved drop path, so the
    /// Dioxus side can keep visual state in sync (compose highlight)
    /// alongside attaching images.
    static SENDER: OnceLock<UnboundedSender<FilePromiseEvent>> = OnceLock::new();
    static RECEIVER: Mutex<Option<UnboundedReceiver<FilePromiseEvent>>> = Mutex::new(None);

    /// Guard against double-install. The overlay is per-window and the
    /// channel is global; both should initialize exactly once.
    static INSTALLED: OnceLock<()> = OnceLock::new();

    /// Install the file-promise drop overlay on the main NSWindow's
    /// content view. Always safe to call — wraps all real work in
    /// panic + ObjC-exception guards so a buggy installation degrades
    /// to a logged error rather than an aborted process.
    pub fn install_on_main_window() {
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(install_inner));
        if let Err(panic) = outcome {
            // Best-effort string from the panic payload — works for
            // both `&str` and `String` panics, which covers most of
            // the Rust std uses of panic!.
            let msg = panic_payload_message(&panic);
            eprintln!("[file_promise] install panicked: {msg}");
        }
    }

    /// Take the receiver for incoming promised paths. Called once at
    /// App's first render; the listener task then pushes paths into
    /// the attachments-pipeline queue.
    pub fn take_receiver() -> Option<UnboundedReceiver<FilePromiseEvent>> {
        RECEIVER.lock().take()
    }

    /// Best-effort fire an event to the Dioxus listener. Drops the
    /// event silently if the channel hasn't been initialized yet
    /// (overlay install would have warned about that path).
    fn emit(event: FilePromiseEvent) {
        if let Some(tx) = SENDER.get() {
            let _ = tx.send(event);
        }
    }

    fn install_inner() {
        if INSTALLED.set(()).is_err() {
            return;
        }
        eprintln!("[file_promise] install starting");

        let (tx, rx) = unbounded_channel();
        let _ = SENDER.set(tx);
        *RECEIVER.lock() = Some(rx);
        eprintln!("[file_promise] channel ready");

        let Some(mtm) = MainThreadMarker::new() else {
            eprintln!("[file_promise] not on main thread — skipping overlay install");
            return;
        };
        eprintln!("[file_promise] on main thread, attaching overlay");

        // Drain autoreleased AppKit allocations + catch ObjC exceptions
        // so we never abort the process. autoreleasepool first because
        // it has the wider lifetime; exception::catch inside it.
        autoreleasepool(|_| {
            // SAFETY: `install_appkit` is `unsafe` because it dispatches
            // ObjC messages; correctness is the function's
            // responsibility, not ours here. exception::catch is also
            // `unsafe` because the closure can raise ObjC exceptions
            // (that's the whole point — it's the catch site).
            let result =
                unsafe { objc2::exception::catch(AssertUnwindSafe(|| install_appkit(mtm))) };
            match result {
                Ok(()) => eprintln!("[file_promise] overlay install complete"),
                Err(exc) => {
                    // Exception's Debug impl renders class + reason
                    // which is the most useful thing for a stderr log.
                    let desc = match exc {
                        Some(e) => format!("{e:?}"),
                        None => "<no exception object>".to_string(),
                    };
                    eprintln!("[file_promise] ObjC exception during install: {desc}");
                }
            }
        });
    }

    /// Concrete AppKit setup. Separated so the `unsafe` block is small
    /// and the `exception::catch` site is clean. Marked `unsafe fn`
    /// because every objc dispatch is unsafe.
    unsafe fn install_appkit(mtm: MainThreadMarker) {
        eprintln!("[file_promise] sharedApplication");
        let app = NSApplication::sharedApplication(mtm);

        eprintln!("[file_promise] querying windows");
        let windows = app.windows();
        let window_count = windows.len();
        eprintln!("[file_promise] window count: {window_count}");

        let Some(window) = windows.iter().next() else {
            eprintln!("[file_promise] no NSWindow at install time — skipping");
            return;
        };

        eprintln!("[file_promise] getting contentView");
        let Some(content_view) = window.contentView() else {
            eprintln!("[file_promise] NSWindow has no contentView — skipping");
            return;
        };

        let bounds = content_view.bounds();
        eprintln!(
            "[file_promise] contentView bounds: {}x{}",
            bounds.size.width, bounds.size.height
        );

        eprintln!("[file_promise] allocating PromiseDropView");
        let overlay: Retained<PromiseDropView> = PromiseDropView::new(mtm, bounds);

        eprintln!("[file_promise] setAutoresizingMask");
        overlay.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        // Register for promise drag types ONLY. Finder drops carry
        // public.file-url which we DON'T register for, so they fall
        // through to the WKWebView untouched.
        eprintln!("[file_promise] building promise drag-type array");
        let promise_types = NSArray::from_slice(&[
            ns_string!("com.apple.NSFilePromiseProvider"),
            // Legacy alias — still emitted by some apps' DnD code.
            ns_string!("com.apple.pasteboard.promised-file-url"),
        ]);

        eprintln!("[file_promise] registerForDraggedTypes");
        overlay.registerForDraggedTypes(&promise_types);

        eprintln!("[file_promise] addSubview (NSWindowAbove)");
        content_view.addSubview_positioned_relativeTo(&overlay, NSWindowOrderingMode::Above, None);

        eprintln!("[file_promise] overlay installed; listening for NSFilePromiseProvider drops");
    }

    /// Pull a printable message out of a panic payload. Covers the
    /// common case of `panic!("…")` (str / String); falls back to a
    /// generic note for opaque payloads.
    fn panic_payload_message(panic: &Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = panic.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = panic.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        }
    }

    define_class!(
        /// Invisible NSView overlay that intercepts NSFilePromiseProvider
        /// drops. Returns NSDragOperationCopy for the promise types it
        /// was registered for; the AppKit drag system never invokes our
        /// callbacks for other types, so we don't need any "is this
        /// for us?" filtering.
        ///
        /// hitTest: returns nil so normal mouse events (clicks, scroll,
        /// drag-selection) pass straight through to the WKWebView
        /// underneath.
        #[unsafe(super(NSView))]
        #[name = "SmooBluePromiseDropView"]
        struct PromiseDropView;

        impl PromiseDropView {
            #[unsafe(method(hitTest:))]
            fn hit_test(&self, _point: NSPoint) -> *mut NSView {
                std::ptr::null_mut()
            }

            #[unsafe(method(draggingEntered:))]
            fn dragging_entered(
                &self,
                _sender: &ProtocolObject<dyn NSDraggingInfo>,
            ) -> NSDragOperation {
                eprintln!("[file_promise] draggingEntered (promise type detected)");
                emit(FilePromiseEvent::DragEnter);
                NSDragOperation::Copy
            }

            #[unsafe(method(draggingUpdated:))]
            fn dragging_updated(
                &self,
                _sender: &ProtocolObject<dyn NSDraggingInfo>,
            ) -> NSDragOperation {
                NSDragOperation::Copy
            }

            #[unsafe(method(draggingExited:))]
            fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
                // Drag left without dropping. Clear the highlight so
                // the textarea doesn't get stuck in "over" state.
                eprintln!("[file_promise] draggingExited");
                emit(FilePromiseEvent::DragExit);
            }

            #[unsafe(method(prepareForDragOperation:))]
            fn prepare_for_drag_operation(
                &self,
                _sender: &ProtocolObject<dyn NSDraggingInfo>,
            ) -> Bool {
                Bool::YES
            }

            #[unsafe(method(performDragOperation:))]
            fn perform_drag_operation(
                &self,
                sender: &ProtocolObject<dyn NSDraggingInfo>,
            ) -> Bool {
                eprintln!("[file_promise] performDragOperation");
                unsafe {
                    let pb = sender.draggingPasteboard();
                    let receivers_class = NSFilePromiseReceiver::class();
                    let classes = NSArray::from_slice(&[receivers_class]);
                    let read_opts: Retained<NSDictionary<NSString>> = NSDictionary::new();
                    let items: Option<Retained<NSArray<AnyObject>>> =
                        pb.readObjectsForClasses_options(&classes, Some(&read_opts));
                    let Some(items) = items else {
                        eprintln!("[file_promise] pasteboard returned no items");
                        return Bool::NO;
                    };
                    if items.count() == 0 {
                        eprintln!("[file_promise] zero receivers despite matching drag type");
                        return Bool::NO;
                    }

                    let dest_dir_path = std::env::temp_dir();
                    let dest_str = NSString::from_str(&dest_dir_path.to_string_lossy());
                    let dest_url: Retained<NSURL> = NSURL::fileURLWithPath(&dest_str);
                    let queue: Retained<NSOperationQueue> = NSOperationQueue::new();
                    let empty_opts: Retained<NSDictionary<AnyObject>> = NSDictionary::new();

                    for raw in items.iter() {
                        let receiver: Retained<NSFilePromiseReceiver> =
                            Retained::cast_unchecked::<NSFilePromiseReceiver>(raw);

                        let block = RcBlock::new(
                            move |url: NonNull<NSURL>, err: *mut NSError| {
                                if !err.is_null() {
                                    let err_ref: &NSError = &*err;
                                    eprintln!(
                                        "[file_promise] receivePromisedFiles error: {}",
                                        err_ref.localizedDescription()
                                    );
                                    return;
                                }
                                let url_ref: &NSURL = url.as_ref();
                                let Some(path_ns) = url_ref.path() else {
                                    eprintln!("[file_promise] resolved URL has no fs path");
                                    return;
                                };
                                let path = PathBuf::from(path_ns.to_string());
                                eprintln!(
                                    "[file_promise] resolved promise → {}",
                                    path.display()
                                );
                                emit(FilePromiseEvent::Drop(path));
                            },
                        );

                        receiver.receivePromisedFilesAtDestination_options_operationQueue_reader(
                            &dest_url,
                            &empty_opts,
                            &queue,
                            &block,
                        );
                    }

                    Bool::YES
                }
            }
        }
    );

    impl PromiseDropView {
        /// Construct a new overlay view sized to the given frame.
        /// `initWithFrame:` is the canonical NSView designated
        /// initializer; we send it via msg_send! so the return type
        /// resolves directly to `Retained<Self>` without intermediate
        /// .cast() dances that don't exist on Allocated/Retained.
        fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
            let this = Self::alloc(mtm);
            unsafe { msg_send![this, initWithFrame: frame] }
        }
    }
}
