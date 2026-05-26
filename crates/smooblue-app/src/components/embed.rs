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

use dioxus::prelude::*;
use smooblue_atproto::{Embed, EmbedExternal, EmbedImage, EmbedKind, EmbedMedia, EmbedRecordView};
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
        EmbedKind::Video {
            playlist,
            thumbnail,
            aspect_ratio,
        } => rsx! {
            VideoPlayer {
                playlist,
                thumb: thumbnail,
                aspect_ratio: aspect_ratio.map(|a| (a.width, a.height)),
            }
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
        EmbedMedia::Video {
            playlist,
            thumbnail,
            aspect_ratio,
        } => rsx! {
            VideoPlayer {
                playlist,
                thumb: thumbnail,
                aspect_ratio: aspect_ratio.map(|a| (a.width, a.height)),
            }
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
    let alt_for_lightbox = img.alt.clone();
    let mut lightbox = use_context::<Signal<crate::state::LightboxFocus>>();
    let open_fullsize = move |e: MouseEvent| {
        // In-app lightbox — far less jarring than shelling out to
        // Preview.app via `open`. Also avoids the URL-scheme-handler
        // attack surface (the safe_open allowlist would catch any
        // non-http(s) URL, but the lightbox sidesteps the question
        // entirely since `<img>` only renders http(s) sources).
        e.stop_propagation();
        lightbox.set(crate::state::LightboxFocus(Some(
            crate::state::LightboxItem::Image {
                url: fullsize.clone(),
                alt: alt_for_lightbox.clone(),
            },
        )));
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
            img { loading: "lazy", decoding: "async", src: "{img.thumb}", alt: "{alt}" }
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
        .and_then(|u| {
            u.host_str()
                .map(|s| s.trim_start_matches("www.").to_string())
        })
        .unwrap_or_else(|| ext.uri.clone());
    let uri = ext.uri.clone();
    let open = move |_| {
        // CRITICAL: `ext.uri` is attacker-controlled — any bsky user
        // can publish a post with an arbitrary external embed. Without
        // scheme validation, macOS `open` would happily launch
        // `file:///Users/<you>/.ssh/id_rsa`, `mailto:phish@evil`,
        // `slack://...` deep links, custom URL handlers, etc. The
        // allowlist in safe_open keeps us to http/https.
        let _ = crate::safe_open::open_in_browser(&uri);
    };
    rsx! {
        button { class: "embed__link", onclick: open, title: "{ext.uri}",
            if let Some(thumb) = ext.thumb.as_ref() {
                div { class: "embed__link-thumb",
                    img { loading: "lazy", decoding: "async", src: "{thumb}", alt: "" }
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
/// chains). The card is clickable: opens the **quoted** post's
/// thread (not the parent post's — that's what the PostCard around
/// us would do, so we stop_propagation). Avatar + name open the
/// quoted author's profile via the same stop_propagation pattern.
#[component]
fn QuoteCard(record: EmbedRecordView) -> Element {
    match record {
        EmbedRecordView::View {
            uri,
            author,
            value,
            embeds,
            ..
        } => {
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

            // Click-through plumbing.
            let mut thread_focus = use_context::<Signal<crate::state::ThreadFocus>>();
            let mut profile_focus = use_context::<Signal<crate::state::ProfileFocus>>();
            let uri_for_thread = uri.clone();
            let open_quoted_thread = move |e: MouseEvent| {
                e.stop_propagation();
                thread_focus.set(crate::state::ThreadFocus(Some(uri_for_thread.clone())));
            };
            // Two separate closures because Dioxus' MouseEvent
            // handler is FnMut + consumed by each `onclick:` slot;
            // the avatar and the name are sibling click targets.
            let did_for_avatar = author.did.clone();
            let open_quoted_profile_avatar = move |e: MouseEvent| {
                e.stop_propagation();
                profile_focus.set(crate::state::ProfileFocus(Some(did_for_avatar.clone())));
            };
            let did_for_name = author.did.clone();
            let open_quoted_profile_name = move |e: MouseEvent| {
                e.stop_propagation();
                profile_focus.set(crate::state::ProfileFocus(Some(did_for_name.clone())));
            };
            rsx! {
                div { class: "embed__quote embed__quote--clickable",
                    title: "Open quoted post",
                    onclick: open_quoted_thread,
                    div { class: "embed__quote-head",
                        if let Some(av) = &author.avatar {
                            img {
                                loading: "lazy",
                                decoding: "async",
                                class: "embed__quote-avatar embed__quote-avatar--clickable",
                                src: "{av}",
                                alt: "{author.handle}",
                                onclick: open_quoted_profile_avatar,
                            }
                        }
                        // Name + handle stacked vertically, same fix as
                        // the parent post head — long display names
                        // shouldn't push the handle into a wrap-jumble.
                        div { class: "embed__quote-identity",
                            span {
                                class: "embed__quote-name embed__quote-name--clickable",
                                onclick: open_quoted_profile_name,
                                "{name}"
                            }
                            span { class: "embed__quote-handle", "@{author.handle}" }
                        }
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

/// HLS video player. The Dioxus desktop window embeds WKWebView (via
/// wry on macOS) which decodes Bluesky's HLS .m3u8 playlists natively
/// — no hls.js, no native bridge needed. We just render a real
/// `<video>` element with the playlist URL and let WebKit handle it.
///
/// `preload="none"` is load-bearing: feeds with N video posts would
/// otherwise fan out N concurrent playlist fetches on render, even
/// for videos the user never scrolls to. With `none` the player sits
/// at zero network until the user actually clicks the centered play
/// button (which the browser's native controls show on top of the
/// poster).
///
/// Aspect ratio: we set padding-top so the player's box reserves the
/// right amount of column height before the video metadata loads.
/// Without it, the column would jump as videos come in.
///
/// Linux fallback: WebKit on Linux (via webkit2gtk) generally also
/// has HLS via GStreamer, but quality varies by distro. The fallback
/// is the same UX with the player's `error` event handler swapping
/// to an "open in browser" affordance — TODO if anyone hits it.
#[component]
fn VideoPlayer(
    playlist: String,
    thumb: Option<String>,
    aspect_ratio: Option<(u32, u32)>,
) -> Element {
    let (w, h) = aspect_ratio.unwrap_or((16, 9));
    // Padding-percent trick to reserve aspect-ratio'd space before
    // the video's intrinsic dimensions are known. (We don't use
    // `aspect-ratio: w/h` CSS because the embed lives inside a
    // grid/flex container that can stretch — this idiom is
    // historically more robust across odd parents.)
    let padding_pct = (h as f32 / w.max(1) as f32) * 100.0;
    let poster_attr = thumb.clone().unwrap_or_default();
    // Same lightbox plumbing as ImageTile — click the expand button
    // (top-right overlay) to pop the video into the full-app
    // lightbox. The inline `<video>` keeps its native controls for
    // play / pause / seek without conflict because the expand
    // button is positioned absolutely above the player.
    let mut lightbox = use_context::<Signal<crate::state::LightboxFocus>>();
    let playlist_for_lb = playlist.clone();
    let poster_for_lb = thumb.clone();
    let open_lightbox = move |e: MouseEvent| {
        e.stop_propagation();
        lightbox.set(crate::state::LightboxFocus(Some(
            crate::state::LightboxItem::Video {
                url: playlist_for_lb.clone(),
                poster: poster_for_lb.clone(),
            },
        )));
    };
    rsx! {
        div { class: "embed__video",
            style: "padding-top: {padding_pct}%;",
            // No `crossorigin` attr — that would force a CORS preflight
            // on the .m3u8 playlist, and bsky's video CDN doesn't ship
            // `Access-Control-Allow-Origin` for those, so the playlist
            // would silently fail to load (controls show but Play
            // does nothing). Same reasoning for `referrerpolicy` —
            // forcing no-referrer can break hotlink-protected CDNs.
            // The lightbox <video> has neither attr and works; this
            // matches that config.
            video {
                class: "embed__video-el",
                src: "{playlist}",
                poster: "{poster_attr}",
                controls: true,
                preload: "metadata",
                "playsinline": "true",
            }
            button { class: "embed__video-expand",
                title: "Open in lightbox",
                onclick: open_lightbox,
                crate::icons::Expand { size: crate::icons::Size::Sm }
            }
        }
    }
}
