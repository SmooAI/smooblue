//! Deck shell — rail + horizontally-scrolling columns + floating "+"
//! (compose) + the global compose sheet + the search-column add sheet.

use crate::components::{
    column::Column, compose::ComposeSheet, engagement::EngagementSheet, profile::ProfileSheet,
    search_sheet::SearchSheet, sidebar::Sidebar, thread::ThreadSheet,
};
use crate::icons;
use crate::state::{ColumnSpec, ComposeContext, Tick};
use dioxus::prelude::*;
use std::time::Duration;

#[component]
pub fn DeckShell() -> Element {
    let cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let columns = cols.read().clone();
    let mut compose_ctx = use_context::<Signal<ComposeContext>>();
    let search_open = use_signal(|| false);

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

    let open_compose = move |_| {
        let mut w = compose_ctx.write();
        w.reply_to = None;
        w.open = true;
    };

    rsx! {
        div { class: "deck-shell",
            Sidebar { search_open }
            div { class: "deck-columns",
                for spec in columns {
                    Column { key: "{spec.id}", spec: spec.clone() }
                }
            }
            button {
                class: "fab",
                title: "New post",
                onclick: open_compose,
                icons::Plus { size: icons::Size::Lg }
            }
            ComposeSheet {}
            SearchSheet { open: search_open }
            ThreadSheet {}
            ProfileSheet {}
            EngagementSheet {}
        }
    }
}
