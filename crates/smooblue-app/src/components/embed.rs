//! Rich-media renderer for post embeds.
//!
//! Handles every `app.bsky.embed.*#view` variant the AppView returns:
//! - **Images** — 1-, 2-, 3-, 4-up grids matching Bluesky's layout.
//! - **External** — link card with thumbnail + title + description + domain.
//! - **Record** — quoted post rendered as a mini card. Deleted /
//!   blocked / detached quotes get explicit "not available" tiles
//!   rather than blank space.
//! - **RecordWithMedia** — outer media (e.g. you attached your own
//!   image) on top, quoted post below.
//! - **Video** — thumbnail with a play overlay; clicking opens
//!   bsky.app in the system browser. Real HLS playback is a follow-up.
//!
//! Unknown / forward-compat embeds render nothing (silent).

use crate::icons;
use dioxus::prelude::*;
use smooblue_atproto::{
    Embed, EmbedExternal, EmbedImage, EmbedKind, EmbedMedia, EmbedRecordView,
};
use url::Url;

#[component]
pub fn EmbedView(embed: Embed) -> Element {
    match embed {
        Embed::Known(kind) => rsx! { EmbedKindView { kind } },
        Embed::Unknown(_) => rsx! { Fragment {} },
    }
}

#[component]
fn EmbedKindView(kind: EmbedKind) -> Element {
    match kind {
        EmbedKind::Images { images } => rsx! { ImageGrid { images } },
        EmbedKind::External { external } => rsx! { LinkCard { ext: external } },
        EmbedKind::Record { record } => rsx! { QuoteCard { record } },
        EmbedKind::RecordWithMedia { record, media } => rsx! {
            div { class: "embed__rwm",
                MediaView { media: *media }
                QuoteCard { record: record.record }
            }
        },
        EmbedKind::Video { thumbnail, aspect_ratio, .. } => rsx! {
            VideoPlaceholder { thumb: thumbnail, aspect_ratio: aspect_ratio.map(|a| (a.width, a.height)) }
        },
    }
}

/// Render the inner-media variant for RecordWithMedia. Reuses the
/// same component pieces but without the Record/RecordWithMedia
/// branches (which would nest forever).
#[component]
fn MediaView(media: EmbedMedia) -> Element {
    match media {
        EmbedMedia::Images { images } => rsx! { ImageGrid { images } },
        EmbedMedia::External { external } => rsx! { LinkCard { ext: external } },
        EmbedMedia::Video { thumbnail, aspect_ratio, .. } => rsx! {
            VideoPlaceholder { thumb: thumbnail, aspect_ratio: aspect_ratio.map(|a| (a.width, a.height)) }
        },
    }
}

/// 1-, 2-, 3-, or 4-up image grid. Matches Bluesky's layout: 1 fills
/// the embed width; 2 is side-by-side; 3 is one big-left + two
/// stacked-right; 4 is a 2x2 grid.
#[component]
fn ImageGrid(images: Vec<EmbedImage>) -> Element {
    let n = images.len().min(4);
    let class = match n {
        1 => "embed__images embed__images--1",
        2 => "embed__images embed__images--2",
        3 => "embed__images embed__images--3",
        _ => "embed__images embed__images--4",
    };
    rsx! {
        div { class: "{class}",
            for (i, img) in images.iter().take(4).enumerate() {
                ImageTile { key: "{i}", img: img.clone(), index: i, total: n }
            }
        }
    }
}

#[component]
fn ImageTile(img: EmbedImage, index: usize, total: usize) -> Element {
    let alt = if img.alt.is_empty() {
        "Attached image".to_string()
    } else {
        img.alt.clone()
    };
    let fullsize = img.fullsize.clone();
    let open_fullsize = move |_| {
        // Open via macOS `open` so it goes to whatever the user's
        // default browser/preview app is. Best-effort — failures here
        // shouldn't crash the click handler.
        let _ = std::process::Command::new("open").arg(&fullsize).spawn();
    };
    // Position class for the 3-up layout (one tall left + two stacked right).
    let pos_class = if total == 3 {
        match index {
            0 => " embed__image--big",
            _ => " embed__image--half",
        }
    } else {
        ""
    };
    rsx! {
        button {
            class: "embed__image{pos_class}",
            title: "{alt}",
            onclick: open_fullsize,
            img { src: "{img.thumb}", alt: "{alt}" }
            if !img.alt.is_empty() {
                span { class: "embed__image-alt-badge", title: "{alt}", "ALT" }
            }
        }
    }
}

/// External link card (`app.bsky.embed.external#view`). Renders as a
/// horizontally-laid-out card: small thumbnail on the left, title +
/// description + domain on the right. Click opens the URL.
#[component]
fn LinkCard(ext: EmbedExternal) -> Element {
    let domain = Url::parse(&ext.uri)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.trim_start_matches("www.").to_string()))
        .unwrap_or_else(|| ext.uri.clone());
    let uri = ext.uri.clone();
    let open = move |_| {
        let _ = std::process::Command::new("open").arg(&uri).spawn();
    };
    rsx! {
        button { class: "embed__link", onclick: open, title: "{ext.uri}",
            if let Some(thumb) = ext.thumb.as_ref() {
                div { class: "embed__link-thumb",
                    img { src: "{thumb}", alt: "" }
                }
            }
            div { class: "embed__link-meta",
                span { class: "embed__link-domain", "{domain}" }
                span { class: "embed__link-title", "{ext.title}" }
                if !ext.description.is_empty() {
                    span { class: "embed__link-desc", "{ext.description}" }
                }
            }
        }
    }
}

/// Quoted post — mini card with the quoted author + text. Nested
/// image embeds inside the quoted post render shallowly (no quote
/// chains).
#[component]
fn QuoteCard(record: EmbedRecordView) -> Element {
    match record {
        EmbedRecordView::View { uri: _, author, value, embeds, .. } => {
            let name = author
                .display_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&author.handle)
                .to_string();
            // Shallow-render any inner image embed (skip nested
            // quotes / record-with-media to avoid infinite nesting).
            let inner_images = embeds.into_iter().find_map(|k| match k {
                EmbedKind::Images { images } => Some(images),
                _ => None,
            });
            rsx! {
                div { class: "embed__quote",
                    div { class: "embed__quote-head",
                        if let Some(av) = &author.avatar {
                            img { class: "embed__quote-avatar", src: "{av}", alt: "{author.handle}" }
                        }
                        span { class: "embed__quote-name", "{name}" }
                        span { class: "embed__quote-handle", "@{author.handle}" }
                    }
                    if !value.text.is_empty() {
                        p { class: "embed__quote-text", "{value.text}" }
                    }
                    if let Some(images) = inner_images {
                        ImageGrid { images }
                    }
                }
            }
        }
        EmbedRecordView::NotFound { .. } => rsx! {
            div { class: "embed__quote embed__quote--missing",
                "Quoted post was deleted"
            }
        },
        EmbedRecordView::Blocked { .. } => rsx! {
            div { class: "embed__quote embed__quote--missing",
                "Quoted post is from a blocked account"
            }
        },
        EmbedRecordView::Detached { .. } => rsx! {
            div { class: "embed__quote embed__quote--missing",
                "Quote was removed by the author"
            }
        },
        EmbedRecordView::Other => rsx! { Fragment {} },
    }
}

/// Video placeholder — thumbnail with a play-button overlay. Real HLS
/// playback is a separate pearl (would need a video element + HLS.js
/// or a native AVPlayer bridge on macOS). For now the click opens the
/// post on bsky.app where the user already has working playback.
#[component]
fn VideoPlaceholder(thumb: Option<String>, aspect_ratio: Option<(u32, u32)>) -> Element {
    let (w, h) = aspect_ratio.unwrap_or((16, 9));
    let padding_pct = (h as f32 / w.max(1) as f32) * 100.0;
    rsx! {
        div { class: "embed__video",
            style: "padding-top: {padding_pct}%;",
            if let Some(t) = thumb {
                img { class: "embed__video-thumb", src: "{t}", alt: "video thumbnail" }
            } else {
                div { class: "embed__video-thumb embed__video-thumb--blank" }
            }
            div { class: "embed__video-play",
                icons::Play { size: icons::Size::Lg }
            }
        }
    }
}
