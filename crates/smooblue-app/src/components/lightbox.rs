//! In-app image / video lightbox.
//!
//! Why this exists: clicking a post image used to shell out to macOS
//! `open` which routes to Preview.app (or the system default). That
//! dropped the user out of Smooblue for what should be a quick
//! glance. Now image and video clicks populate `LightboxFocus` and
//! this component renders a full-screen overlay centered on the
//! deck.
//!
//! Closes on:
//! - backdrop click
//! - the close button (X)
//! - Esc (handled at the deck-shell keyboard layer)
//!
//! WKWebView's native `<video>` element decodes both mp4 and m3u8
//! directly, so we get progress bar / fullscreen / volume controls
//! for free via `controls`.

use crate::icons;
use crate::state::{LightboxFocus, LightboxItem};
use dioxus::prelude::*;

#[component]
pub fn LightboxSheet() -> Element {
    let mut focus = use_context::<Signal<LightboxFocus>>();
    let snap = focus.read().0.clone();
    let Some(item) = snap else {
        return rsx! { Fragment {} };
    };

    let close = move |_| focus.set(LightboxFocus(None));

    rsx! {
        div { class: "lightbox__backdrop",
            // Backdrop click closes. We don't stop propagation on
            // inner clicks because the media itself + the close
            // button handle their own clicks; clicks on the empty
            // letterbox margin should still dismiss.
            onclick: close,
            button { class: "lightbox__close",
                title: "Close (Esc)",
                onclick: close,
                icons::X { size: icons::Size::Md }
            }
            match item {
                LightboxItem::Image { url, alt } => rsx! {
                    img {
                        class: "lightbox__image",
                        src: "{url}",
                        alt: "{alt}",
                        // Stop the inner click from bubbling to the
                        // backdrop close — only the empty margin
                        // should dismiss.
                        onclick: move |e: MouseEvent| e.stop_propagation(),
                    }
                    if !alt.is_empty() {
                        div { class: "lightbox__caption",
                            onclick: move |e: MouseEvent| e.stop_propagation(),
                            "{alt}"
                        }
                    }
                },
                LightboxItem::Video { url, poster } => rsx! {
                    video {
                        class: "lightbox__video",
                        src: "{url}",
                        poster: poster.as_deref().unwrap_or(""),
                        controls: true,
                        autoplay: true,
                        // Same stop-propagation reasoning as the
                        // image branch.
                        onclick: move |e: MouseEvent| e.stop_propagation(),
                    }
                },
            }
        }
    }
}
