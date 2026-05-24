//! Compose sheet — modal post / reply composition with image attachments.
//!
//! Two modes:
//! - *Top-level post* — what the FAB opens.
//! - *Reply* — opens via the reply icon on a PostCard. Same sheet,
//!   shows the parent text as quoted context above the textarea and
//!   submits with a reply ref attached.
//!
//! UX niceties beyond a bare textarea:
//! - **Progress ring** counter around the remaining-chars number.
//!   Goes from teal → orange → red as the post approaches the 300
//!   limit. Tabular-numeric digits so the number doesn't jitter.
//! - **⌘↵ / Ctrl↵** submits without leaving the textarea.
//! - **Draft persistence** — the in-progress text + attachments survive
//!   closing the sheet, only clearing on successful submit.
//! - Bigger textarea + smoo-orange focus ring (in CSS).
//! - **Image attachments** — up to 4 per post. Native file picker,
//!   thumbnail grid, per-image alt-text input. Hooks (in follow-up
//!   pearls) for Apple Vision OCR + Smoo LLM auto-alt seeding.

use crate::icons;
use crate::image_prep::{prepare_from_path, PreparedImage};
use crate::state::ComposeContext;
use dioxus::prelude::*;
use smooblue_atproto::{
    AspectRatio, AtClient, BlobRef, PostImage, ReplyRef, StrongRef,
};
use smooblue_oauth::Session;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

/// Bluesky's hard post length cap (graphemes, but we count chars as a proxy).
pub const MAX_LEN: usize = 300;

/// Per-post image cap from the `app.bsky.embed.images` lexicon.
pub const MAX_IMAGES: usize = 4;

static ATTACHMENT_ID: AtomicU64 = AtomicU64::new(1);

/// In-flight state of a single image attachment.
///
/// We do CPU-bound prep (decode → downscale → JPEG re-encode) on a
/// background task so the UI stays responsive. Once `Ready`, the
/// `PreparedImage` carries everything needed to (a) render a thumbnail
/// and (b) upload via [`AtClient::upload_blob`].
#[derive(Clone, PartialEq)]
pub enum AttachmentState {
    /// Decoding / re-encoding in progress.
    Preparing,
    /// Ready to upload.
    Ready(PreparedImage),
    /// Decode failed — the message goes to the user.
    Failed(String),
}

#[derive(Clone, PartialEq)]
pub struct AttachedImage {
    pub id: u64,
    pub source_path: PathBuf,
    /// Screen-reader description. Starts empty; the user types it
    /// (and in follow-up pearls, OCR/LLM seed it).
    pub alt: String,
    pub state: AttachmentState,
}

impl AttachedImage {
    fn new(path: PathBuf) -> Self {
        Self {
            id: ATTACHMENT_ID.fetch_add(1, Ordering::SeqCst),
            source_path: path,
            alt: String::new(),
            state: AttachmentState::Preparing,
        }
    }
}

#[component]
pub fn ComposeSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut ctx = use_context::<Signal<ComposeContext>>();
    let mut text = use_signal(String::new);
    let attachments = use_signal::<Vec<AttachedImage>>(Vec::new);
    let mut posting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let snap = ctx.read().clone();
    if !snap.open {
        return rsx! { Fragment {} };
    }

    let reply_to = snap.reply_to.clone();

    let len = text.read().chars().count();
    let remaining = MAX_LEN as i64 - len as i64;
    let over = remaining < 0;
    let attachments_snap = attachments.read().clone();
    let has_attachments = !attachments_snap.is_empty();
    let any_preparing = attachments_snap
        .iter()
        .any(|a| matches!(a.state, AttachmentState::Preparing));
    let any_failed = attachments_snap
        .iter()
        .any(|a| matches!(a.state, AttachmentState::Failed(_)));
    // A post is "empty" only if there's no text AND no attached image.
    // (Image-only posts are valid on bsky.)
    let empty = text.read().trim().is_empty() && !has_attachments;
    let at_image_cap = attachments_snap.len() >= MAX_IMAGES;

    // Submit flow (shared by button click + ⌘↵ keyboard shortcut).
    let do_submit = move || {
        let len_now = text.read().chars().count();
        if len_now > MAX_LEN {
            return;
        }
        let attachments_now = attachments.read().clone();
        let no_text = text.read().trim().is_empty();
        if no_text && attachments_now.is_empty() {
            return;
        }
        let body = text.read().clone();
        let sess = session.read().clone();
        let reply = ctx.read().reply_to.as_ref().map(|p| ReplyRef {
            root: StrongRef {
                uri: p.uri.clone(),
                cid: p.cid.clone(),
            },
            parent: StrongRef {
                uri: p.uri.clone(),
                cid: p.cid.clone(),
            },
        });
        // Only Ready attachments get sent. If any are still preparing,
        // the button is disabled, so this branch only runs when all are
        // either Ready or Failed (and we filter Failed out).
        let to_upload: Vec<(PreparedImage, String)> = attachments_now
            .into_iter()
            .filter_map(|a| match a.state {
                AttachmentState::Ready(p) => Some((p, a.alt)),
                _ => None,
            })
            .collect();

        posting.set(true);
        error.set(None);
        let mut posting = posting;
        let mut text = text;
        let mut attachments = attachments;
        let mut error = error;
        let mut ctx = ctx;
        spawn(async move {
            if crate::demo::is_active() || sess.is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                posting.set(false);
                text.set(String::new());
                attachments.set(Vec::new());
                let mut w = ctx.write();
                w.reply_to = None;
                w.open = false;
                return;
            }
            let s = sess.unwrap();
            let base = match Url::parse(&s.pds) {
                Ok(u) => u,
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Bad PDS URL: {e}")));
                    return;
                }
            };
            let client = AtClient::new(s, base);

            // Upload each prepared image, building up a PostImage list.
            // We stop at the first failure so the user doesn't get a
            // half-attached post.
            let mut images: Vec<PostImage> = Vec::with_capacity(to_upload.len());
            for (prep, alt) in to_upload {
                let blob: BlobRef = match client
                    .upload_blob(prep.bytes.clone(), &prep.mime)
                    .await
                {
                    Ok(b) => b,
                    Err(e) => {
                        posting.set(false);
                        error.set(Some(format!("Image upload failed: {e}")));
                        return;
                    }
                };
                images.push(PostImage {
                    blob,
                    alt,
                    aspect_ratio: Some(AspectRatio {
                        width: prep.width,
                        height: prep.height,
                    }),
                });
            }

            let result = client
                .create_post_full(&body, reply.as_ref(), &images)
                .await;
            match result {
                Ok(_record) => {
                    posting.set(false);
                    text.set(String::new());
                    attachments.set(Vec::new());
                    let mut w = ctx.write();
                    w.reply_to = None;
                    w.open = false;
                }
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Couldn't post: {e}")));
                }
            }
        });
    };

    let mut do_submit_btn = do_submit;
    let mut do_submit_kbd = do_submit;

    let close = move |_evt| {
        let mut w = ctx.write();
        w.reply_to = None;
        w.open = false;
    };

    // "+ Image" picker — sync rfd in spawn_blocking, then prep on a
    // background blocking task (JPEG re-encode is CPU-bound).
    let pick_images = move |_| {
        let mut attachments = attachments;
        spawn(async move {
            let already = attachments.read().len();
            let remaining_slots = MAX_IMAGES.saturating_sub(already);
            if remaining_slots == 0 {
                return;
            }
            let files = tokio::task::spawn_blocking(move || {
                rfd::FileDialog::new()
                    .add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif", "heic"])
                    .set_title("Attach images")
                    .pick_files()
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            for path in files.into_iter().take(remaining_slots) {
                let att = AttachedImage::new(path.clone());
                let id = att.id;
                attachments.write().push(att);
                // Kick off CPU-bound prep on the blocking pool.
                let mut atts = attachments;
                let path_for_prep = path.clone();
                spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        prepare_from_path(&path_for_prep)
                    })
                    .await;
                    let state = match result {
                        Ok(Ok(prep)) => AttachmentState::Ready(prep),
                        Ok(Err(e)) => AttachmentState::Failed(format!("{e:#}")),
                        Err(e) => AttachmentState::Failed(format!("prep task panicked: {e}")),
                    };
                    if let Some(slot) = atts.write().iter_mut().find(|a| a.id == id) {
                        slot.state = state;
                    }
                });
            }
        });
    };

    let placeholder = if reply_to.is_some() {
        "Write your reply…"
    } else {
        "What's up?"
    };
    let title_text = if reply_to.is_some() {
        "Reply"
    } else {
        "New post"
    };
    let button_text = if reply_to.is_some() { "Reply" } else { "Post" };

    let textarea_class = if over {
        "input input--lg compose__textarea compose__textarea--over"
    } else {
        "input input--lg compose__textarea"
    };

    let post_disabled = empty || over || any_preparing || any_failed || *posting.read();

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet compose__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "compose__head",
                    span { class: "compose__title", "{title_text}" }
                    button { class: "compose__close",
                        title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                if let Some(parent) = reply_to.as_ref() {
                    div { class: "compose__reply-context",
                        div { class: "compose__reply-author",
                            "Replying to "
                            span { class: "compose__reply-handle", "@{parent.handle}" }
                        }
                        p { class: "compose__reply-text", "{parent.text}" }
                    }
                }
                textarea {
                    class: "{textarea_class}",
                    placeholder: "{placeholder}",
                    autofocus: true,
                    value: "{text}",
                    oninput: move |e| text.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && (e.modifiers().meta() || e.modifiers().ctrl()) {
                            do_submit_kbd();
                        }
                    },
                }
                if has_attachments {
                    AttachmentGrid { attachments }
                }
                div { class: "compose__bar",
                    button {
                        class: if at_image_cap { "compose__attach compose__attach--disabled" } else { "compose__attach" },
                        title: if at_image_cap { "Image limit reached (4 max)" } else { "Attach image" },
                        disabled: at_image_cap,
                        onclick: pick_images,
                        icons::ImageIcon { size: icons::Size::Sm }
                    }
                    ProgressRing { used: len, max: MAX_LEN }
                    span {
                        class: if over { "compose__counter compose__counter--over" } else { "compose__counter" },
                        "{remaining}"
                    }
                    span { class: "compose__hint",
                        if cfg!(target_os = "macos") { "⌘↵" } else { "Ctrl↵" }
                        " to post"
                    }
                    button {
                        class: "btn btn--primary compose__post",
                        disabled: post_disabled,
                        onclick: move |_| do_submit_btn(),
                        if *posting.read() {
                            if has_attachments { "Uploading…" } else { "Posting…" }
                        } else {
                            "{button_text}"
                        }
                    }
                }
                if let Some(msg) = &*error.read() {
                    div { class: "compose__error", "{msg}" }
                }
            }
        }
    }
}

/// Thumbnail grid for attached images. Each tile has a preview, an
/// alt-text textarea, and a small "X" to remove.
#[component]
fn AttachmentGrid(attachments: Signal<Vec<AttachedImage>>) -> Element {
    let snapshot = attachments.read().clone();
    rsx! {
        div { class: "compose__attachments",
            for att in snapshot {
                AttachmentTile { att: att.clone(), attachments }
            }
        }
    }
}

#[component]
fn AttachmentTile(att: AttachedImage, attachments: Signal<Vec<AttachedImage>>) -> Element {
    let id = att.id;
    let alt = att.alt.clone();

    let mut atts = attachments;
    let remove = move |_| {
        atts.write().retain(|a| a.id != id);
    };

    let mut atts_for_alt = attachments;
    let set_alt = move |evt: Event<FormData>| {
        let new_alt = evt.value();
        if let Some(slot) = atts_for_alt.write().iter_mut().find(|a| a.id == id) {
            slot.alt = new_alt;
        }
    };

    let preview = match &att.state {
        AttachmentState::Preparing => rsx! {
            div { class: "compose__thumb compose__thumb--preparing",
                span { class: "compose__thumb-spinner" }
            }
        },
        AttachmentState::Ready(prep) => rsx! {
            img {
                class: "compose__thumb",
                src: "{prep.thumb_data_uri}",
                alt: "Attached image preview",
            }
        },
        AttachmentState::Failed(msg) => rsx! {
            div { class: "compose__thumb compose__thumb--failed",
                title: "{msg}",
                "!"
            }
        },
    };

    let alt_len = alt.chars().count();
    let placeholder_text = match &att.state {
        AttachmentState::Preparing => "Preparing image…",
        AttachmentState::Failed(_) => "Image failed to load",
        AttachmentState::Ready(_) => "Describe this image for screen readers…",
    };

    rsx! {
        div { class: "compose__attachment",
            div { class: "compose__attachment-preview",
                {preview}
                button {
                    class: "compose__attachment-remove",
                    title: "Remove image",
                    onclick: remove,
                    icons::X { size: icons::Size::Sm }
                }
            }
            div { class: "compose__attachment-meta",
                textarea {
                    class: "input compose__alt-input",
                    placeholder: "{placeholder_text}",
                    disabled: matches!(att.state, AttachmentState::Preparing | AttachmentState::Failed(_)),
                    value: "{alt}",
                    oninput: set_alt,
                }
                div { class: "compose__alt-meta",
                    span { class: "compose__alt-counter", "{alt_len}" }
                    if alt.trim().is_empty() && matches!(att.state, AttachmentState::Ready(_)) {
                        span { class: "compose__alt-hint", "alt text helps screen readers" }
                    }
                }
            }
        }
    }
}

/// SVG progress ring for the character counter. As `used` approaches
/// `max`, the ring fills and shifts hue from teal → orange → red.
#[component]
fn ProgressRing(used: usize, max: usize) -> Element {
    const R: f32 = 9.0;
    const STROKE: f32 = 2.2;
    let cx = R + STROKE;
    let circumference = 2.0 * std::f32::consts::PI * R;

    let ratio = (used as f32 / max as f32).min(1.5);
    let filled = (circumference * ratio.min(1.0)).min(circumference);
    let dash = format!("{filled} {circumference}");

    let stroke = if ratio >= 1.0 {
        "var(--color-smooai-red)"
    } else if ratio >= 0.93 {
        "var(--color-smooai-red)"
    } else if ratio >= 0.80 {
        "var(--color-smooai-orange)"
    } else {
        "var(--color-smooai-teal, #00a6a6)"
    };

    let size = (R + STROKE) * 2.0;
    rsx! {
        svg {
            class: "compose__ring",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 {size} {size}",
            circle {
                cx: "{cx}",
                cy: "{cx}",
                r: "{R}",
                fill: "none",
                stroke: "var(--border)",
                stroke_width: "{STROKE}",
            }
            circle {
                cx: "{cx}",
                cy: "{cx}",
                r: "{R}",
                fill: "none",
                stroke: "{stroke}",
                stroke_width: "{STROKE}",
                stroke_linecap: "round",
                stroke_dasharray: "{dash}",
                transform: "rotate(-90 {cx} {cx})",
            }
        }
    }
}
