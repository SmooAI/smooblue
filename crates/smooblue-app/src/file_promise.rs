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
//! Non-macOS builds: stubs that return `None` / no-op.

#[cfg(target_os = "macos")]
pub use macos::{install_on_main_window, take_receiver};

#[cfg(not(target_os = "macos"))]
pub fn install_on_main_window() {}

#[cfg(not(target_os = "macos"))]
pub fn take_receiver() -> Option<tokio::sync::mpsc::UnboundedReceiver<std::path::PathBuf>> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, ProtocolObject};
    use objc2::{define_class, msg_send, ClassType, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSDragOperation, NSDraggingInfo, NSFilePromiseReceiver, NSView,
    };
    use objc2_foundation::{
        ns_string, MainThreadMarker, NSArray, NSDictionary, NSError, NSOperationQueue, NSRect,
        NSString, NSURL,
    };
    use parking_lot::Mutex;
    use std::ptr::NonNull;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
    use tracing::{debug, error, info, warn};

    /// Channel between the AppKit overlay callback (main thread, ObjC
    /// runtime) and the Dioxus listener in `compose.rs` (tokio task).
    /// `OnceLock` so the sender survives multiple install attempts;
    /// `Mutex<Option<…>>` for the receiver because only one consumer
    /// drains it (compose.rs takes it once at first mount).
    static SENDER: OnceLock<UnboundedSender<PathBuf>> = OnceLock::new();
    static RECEIVER: Mutex<Option<UnboundedReceiver<PathBuf>>> = Mutex::new(None);

    /// Guard against double-install. The overlay is per-window and the
    /// channel is global; both should initialize exactly once.
    static INSTALLED: OnceLock<()> = OnceLock::new();

    /// Install the file-promise drop overlay on the main NSWindow's
    /// content view. Must be called on the main thread (it's an
    /// `NSView` operation). Idempotent — subsequent calls are no-ops.
    ///
    /// Call site: `App` component's `use_hook` (runs once, on the main
    /// thread because Dioxus desktop's render thread IS the AppKit
    /// main thread on macOS).
    pub fn install_on_main_window() {
        if INSTALLED.set(()).is_err() {
            return;
        }

        // Initialize the channel before installing the view, so any
        // racing drop event has a sender to write to.
        let (tx, rx) = unbounded_channel();
        let _ = SENDER.set(tx);
        *RECEIVER.lock() = Some(rx);

        // SAFETY: All AppKit calls below are made on the main thread
        // (MainThreadMarker::new() returns Some only there) and only
        // touch documented APIs on freshly-constructed or properly
        // retained value types.
        let Some(mtm) = MainThreadMarker::new() else {
            warn!(
                "file_promise::install_on_main_window called off the main thread — skipping. \
                 This shouldn't happen — Dioxus' App use_hook runs on the AppKit main thread."
            );
            return;
        };

        unsafe {
            let app = NSApplication::sharedApplication(mtm);
            let windows = app.windows();
            let Some(window) = windows.iter().next() else {
                warn!("file_promise: no NSWindow available yet at install time — skipping");
                return;
            };
            let Some(content_view) = window.contentView() else {
                warn!("file_promise: NSWindow has no contentView — skipping");
                return;
            };

            // Build the overlay — an NSView covering the content view's
            // frame, with autoresizing so it tracks window resizes.
            let bounds = content_view.bounds();
            let overlay: Retained<PromiseDropView> = PromiseDropView::new(mtm, bounds);

            // 0b110 = NSViewWidthSizable | NSViewHeightSizable. Without
            // these the overlay stays at install-time size and the
            // bottom-right of the window stops accepting promise drops
            // after a resize.
            overlay.set_autoresizing_mask(
                objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
            );

            // Register for promise drag types ONLY. Finder drops carry
            // public.file-url which we DON'T register for, so they hit
            // the WKWebView underneath via macOS's normal drag
            // destination resolution (drag types must match for a
            // view's draggingEntered: to fire at all).
            //
            // The promise UTI is com.apple.NSFilePromiseProvider on
            // modern macOS; older releases used NSFilesPromisePboardType
            // (the .pbxFileWrapper-style type). Register both to be
            // forgiving across OS versions.
            let promise_types = NSArray::from_slice(&[
                ns_string!("com.apple.NSFilePromiseProvider"),
                // Legacy alias — still emitted by some apps' DnD code.
                ns_string!("com.apple.pasteboard.promised-file-url"),
            ]);
            overlay.register_for_dragged_types(&promise_types);

            // Add as the topmost subview so it gets drag-target priority
            // over the WKWebView for matching types. Frame-on-top trick:
            // pass nil for relativeTo + NSWindowAbove.
            // addSubview:positioned:relativeTo: is the right method here.
            // 1 = NSWindowAbove
            let nil_view: *mut NSView = std::ptr::null_mut();
            let _: () = msg_send![
                &*content_view,
                addSubview: &*overlay,
                positioned: 1i64,
                relativeTo: nil_view,
            ];

            info!(
                "file_promise: overlay installed on window contentView \
                 ({}x{}); listening for NSFilePromiseProvider drops",
                bounds.size.width, bounds.size.height
            );
        }
    }

    /// Take the receiver for incoming promised paths. Called once at
    /// compose-sheet first-mount; the listener task then runs forever
    /// pushing paths into the attachments pipeline.
    pub fn take_receiver() -> Option<UnboundedReceiver<PathBuf>> {
        RECEIVER.lock().take()
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
        /// underneath. Drag-destination resolution uses a separate
        /// AppKit path that doesn't go through hitTest, so promise
        /// drops still find us.
        #[unsafe(super(NSView))]
        #[name = "SmooBluePromiseDropView"]
        struct PromiseDropView;

        impl PromiseDropView {
            #[unsafe(method(hitTest:))]
            fn hit_test(&self, _point: objc2_foundation::NSPoint) -> *mut NSView {
                std::ptr::null_mut()
            }

            #[unsafe(method(draggingEntered:))]
            fn dragging_entered(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
                debug!("file_promise: draggingEntered (promise type detected)");
                NSDragOperation::Copy
            }

            #[unsafe(method(draggingUpdated:))]
            fn dragging_updated(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
                NSDragOperation::Copy
            }

            #[unsafe(method(prepareForDragOperation:))]
            fn prepare_for_drag_operation(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) -> Bool {
                Bool::YES
            }

            #[unsafe(method(performDragOperation:))]
            fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Bool {
                unsafe {
                    let pb = sender.draggingPasteboard();
                    let receivers_class = NSFilePromiseReceiver::class();
                    let classes = NSArray::from_slice(&[receivers_class]);
                    // Cocoa's NSDictionary type-param shows up differently
                    // across the two binding sites — readObjectsForClasses
                    // wants NSDictionary<NSString>, receivePromisedFiles
                    // wants NSDictionary<AnyObject>. Declare each at the
                    // shape its caller expects.
                    let read_opts: Retained<NSDictionary<NSString>> = NSDictionary::new();
                    let items: Option<Retained<NSArray<AnyObject>>> = pb
                        .readObjectsForClasses_options(&classes, Some(&read_opts));
                    let Some(items) = items else {
                        warn!("file_promise: performDragOperation but pasteboard returned no NSFilePromiseReceiver items");
                        return Bool::NO;
                    };
                    if items.count() == 0 {
                        warn!("file_promise: zero receivers despite matching drag type");
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

                        // Take a sender clone per item — the block is
                        // 'static and can outlive this loop iteration.
                        let tx = SENDER.get().cloned();
                        let block = RcBlock::new(
                            move |url: NonNull<NSURL>, err: *mut NSError| {
                                if !err.is_null() {
                                    let err_ref: &NSError = &*err;
                                    error!(
                                        "file_promise: receivePromisedFiles error: {}",
                                        err_ref.localizedDescription().to_string()
                                    );
                                    return;
                                }
                                let url_ref: &NSURL = url.as_ref();
                                let Some(path_ns) = url_ref.path() else {
                                    warn!("file_promise: resolved URL has no filesystem path");
                                    return;
                                };
                                let path = PathBuf::from(path_ns.to_string());
                                debug!("file_promise: resolved promise → {}", path.display());
                                if let Some(tx) = tx.as_ref() {
                                    if let Err(e) = tx.send(path) {
                                        error!("file_promise: channel closed: {e}");
                                    }
                                } else {
                                    warn!("file_promise: SENDER not initialized, dropping path");
                                }
                            },
                        );

                        receiver
                            .receivePromisedFilesAtDestination_options_operationQueue_reader(
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
        fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
            let this = Self::alloc(mtm);
            unsafe {
                let initialized: Retained<Self> = msg_send![this, initWithFrame: frame];
                initialized
            }
        }

        fn set_autoresizing_mask(&self, mask: objc2_app_kit::NSAutoresizingMaskOptions) {
            unsafe {
                let _: () = msg_send![self, set_autoresizing_mask: mask];
            }
        }

        fn register_for_dragged_types(&self, types: &NSArray<NSString>) {
            unsafe {
                let _: () = msg_send![self, register_for_dragged_types: types];
            }
        }
    }
}
