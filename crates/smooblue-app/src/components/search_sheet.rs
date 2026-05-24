//! Small modal sheet that prompts for a query and adds a Search column
//! to the deck. Reuses the shared modal chrome from smooai-ui.

use crate::icons;
use crate::state::{add_column_unique, ColumnSpec};
use dioxus::prelude::*;

#[component]
pub fn SearchSheet(open: Signal<bool>) -> Element {
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();
    let mut query = use_signal(String::new);

    if !*open.read() {
        return rsx! { Fragment {} };
    }

    let mut do_submit = move || {
        let q = query.read().trim().to_string();
        if q.is_empty() {
            return;
        }
        add_column_unique(&mut cols, ColumnSpec::search(q));
        query.set(String::new());
        open.set(false);
    };

    rsx! {
        div { class: "modal__backdrop", onclick: move |_| open.set(false),
            div { class: "modal__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "compose__head",
                    span { class: "compose__title", "Add a Search column" }
                    button { class: "compose__close",
                        title: "Close",
                        onclick: move |_| open.set(false),
                        "✕"
                    }
                }
                input {
                    class: "input input--lg",
                    placeholder: "Search posts… (e.g. \"rust\", \"@alice.bsky.social\")",
                    autofocus: true,
                    value: "{query}",
                    oninput: move |e| query.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            do_submit();
                        }
                    },
                }
                div { class: "compose__bar",
                    span { class: "compose__counter", "Polls every 30s" }
                    button {
                        class: "btn btn--primary",
                        disabled: query.read().trim().is_empty(),
                        onclick: move |_| do_submit(),
                        icons::Search { size: icons::Size::Sm }
                        "Add column"
                    }
                }
            }
        }
    }
}
