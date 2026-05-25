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

use crate::alt_text::{merge_descriptions, AltSuggestion, AltTextProvider, SmooLlmAltText};
use crate::auth_refresh::fresh_client;
use crate::icons;
use crate::image_prep::{prepare_from_path, PreparedImage};
use crate::ocr;
use crate::state::ComposeContext;
use dioxus::prelude::*;
use smooblue_atproto::{AspectRatio, BlobRef, PostImage, ReplyRef, StrongRef};
use smooblue_oauth::Session;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
    /// `true` once the user has typed in the alt field — locks out
    /// AI-suggested overwrites so we don't fight their edits.
    pub alt_user_edited: bool,
    pub state: AttachmentState,
    /// AI-suggested alt-text (LLM scene description). Filled in
    /// asynchronously after the image becomes Ready.
    pub ai_suggestion: Option<AltSuggestion>,
    /// `true` while the LLM describe call is in flight — shows a small
    /// spinner badge on the alt input.
    pub ai_in_flight: bool,
    /// Literal text extracted by Apple Vision OCR. Merged with
    /// `ai_suggestion.text` into the alt field via [`merge_descriptions`].
    pub ocr_text: Option<String>,
    /// `true` while the OCR task is in flight (macOS only).
    pub ocr_in_flight: bool,
}

impl AttachedImage {
    fn new(path: PathBuf) -> Self {
        Self {
            id: ATTACHMENT_ID.fetch_add(1, Ordering::SeqCst),
            source_path: path,
            alt: String::new(),
            alt_user_edited: false,
            state: AttachmentState::Preparing,
            ai_suggestion: None,
            ai_in_flight: false,
            ocr_text: None,
            ocr_in_flight: false,
        }
    }

    /// Compute what the alt field SHOULD show given the current LLM +
    /// OCR results. Returns `None` if neither has resolved yet.
    fn computed_alt(&self) -> Option<String> {
        let llm = self.ai_suggestion.as_ref().map(|s| s.text.as_str());
        let ocr = self.ocr_text.as_deref();
        if llm.is_none() && ocr.is_none() {
            return None;
        }
        let merged = merge_descriptions(llm, ocr);
        if merged.is_empty() {
            None
        } else {
            Some(merged)
        }
    }
}

#[component]
pub fn ComposeSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut ctx = use_context::<Signal<ComposeContext>>();
    // Load any saved draft so users don't lose work across launches.
    // Skipped in demo mode (we always want a clean slate for screenshots)
    // and when a reply is in flight (draft would belong to a top-level
    // post, not a specific reply target).
    let mut text = use_signal(|| {
        if crate::demo::is_active() {
            return String::new();
        }
        crate::persistence::load_draft().unwrap_or_default()
    });
    let attachments = use_signal::<Vec<AttachedImage>>(Vec::new);
    let mut posting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Debug helper: SMOOBLUE_DEBUG_ATTACH=/path/to/image.jpg injects a
    // synthetic attachment on first render so screenshots and UI
    // iteration don't require clicking through the OS file picker.
    // Hook runs unconditionally (before the open-check) per Dioxus rules.
    use_hook(|| {
        if let Ok(p) = std::env::var("SMOOBLUE_DEBUG_ATTACH") {
            let mut attachments = attachments;
            let path = PathBuf::from(p);
            if path.is_file() {
                spawn(async move {
                    inject_synthetic_attachment(&mut attachments, path).await;
                });
            }
        }
    });

    let snap = ctx.read().clone();
    if !snap.open {
        return rsx! { Fragment {} };
    }

    let reply_to = snap.reply_to.clone();
    let quote_to = snap.quote_to.clone();

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
        let quote = ctx.read().quote_to.as_ref().map(|q| StrongRef {
            uri: q.uri.clone(),
            cid: q.cid.clone(),
        });
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
                let _ = crate::persistence::save_draft("");
                attachments.set(Vec::new());
                let mut w = ctx.write();
                w.reply_to = None;
            w.quote_to = None;
                w.open = false;
                return;
            }
            let Some(client) = fresh_client(session).await else {
                posting.set(false);
                error.set(Some("Session expired — please sign in again.".into()));
                return;
            };

            // Upload each prepared image, building up a PostImage list.
            // We stop at the first failure so the user doesn't get a
            // half-attached post.
            let mut images: Vec<PostImage> = Vec::with_capacity(to_upload.len());
            for (prep, alt) in to_upload {
                let blob: BlobRef = match client.upload_blob(prep.bytes.clone(), &prep.mime).await {
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

            // Detect @mentions / links / #hashtags + resolve handles
            // to DIDs before posting. Failure here (network blip on
            // resolveHandle) silently degrades to a plain-text post
            // rather than blocking the user — they'd much rather
            // their post go through than see "couldn't resolve
            // @alice, please retry."
            let facets = client
                .build_facets_from_text(&body)
                .await
                .unwrap_or_default();
            let result = client
                .create_post_full(&body, reply.as_ref(), &images, &facets, quote.as_ref())
                .await;
            match result {
                Ok(_record) => {
                    posting.set(false);
                    text.set(String::new());
                    // Drop the persisted draft now that the post is
                    // live — nothing left to recover.
                    let _ = crate::persistence::save_draft("");
                    attachments.set(Vec::new());
                    let mut w = ctx.write();
                    w.reply_to = None;
            w.quote_to = None;
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
            w.quote_to = None;
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
            // Resolve once per pick — the env-derived endpoint can't
            // change mid-session anyway.
            let llm: Option<Arc<dyn AltTextProvider>> =
                SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
            for path in files.into_iter().take(remaining_slots) {
                let att = AttachedImage::new(path.clone());
                let id = att.id;
                attachments.write().push(att);
                let atts = attachments;
                let llm_for_image = llm.clone();
                spawn(async move {
                    process_attachment(atts, id, path, llm_for_image).await;
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
                if let Some(q) = quote_to.as_ref() {
                    div { class: "compose__quote-context",
                        div { class: "compose__reply-author",
                            "Quoting "
                            span { class: "compose__reply-handle", "@{q.handle}" }
                        }
                        p { class: "compose__reply-text", "{q.text}" }
                    }
                }
                textarea {
                    class: "{textarea_class}",
                    placeholder: "{placeholder}",
                    autofocus: true,
                    value: "{text}",
                    oninput: move |e| {
                        let v = e.value();
                        // Persist on every keystroke — file write is
                        // cheap, max 300 chars, and the alternative is
                        // a debounce that loses the last second of
                        // typing if the user quits suddenly.
                        if !crate::demo::is_active() {
                            let _ = crate::persistence::save_draft(&v);
                        }
                        text.set(v);
                    },
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
            slot.alt_user_edited = true;
        }
    };

    let mut atts_for_use_suggestion = attachments;
    let use_suggestion = move |_| {
        if let Some(slot) = atts_for_use_suggestion
            .write()
            .iter_mut()
            .find(|a| a.id == id)
        {
            // Reset to the best auto-fill (merged LLM+OCR when both
            // exist, otherwise whichever single source we have).
            if let Some(merged) = slot.computed_alt() {
                slot.alt = merged;
                slot.alt_user_edited = true;
            }
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

    // Decide which alt-text chip to show. Pre-computed here so the
    // rsx! block stays declarative.
    let has_llm = att.ai_suggestion.is_some();
    let has_ocr = att.ocr_text.is_some();
    let merged_alt = att.computed_alt().unwrap_or_default();
    let llm_text = att
        .ai_suggestion
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default();
    let ocr_text_clone = att.ocr_text.clone().unwrap_or_default();
    enum ChipState {
        Combined,                 // alt = merged LLM+OCR
        AiOnly,                   // alt = LLM-only suggestion
        OcrOnly,                  // alt = OCR-only text
        UseAi { combined: bool }, // user edited, offer revert
        None,                     // nothing to show
    }
    let chip = if att.ai_in_flight || att.ocr_in_flight {
        ChipState::None // busy state rendered separately
    } else if has_llm && has_ocr && !merged_alt.is_empty() && att.alt == merged_alt {
        ChipState::Combined
    } else if has_llm && !llm_text.is_empty() && att.alt == llm_text {
        ChipState::AiOnly
    } else if has_ocr && !ocr_text_clone.is_empty() && att.alt == ocr_text_clone {
        ChipState::OcrOnly
    } else if has_llm || has_ocr {
        ChipState::UseAi {
            combined: has_llm && has_ocr,
        }
    } else {
        ChipState::None
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
                div { class: "compose__alt-label",
                    span { "Alt text" }
                    if att.ai_in_flight || att.ocr_in_flight {
                        span { class: "compose__alt-ai compose__alt-ai--busy",
                            icons::Sparkles { size: icons::Size::Sm }
                            if att.ai_in_flight && att.ocr_in_flight {
                                "AI describing + reading…"
                            } else if att.ai_in_flight {
                                "AI describing…"
                            } else {
                                "Reading text…"
                            }
                        }
                    } else {
                        match chip {
                            ChipState::Combined => rsx! {
                                span { class: "compose__alt-ai compose__alt-ai--seeded",
                                    icons::Sparkles { size: icons::Size::Sm }
                                    "AI + text"
                                }
                            },
                            ChipState::AiOnly => rsx! {
                                span { class: "compose__alt-ai compose__alt-ai--seeded",
                                    icons::Sparkles { size: icons::Size::Sm }
                                    "AI suggested"
                                }
                            },
                            ChipState::OcrOnly => rsx! {
                                span { class: "compose__alt-ai compose__alt-ai--seeded",
                                    icons::Sparkles { size: icons::Size::Sm }
                                    "From image text"
                                }
                            },
                            ChipState::UseAi { combined } => rsx! {
                                button {
                                    class: "compose__alt-ai compose__alt-ai--use",
                                    title: "Use AI-suggested alt text",
                                    onclick: use_suggestion,
                                    icons::Sparkles { size: icons::Size::Sm }
                                    if combined { "Use AI + text" } else { "Use AI" }
                                }
                            },
                            ChipState::None => rsx! { Fragment {} },
                        }
                    }
                }
                textarea {
                    class: "input compose__alt-input",
                    placeholder: "{placeholder_text}",
                    disabled: matches!(att.state, AttachmentState::Preparing | AttachmentState::Failed(_)),
                    value: "{alt}",
                    oninput: set_alt,
                }
                div { class: "compose__alt-meta",
                    span { class: "compose__alt-counter", "{alt_len}" }
                    if alt.trim().is_empty() && matches!(att.state, AttachmentState::Ready(_)) && !att.ai_in_flight {
                        span { class: "compose__alt-hint", "alt text helps screen readers" }
                    }
                }
            }
        }
    }
}

/// Debug-only: synthesize an AttachedImage from a path on disk, run
/// the same pipeline as the real picker. Used by
/// SMOOBLUE_DEBUG_ATTACH for screenshots.
async fn inject_synthetic_attachment(attachments: &mut Signal<Vec<AttachedImage>>, path: PathBuf) {
    let llm: Option<Arc<dyn AltTextProvider>> =
        SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
    let att = AttachedImage::new(path.clone());
    let id = att.id;
    attachments.write().push(att);
    process_attachment(*attachments, id, path, llm).await;
}

/// Single shared pipeline for a freshly-added attachment: prep image,
/// then in parallel run LLM describe + Apple Vision OCR. As each
/// finishes, write the result into the slot AND recompute the merged
/// alt text (unless the user has already typed). Idempotent if either
/// task fails — we just leave the slot's field empty.
async fn process_attachment(
    attachments: Signal<Vec<AttachedImage>>,
    id: u64,
    path: PathBuf,
    llm: Option<Arc<dyn AltTextProvider>>,
) {
    let mut atts = attachments;
    let path_for_prep = path.clone();
    let prep_result = tokio::task::spawn_blocking(move || prepare_from_path(&path_for_prep)).await;
    let (state, ready_bytes) = match prep_result {
        Ok(Ok(prep)) => {
            let bytes = prep.bytes.clone();
            let mime = prep.mime.clone();
            (AttachmentState::Ready(prep), Some((bytes, mime)))
        }
        Ok(Err(e)) => (AttachmentState::Failed(format!("{e:#}")), None),
        Err(e) => (
            AttachmentState::Failed(format!("prep task panicked: {e}")),
            None,
        ),
    };
    let has_llm = llm.is_some();
    let cfg_ocr = cfg!(target_os = "macos");
    if let Some(slot) = atts.write().iter_mut().find(|a| a.id == id) {
        slot.state = state;
        if ready_bytes.is_some() && has_llm {
            slot.ai_in_flight = true;
        }
        if ready_bytes.is_some() && cfg_ocr {
            slot.ocr_in_flight = true;
        }
    }
    let Some((bytes, mime)) = ready_bytes else {
        return;
    };

    // Kick off LLM + OCR in parallel. Two tokio joins so either can
    // complete independently and update the alt incrementally.
    let bytes_for_ocr = bytes.clone();
    let mut atts_ocr = attachments;
    let ocr_task = spawn(async move {
        let extracted =
            tokio::task::spawn_blocking(move || ocr::extract_text_joined(&bytes_for_ocr))
                .await
                .ok()
                .flatten();
        if let Some(slot) = atts_ocr.write().iter_mut().find(|a| a.id == id) {
            slot.ocr_in_flight = false;
            slot.ocr_text = extracted;
            if !slot.alt_user_edited {
                if let Some(merged) = slot.computed_alt() {
                    slot.alt = merged;
                }
            }
        }
    });
    let mut atts_llm = attachments;
    let llm_task = spawn(async move {
        if let Some(provider) = llm {
            let suggestion = provider.describe(&bytes, &mime).await.ok();
            if let Some(slot) = atts_llm.write().iter_mut().find(|a| a.id == id) {
                slot.ai_in_flight = false;
                if suggestion.is_some() {
                    slot.ai_suggestion = suggestion;
                    if !slot.alt_user_edited {
                        if let Some(merged) = slot.computed_alt() {
                            slot.alt = merged;
                        }
                    }
                }
            }
        }
    });
    let _ = ocr_task;
    let _ = llm_task;
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

    let stroke = if ratio >= 0.93 {
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
