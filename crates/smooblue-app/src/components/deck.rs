//! Deck shell — rail + horizontally-scrolling columns + floating "+"
//! (compose) + the global compose sheet.

use crate::components::{column::Column, compose::ComposeSheet, sidebar::Sidebar};
use crate::icons;
use crate::state::ColumnSpec;
use dioxus::prelude::*;

#[component]
pub fn DeckShell() -> Element {
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let columns = cols.read().clone();
    let mut compose_open = use_signal(|| false);

    rsx! {
        div { class: "deck-shell",
            Sidebar {}
            div { class: "deck-columns",
                for spec in columns {
                    Column { key: "{spec.id}", spec: spec }
                }
            }
            button {
                class: "fab",
                title: "New post",
                onclick: move |_| compose_open.set(true),
                icons::Plus { size: icons::Size::Lg }
            }
            ComposeSheet { open: compose_open }
        }
    }
}
