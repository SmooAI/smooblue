//! Deck shell — left rail + horizontally-scrolling columns.

use crate::components::{column::Column, sidebar::Sidebar};
use crate::state::ColumnSpec;
use dioxus::prelude::*;

#[component]
pub fn DeckShell() -> Element {
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let columns = cols.read().clone();
    rsx! {
        div { class: "deck-shell",
            Sidebar {}
            div { class: "deck-columns",
                for spec in columns {
                    Column { key: "{spec.id}", spec: spec }
                }
            }
        }
    }
}
