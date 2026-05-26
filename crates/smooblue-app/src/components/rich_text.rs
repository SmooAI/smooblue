//! Render a [`PostRecord`]'s text with its `facets` as click-targets.
//!
//! Each [`FacetSegment`] becomes either a plain `<span>` or a
//! `<button>` styled like a link. Mentions open the actor's profile
//! sheet; links open in the system browser (scheme-allowlisted); tags
//! open a Search column for the tag value.
//!
//! Every click handler `stop_propagation`s so it doesn't bubble up to
//! the PostCard's "open thread" wrapper around us.

use crate::state::{add_column_unique, ColumnSpec, ProfileFocus};
use dioxus::prelude::*;
use smooblue_atproto::{FacetSegment, PostRecord};

#[component]
pub fn RichText(record: PostRecord) -> Element {
    let segments = record.resolved_facets();
    let mut profile_focus = use_context::<Signal<ProfileFocus>>();
    let mut cols = use_context::<Signal<Vec<ColumnSpec>>>();

    rsx! {
        for (i, seg) in segments.into_iter().enumerate() {
            match seg {
                FacetSegment::Text(t) => rsx! {
                    span { key: "{i}", "{t}" }
                },
                FacetSegment::Mention { text, did } => {
                    let did_for_click = did.clone();
                    rsx! {
                        button {
                            key: "{i}",
                            class: "post__text-link post__text-mention",
                            title: "Open profile",
                            onclick: move |e: MouseEvent| {
                                e.stop_propagation();
                                profile_focus.set(ProfileFocus(Some(did_for_click.clone())));
                            },
                            "{text}"
                        }
                    }
                }
                FacetSegment::Link { text, uri } => {
                    let uri_for_click = uri.clone();
                    rsx! {
                        button {
                            key: "{i}",
                            class: "post__text-link",
                            title: "{uri}",
                            onclick: move |e: MouseEvent| {
                                e.stop_propagation();
                                let _ = crate::safe_open::open_in_browser(&uri_for_click);
                            },
                            "{text}"
                        }
                    }
                }
                FacetSegment::Tag { text, tag } => {
                    let tag_for_click = tag.clone();
                    rsx! {
                        button {
                            key: "{i}",
                            class: "post__text-link post__text-tag",
                            title: "Search #{tag}",
                            onclick: move |e: MouseEvent| {
                                e.stop_propagation();
                                add_column_unique(
                                    &mut cols,
                                    ColumnSpec::search(format!("#{}", tag_for_click)),
                                );
                            },
                            "{text}"
                        }
                    }
                }
            }
        }
    }
}
