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
            .with_min_inner_size(LogicalSize::new(560.0, 480.0)),
    );

    LaunchBuilder::desktop()
        .with_cfg(cfg)
        .launch(smooblue_app::App);
}
