//! Deck shell — rail + horizontally-scrolling columns + floating "+"
//! (compose) + the global compose sheet.

use crate::components::{column::Column, compose::ComposeSheet, sidebar::Sidebar};
use crate::icons;
use crate::state::{ColumnSpec, Tick};
use dioxus::prelude::*;
use std::time::Duration;

#[component]
pub fn DeckShell() -> Element {
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let columns = cols.read().clone();
    let mut compose_open = use_signal(|| false);

    // 1-second tick that drives time-relative re-renders (post timestamps
    // ticking "11s" → "12s" etc.). Reading the Tick context in
    // PostCard/NotificationCard subscribes them; the signal bump here
    // triggers their re-render.
    let tick = use_context::<Signal<Tick>>();
    use_future(move || {
        let mut tick = tick;
        async move {
            let mut counter: u64 = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                counter = counter.wrapping_add(1);
                tick.set(Tick(counter));
            }
        }
    });

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
