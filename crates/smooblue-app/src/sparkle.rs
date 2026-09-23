//! Sparkle 2 over-the-air updates — the same updater SmoothFlow uses.
//!
//! `scripts/bundle-macos.sh` embeds `Sparkle.framework` in
//! `Smooblue.app/Contents/Frameworks` and writes the feed + public key
//! into Info.plist (`SUFeedURL`, `SUPublicEDKey`). At launch [`start`]
//! loads the framework *at runtime* through `NSBundle` and creates a
//! `SPUStandardUpdaterController`, which from then on checks hourly
//! (Info.plist `SUScheduledCheckInterval`) and drives Sparkle's own
//! native "A new version of Smooblue is available!" dialog: release
//! notes, Skip This Version / Remind Me Later / Install Update, and the
//! "automatically download and install" checkbox. [`start`] also adds
//! "Check for Updates…" to the app menu; Settings calls
//! [`check_for_updates`].
//!
//! Loading at runtime instead of linking is deliberate: `cargo run`,
//! Linux, and any bundle without the framework keep working and simply
//! fall back to the GitHub "update available" toast
//! ([`crate::updates`]). [`is_active`] tells the deck which one applies.
//!
//! The Sparkle class has no Rust bindings, so its two selectors
//! (`initWithStartingUpdater:updaterDelegate:userDriverDelegate:` and
//! `checkForUpdates:`) are sent with `msg_send!` — checked against the
//! vendored 2.9.6 headers. Everything runs inside the same
//! `catch_unwind` + `objc2::exception::catch` guard as `file_promise`:
//! a v1.5.0 selector typo there aborted the whole app, and an updater
//! must never be able to do that. Any failure logs and leaves the
//! toast fallback in place.

use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether a Sparkle updater is running in this process.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// `SMOOBLUE_DISABLE_SPARKLE=1` (and demo mode) skip the updater —
/// Sparkle's first-run prompts and update dialogs would steal focus
/// from screenshots and automation runs, the same reason SmoothFlow
/// skips it under `SMOOTHFLOW_UI_TEST`. `SMOOBLUE_FORCE_SPARKLE=1`
/// runs it anyway (demo included) — how the update flow is tested
/// end to end against a local feed without a real account.
pub fn disabled_by_env() -> bool {
    let flag = |name: &str| matches!(std::env::var(name).as_deref(), Ok("1" | "true" | "yes"));
    if flag("SMOOBLUE_FORCE_SPARKLE") {
        return false;
    }
    crate::demo::is_active() || flag("SMOOBLUE_DISABLE_SPARKLE")
}

#[cfg(not(target_os = "macos"))]
pub fn start() {}

#[cfg(not(target_os = "macos"))]
pub fn check_for_updates() {}

#[cfg(target_os = "macos")]
pub use imp::{check_for_updates, start};

#[cfg(target_os = "macos")]
mod imp {
    use super::ACTIVE;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSMenuItem};
    use objc2_foundation::{NSBundle, NSString};
    use std::panic::AssertUnwindSafe;
    use std::sync::atomic::Ordering;

    /// The live `SPUStandardUpdaterController`. Main-thread only; kept
    /// for the life of the process (the updater must outlive every
    /// check it schedules).
    struct Controller(Retained<AnyObject>);
    // SAFETY: only ever created and dereferenced on the main thread
    // (both entry points require a `MainThreadMarker`); the static just
    // parks the retain so the controller is never deallocated.
    unsafe impl Send for Controller {}
    unsafe impl Sync for Controller {}

    static CONTROLLER: std::sync::OnceLock<Controller> = std::sync::OnceLock::new();

    /// Load Sparkle from the app bundle and start the updater. Call
    /// once, on the main thread, after `NSApplication` is up. No-op
    /// (with a log line) when anything is missing.
    pub fn start() {
        if super::disabled_by_env() || CONTROLLER.get().is_some() {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            tracing::warn!("sparkle: start() called off the main thread; updater not started");
            return;
        };
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: every message inside is checked against the
            // vendored Sparkle 2.9.6 / AppKit headers; the exception
            // guard turns any ObjC exception into an Err.
            unsafe { objc2::exception::catch(AssertUnwindSafe(|| start_inner(mtm))) }
        }));
        match outcome {
            Ok(Ok(Ok(()))) => {
                ACTIVE.store(true, Ordering::Relaxed);
                tracing::info!("sparkle: updater started");
            }
            Ok(Ok(Err(reason))) => tracing::info!(reason, "sparkle: not started"),
            Ok(Err(exc)) => tracing::warn!(?exc, "sparkle: ObjC exception while starting"),
            Err(_) => tracing::warn!("sparkle: panic while starting"),
        }
    }

    unsafe fn start_inner(mtm: MainThreadMarker) -> Result<(), &'static str> {
        let main = NSBundle::mainBundle();
        let frameworks = main
            .privateFrameworksPath()
            .ok_or("app bundle has no Frameworks path")?;
        let path = NSString::from_str(&format!("{frameworks}/Sparkle.framework"));
        let sparkle = NSBundle::bundleWithPath(&path)
            .ok_or("Sparkle.framework not bundled (dev build or Linux-style run)")?;
        if sparkle.loadAndReturnError().is_err() {
            return Err("Sparkle.framework failed to load");
        }
        let cls = AnyClass::get(c"SPUStandardUpdaterController")
            .ok_or("SPUStandardUpdaterController class missing after load")?;
        let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
        let nil: *mut AnyObject = std::ptr::null_mut();
        // startingUpdater: YES — Sparkle schedules the hourly checks
        // itself; delegates nil = the standard user driver (Sparkle's
        // own native windows).
        let controller: Option<Retained<AnyObject>> = msg_send![
            alloc,
            initWithStartingUpdater: Bool::YES,
            updaterDelegate: nil,
            userDriverDelegate: nil
        ];
        let controller = controller.ok_or("SPUStandardUpdaterController init returned nil")?;
        add_menu_item(mtm, &controller);
        let _ = CONTROLLER.set(Controller(controller));
        Ok(())
    }

    /// Put "Check for Updates…" at the top of the app menu (under
    /// "About Smooblue" if the menu has one — dioxus-desktop's default
    /// doesn't), targeting the controller, the way SmoothFlow and other
    /// Sparkle apps do. Best-effort: no app menu, no item.
    unsafe fn add_menu_item(mtm: MainThreadMarker, controller: &AnyObject) {
        let app = NSApplication::sharedApplication(mtm);
        let Some(bar) = app.mainMenu() else {
            return;
        };
        if bar.numberOfItems() == 0 {
            return;
        }
        let Some(app_menu) = bar.itemAtIndex(0).and_then(|i| i.submenu()) else {
            return;
        };
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Check for Updates…"),
            Some(sel!(checkForUpdates:)),
            &NSString::from_str(""),
        );
        item.setTarget(Some(controller));
        let has_about = app_menu
            .itemAtIndex(0)
            .is_some_and(|first| first.title().to_string().starts_with("About"));
        let at = if has_about { 1 } else { 0 };
        app_menu.insertItem_atIndex(&item, at);
        app_menu.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), at + 1);
    }

    /// Show Sparkle's "Check for Updates" flow (the same as the menu
    /// item). No-op when the updater isn't running.
    pub fn check_for_updates() {
        let Some(controller) = CONTROLLER.get() else {
            return;
        };
        if MainThreadMarker::new().is_none() {
            tracing::warn!("sparkle: check_for_updates() off the main thread; ignored");
            return;
        }
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: `checkForUpdates:` is an IBAction on
            // SPUStandardUpdaterController taking a nullable sender.
            unsafe {
                objc2::exception::catch(AssertUnwindSafe(|| {
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![&*controller.0, checkForUpdates: nil];
                }))
            }
        }));
        if !matches!(outcome, Ok(Ok(()))) {
            tracing::warn!("sparkle: checkForUpdates failed");
        }
    }
}
