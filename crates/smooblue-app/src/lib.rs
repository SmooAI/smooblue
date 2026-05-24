//! Smooblue UI library — the binary in `main.rs` is a thin entry that
//! constructs the Dioxus window and calls [`App`]. Everything testable
//! (components, view models, persistence helpers) lives here.

pub mod components;
pub mod demo;
pub mod icons;
pub mod image_prep;
pub mod persistence;
pub mod state;
pub mod views;

use dioxus::prelude::*;
use smooblue_theme::STYLES;

/// Top-level Dioxus component.
///
/// Branches on the persisted session:
/// - `Some(session)` → render the deck shell with the Home column
/// - `None` → render the login view
#[component]
pub fn App() -> Element {
    // Bootstrap global state on first render.
    state::use_bootstrap();

    let session = use_context::<Signal<Option<smooblue_oauth::Session>>>();

    rsx! {
        style { "{STYLES}" }
        div {
            id: "main",
            if session.read().is_some() {
                components::deck::DeckShell {}
            } else {
                views::login::LoginView {}
            }
        }
    }
}
