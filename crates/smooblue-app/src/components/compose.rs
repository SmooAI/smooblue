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
use smooblue_atproto::{
    ActorProfile, AspectRatio, BlobRef, FacetKind, LinkCard, PostExternal, PostImage, PostVideo,
    ReplyRef, StrongRef,
};
use smooblue_oauth::Session;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Bluesky's hard post length cap (graphemes, but we count chars as a proxy).
pub const MAX_LEN: usize = 300;

/// Per-post image cap from the `app.bsky.embed.images` lexicon.
pub const MAX_IMAGES: usize = 4;

/// Per-image alt-text cap from the `app.bsky.embed.images#image.alt`
/// lexicon (graphemes — we approximate with chars). Going over this
/// makes the AppView reject the post with a validation error; the
/// LLM auto-suggestion path can produce long descriptions, so we
/// truncate proactively rather than failing at submit time.
pub const MAX_ALT_LEN: usize = 2000;

/// Truncate `s` to at most [`MAX_ALT_LEN`] chars. Char-based so we
/// don't slice a UTF-8 codepoint in half on the byte boundary.
fn truncate_alt(s: String) -> String {
    if s.chars().count() <= MAX_ALT_LEN {
        return s;
    }
    s.chars().take(MAX_ALT_LEN).collect()
}

/// True if `c` is a valid character inside a Bluesky handle. Per the
/// atproto handle grammar, handles are dot-separated alphanumeric
/// labels with `-` allowed inside; that means inside a single label
/// (which is what the user is mid-typing) the legal chars are
/// `[a-zA-Z0-9._-]`. We accept all of those so partials like
/// `@foo.bar.bsky.so` get recognized as one mention prefix instead
/// of being split at the first dot.
fn is_handle_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'
}

/// Extract the active `@mention` partial from the END of the
/// textarea's text — the heuristic for "user is mid-typing a
/// mention." Returns `None` if the last sequence isn't a trailing
/// `@<handle-chars>` (i.e. there's whitespace after it, or there's
/// no `@` near the end at all). The `@` must be either at the very
/// start of the text or preceded by whitespace, so mid-word `@`
/// (like an email or an `at` in a sentence — though those are rare)
/// doesn't accidentally pop the popover. Returns the partial
/// AFTER the `@`, or `Some("")` immediately after typing `@`.
pub fn active_mention_prefix(text: &str) -> Option<&str> {
    let trail_handle_start = text
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_handle_char(*c))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    // Char immediately before the handle run must be `@`.
    let at_pos = text[..trail_handle_start].chars().last()?;
    if at_pos != '@' {
        return None;
    }
    let at_byte = trail_handle_start - '@'.len_utf8();
    // The `@` itself must be at-string-start or after whitespace.
    if at_byte > 0 {
        let prev = text[..at_byte].chars().last()?;
        if !prev.is_whitespace() {
            return None;
        }
    }
    Some(&text[trail_handle_start..])
}

/// Replace the trailing `@<partial>` (as identified by
/// [`active_mention_prefix`]) with `@<full_handle> `. Returns the
/// new text; if no active mention is present, returns the input
/// unchanged. Adds a trailing space so the user can keep typing
/// without manually breaking out of the popover.
pub fn replace_mention_prefix(text: &str, full_handle: &str) -> String {
    let Some(partial) = active_mention_prefix(text) else {
        return text.to_string();
    };
    // Strip the partial AND the leading `@` to get the prefix.
    let cut = text.len() - partial.len() - '@'.len_utf8();
    let mut out = String::with_capacity(cut + 2 + full_handle.len());
    out.push_str(&text[..cut]);
    out.push('@');
    out.push_str(full_handle);
    out.push(' ');
    out
}

/// First link URL in `text`, if any — drives the link-card preview.
/// Reuses the same facet detector the post pipeline uses so the card
/// we preview matches the link facet we'll actually publish (no
/// second, divergent URL regex). Returns the first http(s) link.
pub fn first_link_url(text: &str) -> Option<String> {
    smooblue_atproto::detect_facet_candidates(text)
        .into_iter()
        .find_map(|c| match c.kind {
            FacetKind::Link { uri } => Some(uri),
            _ => None,
        })
}

/// How many actors to pull from the typeahead before re-ranking. We
/// fetch wider than we show (`MENTION_SHOWN`) so a mutual buried at
/// position 15 by the server's ordering can still be promoted into
/// the visible list by [`rank_mention_results`].
const MENTION_FETCH: u32 = 25;
/// How many ranked rows the popover shows.
const MENTION_SHOWN: usize = 8;

/// Re-rank @mention typeahead results. Bluesky's
/// `searchActorsTypeahead` is only lightly personalized, so on its own
/// it buries people you actually talk to under big strangers who
/// happen to prefix-match. We re-sort by, in order:
///
/// 1. **Relationship** — mutuals first, then people you follow, then
///    people who follow you, then strangers. This is the "followers /
///    followed" bias.
/// 2. **Match quality** — a prefix match on the handle or display name
///    beats a mid-string match; handle ties beat display-name ties.
/// 3. **Server order** — preserved for equal scores (it already
///    factors in popularity), via a stable decorate-sort.
///
/// Case-insensitive. `query` is the text after the `@` (no leading
/// `@`).
pub fn rank_mention_results(results: Vec<ActorProfile>, query: &str) -> Vec<ActorProfile> {
    let q = query.trim().to_lowercase();
    let mut scored: Vec<(i32, usize, ActorProfile)> = results
        .into_iter()
        .enumerate()
        .map(|(i, a)| (mention_score(&a, &q), i, a))
        .collect();
    // Descending score; ties fall back to the server's original index
    // so the sort stays stable and predictable.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, a)| a).collect()
}

/// Score one actor for the active query. Relationship is the dominant
/// axis (×100) so someone you follow with a weaker textual match still
/// outranks a stranger — that's the whole point of the follows bias.
fn mention_score(a: &ActorProfile, q: &str) -> i32 {
    let rel = a.viewer.as_ref().map_or(0, |v| {
        match (v.following.is_some(), v.followed_by.is_some()) {
            (true, true) => 3,   // mutual
            (true, false) => 2,  // you follow them
            (false, true) => 1,  // they follow you
            (false, false) => 0, // stranger
        }
    });
    let handle = a.handle.to_lowercase();
    let name = a.display_name.as_deref().unwrap_or("").to_lowercase();
    let m = if q.is_empty() {
        0
    } else if handle.starts_with(q) {
        4
    } else if name.starts_with(q) {
        3
    } else if handle.contains(q) {
        2
    } else if name.contains(q) {
        1
    } else {
        0
    };
    rel * 100 + m * 10
}

/// Hard cap on dropped video file size before we accept it. Matches
/// bsky's own `app.bsky.video.uploadVideo` ceiling — files above
/// this would 413 at the AppView even if we managed to upload them,
/// and reading them blocks the renderer thread. Surface a clear
/// error instead of letting the drop silently swallow a 4 GB file.
pub const MAX_VIDEO_BYTES: u64 = 50 * 1024 * 1024;

static ATTACHMENT_ID: AtomicU64 = AtomicU64::new(1);

/// Single attached video. Mutually exclusive with images. Held as
/// raw bytes in memory until submit; bsky's lexicon caps video at
/// ~50MB so the in-memory load is fine for normal usage.
#[derive(Clone, PartialEq)]
pub struct VideoAttachment {
    pub source_path: PathBuf,
    pub bytes: Vec<u8>,
    pub mime: String,
    pub alt: String,
}

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
    /// OCR results. Returns `None` if neither has resolved yet. The
    /// merged result is truncated to [`MAX_ALT_LEN`] chars — the LLM
    /// can produce 3-4k-char scene descriptions, and Bluesky's
    /// `app.bsky.embed.images#image.alt` field rejects anything
    /// over 2000 graphemes at submit time.
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
            Some(truncate_alt(merged))
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
    // Consume a one-shot prefill handed off from another surface (the
    // inbox quick-reply "pop out" button) so an in-progress reply isn't
    // lost when escalating to the full composer. Runs whenever ctx
    // changes; clearing prefill makes it idempotent.
    use_effect(move || {
        let pf = ctx.read().prefill.clone();
        if let Some(t) = pf {
            if !t.is_empty() {
                text.set(t);
            }
            ctx.write().prefill = None;
        }
    });
    let attachments = use_signal::<Vec<AttachedImage>>(Vec::new);
    // Single video attachment (mutually exclusive with images per
    // the lexicon — bsky records carry one media slot). Holds raw
    // bytes + mime + an editable alt-text field. Empty until the
    // user drops or picks a video file.
    let mut video_attachment = use_signal::<Option<VideoAttachment>>(|| None);
    let mut posting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // Extra posts that get chained as replies to the root after
    // submit. Each entry is the body text of one downstream post.
    // Plain text only — no images / facets / quotes on extras
    // (keeps the UI focused; the root carries the heavy payload).
    let mut thread_extras = use_signal::<Vec<String>>(Vec::new);

    // @mention typeahead state. `mention_query` is the partial after
    // the trailing `@` in the textarea (None when no active mention).
    // The use_effect below debounces it and pushes results into
    // `mention_results`. `mention_selected` tracks the keyboard
    // selection within the popover.
    let mut mention_query = use_signal::<Option<String>>(|| None);
    let mut mention_results = use_signal::<Vec<ActorProfile>>(Vec::new);
    let mut mention_selected = use_signal::<usize>(|| 0);
    // Monotonic search sequence — when a later keystroke kicks off a
    // newer search, older in-flight responses set seq and discover
    // they're stale, dropping their results instead of clobbering
    // newer ones.
    let mut mention_search_seq = use_signal::<u64>(|| 0);

    // Link-card preview state. When the post text contains a URL we
    // fetch its OpenGraph card (via CardyB) and preview it under the
    // textarea with a remove (×). The card is attached at post time
    // only when no image/video is set — those own the single media
    // slot. `link_card_dismissed` holds URLs the user removed so we
    // don't immediately re-fetch them. `link_card_seq` is the same
    // stale-response guard the mention search uses.
    let mut link_card = use_signal::<Option<LinkCard>>(|| None);
    let mut link_card_loading = use_signal(|| false);
    let mut link_card_dismissed = use_signal::<std::collections::HashSet<String>>(Default::default);
    let mut link_card_seq = use_signal::<u64>(|| 0);

    // Debounced typeahead. `mention_query` change → wait 150ms → if
    // the query is still the same (no further keystrokes have
    // superseded it), call the AppView. Failure silently empties the
    // result list — the user can keep typing and just won't get
    // suggestions, which is strictly better than blocking on a
    // network blip.
    use_effect(move || {
        let q_snap = mention_query.read().clone();
        let Some(q) = q_snap.filter(|s| !s.is_empty()) else {
            mention_results.set(Vec::new());
            mention_selected.set(0);
            return;
        };
        let session_for_search = session;
        let seq = {
            let mut s = mention_search_seq.write();
            *s = s.wrapping_add(1);
            *s
        };
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            // Bail if the user has already started a newer query.
            if *mention_search_seq.peek() != seq {
                return;
            }
            // Bail if the query no longer matches (user kept typing
            // past the debounce window and the effect re-fired).
            if mention_query.peek().as_deref() != Some(q.as_str()) {
                return;
            }
            let Some(client) = fresh_client(session_for_search).await else {
                return;
            };
            match client.search_actors_typeahead(&q, MENTION_FETCH).await {
                Ok(actors) => {
                    if *mention_search_seq.peek() == seq {
                        // Bias toward people you follow / who follow
                        // you, then trim to the visible row count.
                        let mut ranked = rank_mention_results(actors, &q);
                        ranked.truncate(MENTION_SHOWN);
                        mention_results.set(ranked);
                        mention_selected.set(0);
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "compose: actor typeahead search failed");
                }
            }
        });
    });

    // Debounced link-card fetch. Watches the post text for the first
    // URL; when it changes (and isn't dismissed) we fetch the card
    // after a short pause. Clears the card when the URL disappears.
    // Failures are silent — a post with a bare link is still fine,
    // it just won't get a card.
    use_effect(move || {
        let url = first_link_url(&text.read());
        // Drop the card if the link is gone or the user dismissed it.
        let Some(url) = url.filter(|u| !link_card_dismissed.read().contains(u)) else {
            if link_card.peek().is_some() {
                link_card.set(None);
            }
            return;
        };
        // Already have (or are loading) this exact card — nothing to do.
        if link_card.peek().as_ref().map(|c| &c.uri) == Some(&url) {
            return;
        }
        let session_for_card = session;
        let seq = {
            let mut s = link_card_seq.write();
            *s = s.wrapping_add(1);
            *s
        };
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            // Superseded by a newer URL? bail.
            if *link_card_seq.peek() != seq {
                return;
            }
            if first_link_url(&text.peek()).as_deref() != Some(url.as_str()) {
                return;
            }
            link_card_loading.set(true);
            let card = match fresh_client(session_for_card).await {
                Some(client) => client.fetch_link_card(&url).await.ok(),
                None => None,
            };
            // Only commit if we're still the newest request.
            if *link_card_seq.peek() == seq {
                link_card_loading.set(false);
                if let Some(card) = card {
                    link_card.set(Some(card));
                }
            }
        });
    });

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

    // File-promise integration with the macOS overlay (file_promise.rs).
    // Two App-level signals feed this:
    //   - pending_drops: VecDeque<PathBuf>  — drops to attach
    //   - promise_drag_active: bool         — true between draggingEntered
    //                                          and draggingExited/drop,
    //                                          drives the --drag highlight
    //                                          on the outer container
    // The use_effect re-runs on either signal changing; we open compose
    // on drag-enter (so the user sees the highlight before they drop)
    // AND on drop landing (when compose is otherwise closed), then
    // drain pending paths through the same image-attachment pipeline.
    let mut pending_drops = use_context::<Signal<std::collections::VecDeque<PathBuf>>>();
    let promise_drag_active = use_context::<Signal<bool>>();
    use_effect(move || {
        let drag_active = *promise_drag_active.read();
        let has_pending = !pending_drops.read().is_empty();
        if !drag_active && !has_pending {
            return;
        }
        let mut atts = attachments;
        let mut ctx_open = ctx;
        let mut error_for_drop = error;
        // Open compose for either: drag-enter (user sees highlight as
        // they hover) or pending-drop (user sees the attached image).
        if !ctx_open.read().open {
            ctx_open.write().open = true;
        }
        if !has_pending {
            return;
        }
        let drained: Vec<PathBuf> = pending_drops.write().drain(..).collect();
        spawn(async move {
            let already = atts.read().len();
            let slots = MAX_IMAGES.saturating_sub(already);
            if slots == 0 {
                error_for_drop.set(Some(format!(
                    "Already at {MAX_IMAGES} images — dropped screenshot ignored."
                )));
                return;
            }
            let llm: Option<Arc<dyn AltTextProvider>> =
                SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
            for path in drained.into_iter().take(slots) {
                if !path.is_file() {
                    continue;
                }
                let att = AttachedImage::new(path.clone());
                let id = att.id;
                atts.write().push(att);
                let llm_for_image = llm.clone();
                spawn(async move {
                    process_attachment(atts, id, path, llm_for_image).await;
                });
            }
        });
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
    let has_video = video_attachment.read().is_some();
    // A post is "empty" only if there's no text AND no attached
    // media. Image-only / video-only posts are valid on bsky.
    let empty = text.read().trim().is_empty() && !has_attachments && !has_video;
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
        let video_snap = video_attachment.read().clone();
        let card_snap = link_card.read().clone();
        let quote = ctx.read().quote_to.as_ref().map(|q| StrongRef {
            uri: q.uri.clone(),
            cid: q.cid.clone(),
        });
        // root = the thread root carried on the ReplyTarget (the
        // ancestor root for a deep reply, or the parent itself for a
        // top-level one); parent = the post being replied to. Setting
        // root = parent here orphaned deep replies — see th-f603e2.
        let reply = ctx.read().reply_to.as_ref().map(|p| ReplyRef {
            root: StrongRef {
                uri: p.root_uri.clone(),
                cid: p.root_cid.clone(),
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
                video_attachment.set(None);
                link_card.set(None);
                link_card_dismissed.write().clear();
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

            // Upload the video blob if present. Mutually exclusive
            // with images per the lexicon — if both were somehow
            // attached we'd hit a 400 on the embed step.
            let video_post: Option<PostVideo> = if let Some(v) = video_snap {
                match client.upload_blob(v.bytes, &v.mime).await {
                    Ok(blob) => Some(PostVideo {
                        video: blob,
                        alt: v.alt,
                        aspect_ratio: None,
                    }),
                    Err(e) => {
                        posting.set(false);
                        error.set(Some(format!("Video upload failed: {e}")));
                        return;
                    }
                }
            } else {
                None
            };

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
            // Build the link-card embed only when nothing else owns the
            // media slot (images / video). Uploading the thumb is
            // best-effort — a failed thumb still posts the card, just
            // without the image.
            let external: Option<PostExternal> = if images.is_empty() && video_post.is_none() {
                if let Some(card) = card_snap {
                    let thumb = match card.image_url.as_deref() {
                        Some(u) => client.upload_link_card_thumb(u).await.ok(),
                        None => None,
                    };
                    Some(PostExternal {
                        uri: card.uri,
                        title: card.title,
                        description: card.description,
                        thumb,
                    })
                } else {
                    None
                }
            } else {
                None
            };
            let result = client
                .create_post_full(
                    &body,
                    reply.as_ref(),
                    &images,
                    &facets,
                    quote.as_ref(),
                    video_post.as_ref(),
                    external.as_ref(),
                )
                .await;
            let root_record = match result {
                Ok(rec) => rec,
                Err(e) => {
                    posting.set(false);
                    error.set(Some(format!("Couldn't post: {e}")));
                    return;
                }
            };

            // Thread continuation — chain each non-empty extra as a
            // reply with root = first post, parent = previous post.
            // If any one fails mid-thread we surface the error but
            // keep the root + any successful intermediate posts:
            // partial threads are better than reverted threads (we
            // can't atomically roll back a published post anyway).
            let extras_snap = thread_extras.read().clone();
            let mut prev: smooblue_atproto::CreatedRecord = root_record.clone();
            let mut thread_error: Option<String> = None;
            for chunk in extras_snap.iter().filter(|c| !c.trim().is_empty()) {
                let reply_chain = smooblue_atproto::ReplyRef {
                    root: smooblue_atproto::StrongRef {
                        uri: root_record.uri.clone(),
                        cid: root_record.cid.clone(),
                    },
                    parent: smooblue_atproto::StrongRef {
                        uri: prev.uri.clone(),
                        cid: prev.cid.clone(),
                    },
                };
                // Re-run facet detection per chunk so mentions /
                // links / tags work in continuation posts too.
                let chunk_facets = client
                    .build_facets_from_text(chunk)
                    .await
                    .unwrap_or_default();
                match client
                    .create_post_full(
                        chunk,
                        Some(&reply_chain),
                        &[],
                        &chunk_facets,
                        None,
                        None,
                        None,
                    )
                    .await
                {
                    Ok(rec) => {
                        prev = rec;
                    }
                    Err(e) => {
                        thread_error = Some(format!(
                            "Posted the first {} of {} — couldn't post the rest: {e}",
                            extras_snap.iter().position(|c| c == chunk).unwrap_or(0) + 1,
                            extras_snap.len() + 1,
                        ));
                        break;
                    }
                }
            }

            posting.set(false);
            if let Some(msg) = thread_error {
                error.set(Some(msg));
            } else {
                text.set(String::new());
                thread_extras.set(Vec::new());
                video_attachment.set(None);
                // Drop the persisted draft now that the post is
                // live — nothing left to recover.
                let _ = crate::persistence::save_draft("");
                attachments.set(Vec::new());
                video_attachment.set(None);
                link_card.set(None);
                link_card_dismissed.write().clear();
                let mut w = ctx.write();
                w.reply_to = None;
                w.quote_to = None;
                w.open = false;
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
        thread_extras.set(Vec::new());
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

    // Drag-and-drop: accept image files dropped anywhere on the
    // compose sheet. Same pipeline as the +Image picker — push an
    // AttachedImage placeholder, then process in the background
    // (decode, generate alt-text, etc.). dragover must call
    // prevent_default or the browser refuses to fire drop.
    let mut dragging = use_signal(|| false);
    let on_dragover = move |e: DragEvent| {
        e.prevent_default();
        if !*dragging.read() {
            dragging.set(true);
        }
    };
    let on_dragleave = move |_| dragging.set(false);
    let on_drop = move |e: DragEvent| {
        use dioxus::html::HasFileData;
        e.prevent_default();
        // Stop the drop from bubbling to the deck-shell window-level
        // handler — when compose is open, the local drop handler
        // attaches the image; the window handler would re-attach the
        // same path via the FilePromiseEvent::Drop channel.
        e.stop_propagation();
        dragging.set(false);
        let Some(file_engine) = e.files() else {
            return;
        };
        let names = file_engine.files();
        let mut attachments_for_drop = attachments;
        spawn(async move {
            let already = attachments_for_drop.read().len();
            let slots = MAX_IMAGES.saturating_sub(already);
            if slots == 0 {
                return;
            }
            let llm: Option<Arc<dyn AltTextProvider>> =
                SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
            for name in names.into_iter().take(slots) {
                // file_engine.files() returns paths on desktop;
                // skip anything that isn't a readable file.
                let path = PathBuf::from(&name);
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                // Video: replaces any prior video attachment (only
                // one video per post per the lexicon). Loaded fully
                // into memory — bsky caps video at ~50MB which is
                // fine to hold; bigger files will OOM the renderer
                // before we even hit the upload step (caller's
                // responsibility to crop / compress).
                if matches!(ext.as_str(), "mp4" | "mov" | "m4v" | "webm") {
                    // Size-gate BEFORE reading so a 4 GB drop can't
                    // OOM the renderer. bsky's own ceiling is 50 MB —
                    // anything bigger would 413 from the AppView
                    // even if we did manage to upload it. Surface a
                    // clear error toast instead of silently dropping.
                    let size = match std::fs::metadata(&path) {
                        Ok(m) => m.len(),
                        Err(_) => continue,
                    };
                    if size > MAX_VIDEO_BYTES {
                        error.set(Some(format!(
                            "Video too large ({:.1} MB). Bluesky caps videos at {} MB.",
                            size as f64 / 1_048_576.0,
                            MAX_VIDEO_BYTES / 1_048_576,
                        )));
                        break;
                    }
                    let mime = match ext.as_str() {
                        "mp4" | "m4v" => "video/mp4",
                        "mov" => "video/quicktime",
                        "webm" => "video/webm",
                        _ => "application/octet-stream",
                    }
                    .to_string();
                    let path_for_read = path.clone();
                    // Read off the renderer thread — even 50 MB of
                    // disk I/O stutters the UI when done synchronously.
                    let bytes =
                        match tokio::task::spawn_blocking(move || std::fs::read(&path_for_read))
                            .await
                        {
                            Ok(Ok(b)) => b,
                            _ => {
                                error.set(Some("Couldn't read the dropped video file.".into()));
                                break;
                            }
                        };
                    video_attachment.set(Some(VideoAttachment {
                        source_path: path,
                        bytes,
                        mime,
                        alt: String::new(),
                    }));
                    // One video per post — don't process more dropped
                    // files; if the user dropped an image alongside
                    // we'd otherwise mix media types.
                    break;
                }
                if !matches!(
                    ext.as_str(),
                    "jpg" | "jpeg" | "png" | "webp" | "gif" | "heic"
                ) {
                    continue;
                }
                let att = AttachedImage::new(path.clone());
                let id = att.id;
                attachments_for_drop.write().push(att);
                let atts = attachments_for_drop;
                let llm_for_image = llm.clone();
                spawn(async move {
                    process_attachment(atts, id, path, llm_for_image).await;
                });
            }
        });
    };

    rsx! {
        // The compose sheet is always the topmost modal: you can
        // open it from inside a thread / profile / engagement sheet
        // (e.g. "Quote post" while reading a thread), and the
        // expectation is that the compose dialog lands ON TOP of
        // whatever you were reading — not buried behind it.
        // `--compose` lifts the z-index above the other sheets.
        div { class: "modal__backdrop modal__backdrop--compose", onclick: close,
            div {
                // `dragging` is the HTML5 dragover signal (Finder file
                // drops); `promise_drag_active` is the AppKit overlay's
                // drag-tracking signal (screenshot floater drops). Either
                // source lights up the same --drag highlight so the user
                // gets consistent visual feedback regardless of where the
                // image came from.
                class: if *dragging.read() || *promise_drag_active.read() {
                    "modal__sheet compose__sheet compose__sheet--drag"
                } else {
                    "modal__sheet compose__sheet"
                },
                onclick: move |e| e.stop_propagation(),
                ondragover: on_dragover,
                ondragleave: on_dragleave,
                ondrop: on_drop,
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
                div { class: "compose__textarea-wrap",
                    textarea {
                        class: "{textarea_class}",
                        placeholder: "{placeholder}",
                        autofocus: true,
                        value: "{text}",
                        oninput: move |e| {
                            let v = e.value();
                            // Update the signal first — the textarea must
                            // reflect the keystroke immediately.
                            text.set(v.clone());
                            // Drive the @mention popover off the same
                            // event so we don't need a second listener.
                            mention_query.set(active_mention_prefix(&v).map(String::from));
                            // Move the file write OFF the render thread.
                            // Was blocking inline before, causing visible
                            // keystroke lag on long drafts / slower disks
                            // (every keystroke = create_dir_all + write).
                            // spawn_blocking is safe + cheap; a few
                            // redundant writes per second is fine, the
                            // file just gets overwritten with the latest.
                            if !crate::demo::is_active() {
                                tokio::task::spawn_blocking(move || {
                                    let _ = crate::persistence::save_draft(&v);
                                });
                            }
                        },
                        onkeydown: move |e| {
                            let popover_open = mention_query.peek().is_some()
                                && !mention_results.peek().is_empty();
                            // Popover-active key handling takes precedence over
                            // the default ⌘↵ submit so the user can pick a
                            // suggestion mid-compose without accidentally posting.
                            if popover_open {
                                let key = e.key();
                                match key {
                                    Key::ArrowDown => {
                                        e.prevent_default();
                                        let len = mention_results.peek().len();
                                        let cur = *mention_selected.peek();
                                        mention_selected.set((cur + 1) % len.max(1));
                                        return;
                                    }
                                    Key::ArrowUp => {
                                        e.prevent_default();
                                        let len = mention_results.peek().len().max(1);
                                        let cur = *mention_selected.peek();
                                        mention_selected.set((cur + len - 1) % len);
                                        return;
                                    }
                                    Key::Escape => {
                                        e.prevent_default();
                                        mention_query.set(None);
                                        mention_results.set(Vec::new());
                                        return;
                                    }
                                    Key::Enter | Key::Tab => {
                                        e.prevent_default();
                                        let idx = *mention_selected.peek();
                                        // Pull the actor out and drop the
                                        // peek guard BEFORE any .set() —
                                        // Dioxus tracks signal borrows
                                        // dynamically and a held read-guard
                                        // during a write panics.
                                        let actor: Option<ActorProfile> = {
                                            let snap = mention_results.peek();
                                            snap.get(idx).cloned()
                                        };
                                        if let Some(actor) = actor {
                                            let new_text = {
                                                let snap = text.peek();
                                                replace_mention_prefix(&snap, &actor.handle)
                                            };
                                            text.set(new_text.clone());
                                            mention_query.set(None);
                                            mention_results.set(Vec::new());
                                            if !crate::demo::is_active() {
                                                tokio::task::spawn_blocking(move || {
                                                    let _ = crate::persistence::save_draft(
                                                        &new_text,
                                                    );
                                                });
                                            }
                                        }
                                        return;
                                    }
                                    _ => {}
                                }
                            }
                            let cmd = e.modifiers().meta() || e.modifiers().ctrl();
                            if cmd && e.key() == Key::Enter {
                                do_submit_kbd();
                                return;
                            }
                            // ⌘V / Ctrl+V: try to attach a clipboard image
                            // before the textarea's native paste handler runs.
                            // We don't prevent_default — if the clipboard has
                            // text, the textarea's native paste still fires.
                            // The image-attach path is no-op when the
                            // clipboard has no image (e.g. plain-text paste).
                            // Solves macOS's screenshot-floater drag, which
                            // hands Wry an unresolvable NSFilePromise.
                            if cmd && e.key().to_string() == "v" {
                                spawn_paste_clipboard_image(attachments);
                            }
                        },
                    }
                    // @mention typeahead popover — anchored beneath the
                    // textarea (same wrap div, position: absolute via CSS).
                    if mention_query.read().is_some() && !mention_results.read().is_empty() {
                        div { class: "compose__mention-popover",
                            for (i, actor) in mention_results.read().iter().enumerate() {
                                {
                                    let actor = actor.clone();
                                    let handle = actor.handle.clone();
                                    let selected = i == *mention_selected.read();
                                    let row_class = if selected {
                                        "compose__mention-row compose__mention-row--selected"
                                    } else {
                                        "compose__mention-row"
                                    };
                                    rsx! {
                                        button {
                                            key: "{actor.did}",
                                            class: "{row_class}",
                                            onmousedown: move |e| {
                                                // mousedown (not click) so we beat the
                                                // textarea's blur-on-click which would
                                                // close the popover before the click
                                                // handler fires.
                                                e.prevent_default();
                                                let new_text = {
                                                    let snap = text.peek();
                                                    replace_mention_prefix(&snap, &handle)
                                                };
                                                text.set(new_text.clone());
                                                mention_query.set(None);
                                                mention_results.set(Vec::new());
                                                if !crate::demo::is_active() {
                                                    tokio::task::spawn_blocking(move || {
                                                        let _ = crate::persistence::save_draft(
                                                            &new_text,
                                                        );
                                                    });
                                                }
                                            },
                                            div { class: "compose__mention-avatar",
                                                if let Some(av) = actor.avatar.as_ref() {
                                                    img { src: "{av}", alt: "{actor.handle}" }
                                                }
                                            }
                                            div { class: "compose__mention-meta",
                                                if let Some(name) = actor
                                                    .display_name
                                                    .as_ref()
                                                    .filter(|s| !s.is_empty())
                                                {
                                                    span { class: "compose__mention-name", "{name}" }
                                                }
                                                span { class: "compose__mention-handle", "@{actor.handle}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if has_attachments {
                    AttachmentGrid { attachments }
                }
                if let Some(v) = video_attachment.read().clone() {
                    div { class: "compose__video-tile",
                        div { class: "compose__video-row",
                            span { class: "compose__video-icon", icons::Play { size: icons::Size::Md } }
                            div { class: "compose__video-meta",
                                span { class: "compose__video-name",
                                    "{v.source_path.file_name().and_then(|s| s.to_str()).unwrap_or(\"video\")}"
                                }
                                span { class: "compose__video-size",
                                    // One decimal so a 1.6 MB clip
                                    // doesn't round to "2 MB" / a 1.2
                                    // doesn't round to "1 MB".
                                    "{(v.bytes.len() as f64 / 1_048_576.0):.1} MB · {v.mime}"
                                }
                            }
                            button { class: "compose__video-remove",
                                title: "Remove video",
                                onclick: move |_| video_attachment.set(None),
                                icons::X { size: icons::Size::Sm }
                            }
                        }
                        // Alt text editor for accessibility.
                        textarea {
                            class: "input compose__video-alt",
                            placeholder: "Describe the video for screen readers (optional)",
                            value: "{v.alt}",
                            oninput: move |e| {
                                if let Some(slot) = video_attachment.write().as_mut() {
                                    slot.alt = truncate_alt(e.value());
                                }
                            },
                        }
                    }
                }
                // Link-card preview. Shows the OpenGraph card we'll
                // attach for the first URL in the post. Hidden once
                // images / a video are attached (they own the media
                // slot, so the card won't be sent). The × dismisses it.
                if attachments_snap.is_empty() && !has_video {
                    if let Some(card) = link_card.read().clone() {
                        div { class: "compose__link-card",
                            if let Some(img) = card.image_url.as_ref() {
                                div { class: "compose__link-card-thumb",
                                    img { loading: "lazy", decoding: "async", src: "{img}", alt: "" }
                                }
                            }
                            div { class: "compose__link-card-meta",
                                span { class: "compose__link-card-title", "{card.title}" }
                                if !card.description.is_empty() {
                                    span { class: "compose__link-card-desc", "{card.description}" }
                                }
                                span { class: "compose__link-card-url", "{card.uri}" }
                            }
                            button { class: "compose__link-card-remove",
                                title: "Remove link preview",
                                onclick: move |_| {
                                    if let Some(c) = link_card.peek().clone() {
                                        link_card_dismissed.write().insert(c.uri);
                                    }
                                    link_card.set(None);
                                },
                                icons::X { size: icons::Size::Sm }
                            }
                        }
                    } else if *link_card_loading.read() {
                        div { class: "compose__link-card compose__link-card--loading",
                            span { class: "compose__thumb-spinner" }
                            span { "Fetching link preview…" }
                        }
                    }
                }
                // Thread extras — only shown when at least one extra
                // has been added. Each is a smaller textarea with a
                // remove (×) button. Plain text only (intentional).
                if !thread_extras.read().is_empty() {
                    div { class: "compose__thread",
                        for (idx, extra) in thread_extras.read().clone().into_iter().enumerate() {
                            div { class: "compose__thread-row",
                                key: "extra-{idx}",
                                span { class: "compose__thread-label", "{idx + 2}/" }
                                textarea {
                                    class: "input compose__thread-text",
                                    placeholder: "Continue the thread…",
                                    value: "{extra}",
                                    oninput: move |e| {
                                        let v = e.value();
                                        if let Some(slot) = thread_extras.write().get_mut(idx) {
                                            *slot = v;
                                        }
                                    },
                                }
                                button { class: "compose__thread-remove",
                                    title: "Remove",
                                    onclick: move |_| {
                                        thread_extras.write().remove(idx);
                                    },
                                    icons::X { size: icons::Size::Sm }
                                }
                            }
                        }
                    }
                }
                div { class: "compose__bar",
                    if reply_to.is_none() && quote_to.is_none() {
                        button { class: "compose__thread-add",
                            title: "Add another post to chain as a self-thread",
                            onclick: move |_| {
                                thread_extras.write().push(String::new());
                            },
                            icons::Plus { size: icons::Size::Sm }
                            " Thread"
                        }
                    }
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
        let new_alt = truncate_alt(evt.value());
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
                                    title: if combined {
                                        "Fill alt text from an AI description of the image PLUS any text the OCR pass detected. For screen-reader accessibility."
                                    } else {
                                        "Fill alt text from an AI description of the image. For screen-reader accessibility."
                                    },
                                    onclick: use_suggestion,
                                    icons::Sparkles { size: icons::Size::Sm }
                                    if combined { "Auto-fill alt (AI + text)" } else { "Auto-fill alt with AI" }
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
                    // maxlength caps user keystrokes — set_alt also
                    // truncate_alt's defensively so an auto-fill or
                    // paste exceeding 2000 chars stays inside the
                    // lexicon limit even if the input ever bypasses
                    // the browser cap.
                    maxlength: "{MAX_ALT_LEN}",
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

/// Spawn the clipboard-paste image attach. Reads the clipboard on a
/// blocking thread, PNG-encodes the raw RGBA, drops it in `$TMPDIR`,
/// then funnels through the same `process_attachment` pipeline drag-drop
/// and the file picker use. Silent no-op when the clipboard holds no
/// image — the textarea's native paste handler still runs for text.
fn spawn_paste_clipboard_image(mut attachments: Signal<Vec<AttachedImage>>) {
    spawn(async move {
        let already = attachments.read().len();
        if MAX_IMAGES.saturating_sub(already) == 0 {
            return;
        }
        let path = match tokio::task::spawn_blocking(read_clipboard_image_to_temp).await {
            Ok(Ok(p)) => p,
            _ => return,
        };
        let llm: Option<Arc<dyn AltTextProvider>> =
            SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
        let att = AttachedImage::new(path.clone());
        let id = att.id;
        attachments.write().push(att);
        process_attachment(attachments, id, path, llm).await;
    });
}

/// Blocking: pull the clipboard image (RGBA8 + dimensions), PNG-encode
/// it, write to a uniquely-named file under the OS temp dir, and return
/// the path. Errors propagate as anyhow so the caller can simply discard
/// any failure (no-clipboard-image being the common case).
fn read_clipboard_image_to_temp() -> anyhow::Result<PathBuf> {
    let mut cb = arboard::Clipboard::new()?;
    let img = cb.get_image()?;
    let rgba =
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
            .ok_or_else(|| anyhow::anyhow!("clipboard image dims/bytes mismatch"))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("smooblue-paste-{nanos}.png"));
    rgba.save_with_format(&path, image::ImageFormat::Png)?;
    Ok(path)
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

#[cfg(test)]
mod mention_prefix_tests {
    use super::{active_mention_prefix, replace_mention_prefix};

    #[test]
    fn no_at_no_prefix() {
        assert_eq!(active_mention_prefix("hello world"), None);
        assert_eq!(active_mention_prefix(""), None);
    }

    #[test]
    fn standalone_at_is_empty_prefix() {
        assert_eq!(active_mention_prefix("@"), Some(""));
        assert_eq!(active_mention_prefix("hello @"), Some(""));
    }

    #[test]
    fn partial_handle_matches() {
        assert_eq!(active_mention_prefix("hey @al"), Some("al"));
        assert_eq!(active_mention_prefix("@alice.bsky"), Some("alice.bsky"));
    }

    #[test]
    fn whitespace_after_at_kills_prefix() {
        // Space after the handle = mention is committed, not active.
        assert_eq!(active_mention_prefix("hey @alice "), None);
    }

    #[test]
    fn at_must_be_at_word_start() {
        // `@` in the middle of a word (e.g. email-style) doesn't fire.
        assert_eq!(active_mention_prefix("foo@alice"), None);
        assert_eq!(active_mention_prefix("email me at foo@bar"), None);
    }

    #[test]
    fn handle_chars_include_dot_underscore_dash() {
        assert_eq!(
            active_mention_prefix("hi @alice.bsky-test_user"),
            Some("alice.bsky-test_user")
        );
    }

    #[test]
    fn replace_appends_handle_and_space() {
        let out = replace_mention_prefix("hey @al", "alice.bsky.social");
        assert_eq!(out, "hey @alice.bsky.social ");
    }

    #[test]
    fn replace_handles_at_start_of_text() {
        let out = replace_mention_prefix("@", "alice.bsky.social");
        assert_eq!(out, "@alice.bsky.social ");
    }

    #[test]
    fn replace_no_active_mention_is_identity() {
        let out = replace_mention_prefix("hey alice", "alice.bsky.social");
        assert_eq!(out, "hey alice");
    }

    #[test]
    fn replace_preserves_text_before_the_mention() {
        let out = replace_mention_prefix("thanks for the heads-up\n\n@a", "alice.bsky.social");
        assert_eq!(out, "thanks for the heads-up\n\n@alice.bsky.social ");
    }
}

#[cfg(test)]
mod mention_ranking_tests {
    use super::rank_mention_results;
    use smooblue_atproto::{ActorProfile, ActorViewerState};

    fn actor(handle: &str, name: Option<&str>, following: bool, followed_by: bool) -> ActorProfile {
        ActorProfile {
            did: format!("did:plc:{handle}"),
            handle: handle.into(),
            display_name: name.map(String::from),
            description: None,
            avatar: None,
            banner: None,
            followers_count: None,
            follows_count: None,
            posts_count: None,
            viewer: Some(ActorViewerState {
                following: following.then(|| "at://follow".into()),
                followed_by: followed_by.then(|| "at://followedby".into()),
                muted: None,
                blocked_by: None,
                ..Default::default()
            }),
            pinned_post: None,
        }
    }

    fn handles(v: &[ActorProfile]) -> Vec<&str> {
        v.iter().map(|a| a.handle.as_str()).collect()
    }

    #[test]
    fn follows_rank_above_strangers_even_with_weaker_match() {
        // Stranger is a clean prefix match; the person you follow only
        // matches mid-handle. Relationship still wins — that's the bias.
        let stranger = actor("alex.bsky.social", Some("Alex"), false, false);
        let you_follow = actor("dralice.bsky.social", Some("Dr Alice"), true, false);
        let ranked = rank_mention_results(vec![stranger, you_follow], "al");
        assert_eq!(
            handles(&ranked),
            vec!["dralice.bsky.social", "alex.bsky.social"]
        );
    }

    #[test]
    fn relationship_tiers_order_mutual_following_followed_stranger() {
        let stranger = actor("s.bsky.social", Some("S"), false, false);
        let follows_you = actor("fy.bsky.social", Some("FY"), false, true);
        let you_follow = actor("yf.bsky.social", Some("YF"), true, false);
        let mutual = actor("mu.bsky.social", Some("Mu"), true, true);
        // Deliberately shuffled input.
        let ranked = rank_mention_results(vec![stranger, follows_you, mutual, you_follow], "");
        assert_eq!(
            handles(&ranked),
            vec![
                "mu.bsky.social",
                "yf.bsky.social",
                "fy.bsky.social",
                "s.bsky.social"
            ]
        );
    }

    #[test]
    fn within_a_tier_handle_prefix_beats_display_name_substring() {
        // Both strangers. One prefix-matches the handle, the other only
        // matches inside the display name.
        let handle_prefix = actor("bobby.bsky.social", Some("Z"), false, false);
        let name_substr = actor("zzz.bsky.social", Some("Mr Bob"), false, false);
        let ranked = rank_mention_results(vec![name_substr, handle_prefix], "bob");
        assert_eq!(
            handles(&ranked),
            vec!["bobby.bsky.social", "zzz.bsky.social"]
        );
    }

    #[test]
    fn equal_scores_preserve_server_order() {
        // Two strangers, identical match quality → server order kept.
        let first = actor("aaa.bsky.social", Some("A"), false, false);
        let second = actor("aab.bsky.social", Some("A"), false, false);
        let ranked = rank_mention_results(vec![first, second], "aa");
        assert_eq!(handles(&ranked), vec!["aaa.bsky.social", "aab.bsky.social"]);
    }
}
