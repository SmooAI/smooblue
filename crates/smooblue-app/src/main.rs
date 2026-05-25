//! Smooblue desktop entry point.
//!
//! Constructs the Dioxus window with the smoo-branded icon and 1280×800
//! default size, then launches [`smooblue_app::App`].

use dioxus::prelude::*;
use dioxus_desktop::{Config, LogicalSize, WindowBuilder};

fn main() {
    let cfg = Config::new().with_window(
        WindowBuilder::new()
            .with_title("Smooblue")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(560.0, 480.0))
            // Take keyboard focus on launch so the window receives
            // keystrokes immediately. Without this, winit creates a
            // visible-but-unfocused window and the user has to click
            // somewhere first.
            .with_focused(true),
    );

    // macOS-specific: explicitly promote the process to a regular
    // foreground app + activate it. Without this, Smooblue paints
    // its window but never becomes the `[NSApp frontmost]` app —
    // which means system-wide hotkey tools (BetterSnapTool, Magnet,
    // Raycast window-management, etc.) target whichever app *was*
    // frontmost before launch, not Smooblue. Clicking the menu bar
    // works around it because that's an explicit foreground promote
    // — but the user shouldn't have to.
    //
    // Done before `launch` so the activation runs during the same
    // event-loop tick as the window's first show.
    #[cfg(target_os = "macos")]
    activate_macos_app();

    LaunchBuilder::desktop()
        .with_cfg(cfg)
        .launch(smooblue_app::App);
}

#[cfg(target_os = "macos")]
fn activate_macos_app() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    // SAFETY: Pure Cocoa calls. setActivationPolicy + activate are
    // idempotent and safe on the main thread (which is where `main`
    // runs). NSApplicationActivationPolicyRegular = 0; ignore-other-
    // apps flag is YES.
    unsafe {
        let cls = class!(NSApplication);
        let app: *mut AnyObject = msg_send![cls, sharedApplication];
        let _: () = msg_send![app, setActivationPolicy: 0i64];
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
    }
}
