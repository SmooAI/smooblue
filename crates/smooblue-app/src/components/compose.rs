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
//! - **Drafts** — everything (reply / quote target, every post of a
//!   thread, attached media + alt text) autosaves to [`crate::drafts`]
//!   and survives closing the sheet or quitting the app; clears only
//!   on post or discard. Many drafts can exist; the header's "Drafts"
//!   list switches between them.
//! - **Threads** — "+ Add post" chains continuation posts, each with
//!   its own counter, images and "Split into thread". The whole thread
//!   is validated before anything is published, and a mid-thread
//!   failure keeps the unposted remainder as a reply to the last live
//!   post instead of re-posting (duplicating) what already landed.
//! - Bigger textarea + smoo-orange focus ring (in CSS).
//! - **Image attachments** — up to 4 per post. Native file picker,
//!   thumbnail grid, per-image alt-text input. Hooks (in follow-up
//!   pearls) for Apple Vision OCR + Smoo LLM auto-alt seeding.

use crate::alt_text::{merge_descriptions, AltSuggestion, AltTextProvider, SmooLlmAltText};
use crate::auth_refresh::fresh_client;
use crate::drafts::{Draft, DraftMedia, DraftPost, DraftTarget};
use crate::icons;
use crate::image_prep::{prepare_from_path, PreparedImage};
use crate::ocr;
use crate::state::{
    refresh_drafts_index, ComposeContext, DraftsIndex, PostedTick, QuoteTarget, ReplyTarget,
};
use dioxus::prelude::*;
use smooblue_atproto::{
    ActorProfile, AspectRatio, BlobRef, CreatedRecord, FacetKind, LinkCard, PostExternal,
    PostImage, PostVideo, ReplyRef, StrongRef,
};
use smooblue_oauth::Session;
use std::collections::HashSet;
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

/// Process-unique id for attachments and continuation posts. Starts at
/// 1 so it can never collide with [`ROOT_SLOT`].
fn next_id() -> u64 {
    ATTACHMENT_ID.fetch_add(1, Ordering::SeqCst)
}

/// Slot id of the first post in the thread.
pub const ROOT_SLOT: u64 = 0;

/// A continuation post (2nd, 3rd, … in a self-thread). Its images live
/// in the shared attachment list under `slot == id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtraPost {
    pub id: u64,
    pub text: String,
}

impl ExtraPost {
    fn new(text: String) -> Self {
        Self {
            id: next_id(),
            text,
        }
    }
}

/// MIME type for a video file extension we accept, or `None` if the
/// extension isn't a supported video.
fn video_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "mp4" | "m4v" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        _ => None,
    }
}

fn lower_ext(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png" | "webp" | "gif" | "heic")
}

/// Index of every post (0-based, thread order) whose text is over
/// [`MAX_LEN`]. Posting is blocked while this is non-empty, so a
/// too-long post 3 can't fail *after* posts 1 and 2 are already live.
pub fn over_limit_posts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    texts
        .into_iter()
        .enumerate()
        .filter(|(_, t)| t.chars().count() > MAX_LEN)
        .map(|(i, _)| i)
        .collect()
}

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
    /// Which post in the thread this image belongs to: [`ROOT_SLOT`]
    /// for the first post, otherwise the [`ExtraPost::id`] of a
    /// continuation post. One flat list keyed by slot keeps the
    /// background prep / AI pipeline (which finds images by `id`)
    /// unchanged for multi-post threads.
    pub slot: u64,
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
    fn new(path: PathBuf, slot: u64) -> Self {
        Self {
            id: next_id(),
            slot,
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

/// Every signal the composer's draft actions touch, bundled so the
/// save / load / switch / post helpers take one `Copy` value instead of
/// a dozen captured signals.
#[derive(Clone, Copy)]
struct Composer {
    session: Signal<Option<Session>>,
    reply_to: Signal<Option<ReplyTarget>>,
    quote_to: Signal<Option<QuoteTarget>>,
    text: Signal<String>,
    extras: Signal<Vec<ExtraPost>>,
    attachments: Signal<Vec<AttachedImage>>,
    video: Signal<Option<VideoAttachment>>,
    link_card: Signal<Option<LinkCard>>,
    link_card_dismissed: Signal<HashSet<String>>,
    draft_id: Signal<Option<String>>,
    index: Signal<DraftsIndex>,
    error: Signal<Option<String>>,
}

impl Composer {
    fn account_did(&self) -> Option<String> {
        self.session.peek().as_ref().map(|s| s.did.clone())
    }

    fn target(&self) -> DraftTarget {
        DraftTarget::of(self.reply_to.peek().as_ref(), self.quote_to.peek().as_ref())
    }

    /// The composer contents as a draft. `id` is empty until something
    /// has been saved. Failed images are left out (they can't be
    /// restored either).
    fn snapshot(&self) -> Draft {
        let atts = self.attachments.peek();
        let images_for = |slot: u64| -> Vec<DraftMedia> {
            atts.iter()
                .filter(|a| a.slot == slot && !matches!(a.state, AttachmentState::Failed(_)))
                .map(|a| DraftMedia {
                    path: a.source_path.clone(),
                    alt: a.alt.clone(),
                })
                .collect()
        };
        let mut posts = vec![DraftPost {
            text: self.text.peek().clone(),
            images: images_for(ROOT_SLOT),
            video: self.video.peek().as_ref().map(|v| DraftMedia {
                path: v.source_path.clone(),
                alt: v.alt.clone(),
            }),
        }];
        for e in self.extras.peek().iter() {
            posts.push(DraftPost {
                text: e.text.clone(),
                images: images_for(e.id),
                video: None,
            });
        }
        Draft {
            id: self.draft_id.peek().clone().unwrap_or_default(),
            account_did: self.account_did(),
            updated_at: chrono::Utc::now(),
            reply_to: self.reply_to.peek().clone(),
            quote_to: self.quote_to.peek().clone(),
            posts,
        }
    }

    /// Snapshot with an id, allocating (and remembering) one the first
    /// time there's something worth saving. `None` = nothing to save.
    fn snapshot_for_save(mut self) -> Option<Draft> {
        let mut d = self.snapshot();
        if d.id.is_empty() {
            if d.is_empty() {
                return None;
            }
            d.id = crate::drafts::new_id();
            self.draft_id.set(Some(d.id.clone()));
        }
        Some(d)
    }

    /// Save right now on this thread. For the moments that must not
    /// race a later read — closing the sheet, switching drafts. A
    /// single small SQLite upsert.
    fn flush(self) {
        let Some(d) = self.snapshot_for_save() else {
            return;
        };
        if let Err(e) = crate::drafts::save(&d) {
            tracing::warn!(error = %e, "compose: draft save failed");
        }
        refresh_drafts_index(self.index, self.account_did());
    }

    /// Save off the UI thread — the debounced keystroke autosave.
    fn save_in_background(self) {
        let Some(d) = self.snapshot_for_save() else {
            return;
        };
        let index = self.index;
        let account = self.account_did();
        spawn(async move {
            match tokio::task::spawn_blocking(move || crate::drafts::save(&d)).await {
                Ok(Err(e)) => tracing::warn!(error = %e, "compose: draft autosave failed"),
                Err(e) => tracing::warn!(error = %e, "compose: draft autosave panicked"),
                Ok(Ok(())) => {}
            }
            refresh_drafts_index(index, account);
        });
    }

    /// Empty every post (the reply / quote target is kept) and detach
    /// from the saved draft, so the next keystroke starts a new one.
    fn clear_content(mut self) {
        self.text.set(String::new());
        self.extras.set(Vec::new());
        self.attachments.set(Vec::new());
        self.video.set(None);
        self.link_card.set(None);
        self.link_card_dismissed.write().clear();
        self.draft_id.set(None);
        self.error.set(None);
    }

    /// Replace the composer contents with a saved draft. Media is
    /// re-attached from disk; files that have since been moved or
    /// deleted are dropped with a note rather than failing the load.
    fn load(mut self, d: &Draft) {
        self.clear_content();
        self.draft_id.set(Some(d.id.clone()));
        self.reply_to.set(d.reply_to.clone());
        self.quote_to.set(d.quote_to.clone());
        let mut missing = 0usize;
        let mut to_restore: Vec<(u64, DraftMedia)> = Vec::new();
        for (i, p) in d.posts.iter().enumerate() {
            let slot = if i == 0 {
                self.text.set(p.text.clone());
                ROOT_SLOT
            } else {
                let e = ExtraPost::new(p.text.clone());
                let id = e.id;
                self.extras.write().push(e);
                id
            };
            for m in &p.images {
                if m.path.is_file() {
                    to_restore.push((slot, m.clone()));
                } else {
                    missing += 1;
                }
            }
        }
        let llm: Option<Arc<dyn AltTextProvider>> =
            SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
        for (slot, m) in to_restore {
            let mut att = AttachedImage::new(m.path.clone(), slot);
            // A saved alt is the user's (or an accepted suggestion) —
            // don't let a fresh AI pass overwrite it, and don't pay for
            // one at all when there's already a description.
            let describe = m.alt.trim().is_empty();
            att.alt_user_edited = !describe;
            att.alt = m.alt;
            let id = att.id;
            self.attachments.write().push(att);
            let atts = self.attachments;
            let llm = llm.clone();
            spawn(async move {
                process_attachment(atts, id, m.path, llm, describe).await;
            });
        }
        if let Some(v) = d.posts.first().and_then(|p| p.video.clone()) {
            if v.path.is_file() {
                let mut video = self.video;
                let draft_id = self.draft_id;
                let expect = d.id.clone();
                spawn(async move {
                    let path = v.path.clone();
                    let Ok(Ok(Some(att))) =
                        tokio::task::spawn_blocking(move || read_video(&path, v.alt)).await
                    else {
                        return;
                    };
                    // The user may have switched drafts while we read.
                    if draft_id.peek().as_deref() == Some(expect.as_str()) {
                        video.set(Some(att));
                    }
                });
            } else {
                missing += 1;
            }
        }
        if missing > 0 {
            self.error.set(Some(format!(
                "{missing} attachment{} in this draft {} no longer on disk and {} dropped.",
                if missing == 1 { "" } else { "s" },
                if missing == 1 { "is" } else { "are" },
                if missing == 1 { "was" } else { "were" },
            )));
        }
    }

    /// Called when the sheet opens for `reply` / `quote` (both `None`
    /// = a new post). Work in progress for the same target is kept;
    /// otherwise it's saved and the newest draft for the requested
    /// target is resumed — or the composer starts blank.
    fn open_for(mut self, reply: Option<ReplyTarget>, quote: Option<QuoteTarget>) {
        let want = DraftTarget::of(reply.as_ref(), quote.as_ref());
        let current_empty = self.snapshot().is_empty();
        if want == self.target() && !current_empty {
            // Same conversation: keep going. Refresh the target itself
            // (the caller's copy of the parent text is fresher).
            self.reply_to.set(reply);
            self.quote_to.set(quote);
            return;
        }
        let current_id = self.draft_id.peek().clone();
        self.flush();
        let drafts = crate::drafts::list(self.account_did().as_deref()).unwrap_or_default();
        let resume = crate::drafts::pick_resume(&drafts, &want)
            .filter(|d| !(current_empty && current_id.as_deref() == Some(d.id.as_str())))
            .cloned();
        match resume {
            Some(d) => self.load(&d),
            None => self.clear_content(),
        }
        self.reply_to.set(reply);
        self.quote_to.set(quote);
    }

    /// Save the current draft and load another one.
    fn switch_to(mut self, id: &str) {
        if self.draft_id.peek().as_deref() == Some(id) {
            return;
        }
        self.flush();
        let found = self
            .index
            .peek()
            .0
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .or_else(|| crate::drafts::get(id).ok().flatten());
        match found {
            Some(d) => self.load(&d),
            None => self
                .error
                .set(Some("That draft no longer exists.".to_string())),
        }
    }

    /// Save the current draft and start a blank top-level post.
    fn start_new(mut self) {
        self.flush();
        self.clear_content();
        self.reply_to.set(None);
        self.quote_to.set(None);
    }

    /// Throw the current draft away for good.
    fn discard(self) {
        if let Some(id) = self.draft_id.peek().clone() {
            if let Err(e) = crate::drafts::delete(&id) {
                tracing::warn!(error = %e, "compose: draft delete failed");
            }
        }
        self.clear_content();
        refresh_drafts_index(self.index, self.account_did());
    }

    /// Part of a thread went live and a later post failed. Drop what
    /// was published and turn the rest into a reply to the last live
    /// post, so pressing Reply again finishes the thread instead of
    /// re-posting (duplicating) the part that already landed.
    fn keep_unposted(
        mut self,
        posted: &[(u64, CreatedRecord, String)],
        root: StrongRef,
        total: usize,
        err: &str,
    ) {
        let Some((_, last, last_text)) = posted.last() else {
            return;
        };
        let done: HashSet<u64> = posted.iter().map(|(slot, _, _)| *slot).collect();
        let root_text = self.text.peek().clone();
        let remaining: Vec<(u64, String)> = std::iter::once((ROOT_SLOT, root_text))
            .chain(self.extras.peek().iter().map(|e| (e.id, e.text.clone())))
            .filter(|(slot, _)| !done.contains(slot))
            .collect();
        let Some((new_root_slot, new_root_text)) = remaining.first().cloned() else {
            return;
        };
        self.text.set(new_root_text);
        self.extras.set(
            remaining[1..]
                .iter()
                .map(|(id, text)| ExtraPost {
                    id: *id,
                    text: text.clone(),
                })
                .collect(),
        );
        self.attachments.with_mut(|atts| {
            atts.retain(|a| !done.contains(&a.slot));
            for a in atts.iter_mut().filter(|a| a.slot == new_root_slot) {
                a.slot = ROOT_SLOT;
            }
        });
        if done.contains(&ROOT_SLOT) {
            self.video.set(None);
            self.link_card.set(None);
        }
        let handle = self
            .session
            .peek()
            .as_ref()
            .map(|s| s.handle.clone())
            .unwrap_or_default();
        self.quote_to.set(None);
        self.reply_to.set(Some(ReplyTarget {
            uri: last.uri.clone(),
            cid: last.cid.clone(),
            root_uri: root.uri,
            root_cid: root.cid,
            handle,
            text: last_text.clone(),
        }));
        self.error.set(Some(format!(
            "Posted {} of {total}. The rest is saved as a reply to your last post — press Reply to finish the thread. ({err})",
            posted.len(),
        )));
    }
}

/// One post of a thread, resolved and ready to publish.
struct OutPost {
    slot: u64,
    text: String,
    images: Vec<(PreparedImage, String)>,
    video: Option<VideoAttachment>,
    card: Option<LinkCard>,
}

/// Upload one post's media and create the record.
async fn publish_one(
    client: &smooblue_atproto::AtClient,
    post: &OutPost,
    reply: Option<&ReplyRef>,
    quote: Option<&StrongRef>,
) -> Result<CreatedRecord, String> {
    let mut images: Vec<PostImage> = Vec::with_capacity(post.images.len());
    for (prep, alt) in &post.images {
        let blob: BlobRef = client
            .upload_blob(prep.bytes.clone(), &prep.mime)
            .await
            .map_err(|e| format!("image upload failed: {e}"))?;
        images.push(PostImage {
            blob,
            alt: alt.clone(),
            aspect_ratio: Some(AspectRatio {
                width: prep.width,
                height: prep.height,
            }),
        });
    }
    let video = match &post.video {
        Some(v) => Some(PostVideo {
            video: client
                .upload_blob(v.bytes.clone(), &v.mime)
                .await
                .map_err(|e| format!("video upload failed: {e}"))?,
            alt: v.alt.clone(),
            aspect_ratio: None,
        }),
        None => None,
    };
    // Facet detection failing (resolveHandle blip) degrades to a plain
    // text post rather than blocking it.
    let facets = client
        .build_facets_from_text(&post.text)
        .await
        .unwrap_or_default();
    // The thumb upload is best-effort; a card without an image still
    // posts.
    let external = match &post.card {
        Some(card) => {
            let thumb = match card.image_url.as_deref() {
                Some(u) => client.upload_link_card_thumb(u).await.ok(),
                None => None,
            };
            Some(PostExternal {
                uri: card.uri.clone(),
                title: card.title.clone(),
                description: card.description.clone(),
                thumb,
            })
        }
        None => None,
    };
    client
        .create_post_full(
            &post.text,
            reply,
            &images,
            &facets,
            quote,
            video.as_ref(),
            external.as_ref(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Demo-mode stand-in for [`publish_one`]. `SMOOBLUE_DEMO_FAIL_POST=N`
/// fails the Nth post of a submit (1-based), for exercising the
/// partial-thread recovery path without a network.
async fn demo_publish(position: usize) -> Result<CreatedRecord, String> {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let fail_at = std::env::var("SMOOBLUE_DEMO_FAIL_POST")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());
    if fail_at == Some(position + 1) {
        return Err("simulated network failure".into());
    }
    Ok(CreatedRecord {
        uri: format!(
            "at://did:plc:demo/app.bsky.feed.post/demo-{}",
            crate::drafts::new_id()
        ),
        cid: "demo-cid".into(),
    })
}

/// Read a video file for attaching, enforcing [`MAX_VIDEO_BYTES`].
/// Blocking — call from `spawn_blocking`. `Ok(None)` for an
/// unsupported extension.
fn read_video(path: &std::path::Path, alt: String) -> Result<Option<VideoAttachment>, String> {
    let Some(mime) = video_mime(&lower_ext(path)) else {
        return Ok(None);
    };
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_VIDEO_BYTES {
        return Err(format!(
            "Video too large ({:.1} MB). Bluesky caps videos at {} MB.",
            size as f64 / 1_048_576.0,
            MAX_VIDEO_BYTES / 1_048_576,
        ));
    }
    let bytes = std::fs::read(path).map_err(|_| "Couldn't read the video file.".to_string())?;
    Ok(Some(VideoAttachment {
        source_path: path.to_path_buf(),
        bytes,
        mime: mime.to_string(),
        alt,
    }))
}

/// Attach image files to one post (`slot`), up to its [`MAX_IMAGES`]
/// cap, running each through the prep + alt-text pipeline.
fn attach_images(mut attachments: Signal<Vec<AttachedImage>>, slot: u64, paths: Vec<PathBuf>) {
    let already = attachments.peek().iter().filter(|a| a.slot == slot).count();
    let slots = MAX_IMAGES.saturating_sub(already);
    let llm: Option<Arc<dyn AltTextProvider>> =
        SmooLlmAltText::from_env().map(|p| Arc::new(p) as Arc<dyn AltTextProvider>);
    for path in paths.into_iter().take(slots) {
        let att = AttachedImage::new(path.clone(), slot);
        let id = att.id;
        attachments.write().push(att);
        let llm = llm.clone();
        spawn(async move {
            process_attachment(attachments, id, path, llm, true).await;
        });
    }
}

/// "+ Image" picker for one post of the thread.
fn pick_images_into(attachments: Signal<Vec<AttachedImage>>, slot: u64) {
    spawn(async move {
        let already = attachments.peek().iter().filter(|a| a.slot == slot).count();
        if already >= MAX_IMAGES {
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
        attach_images(attachments, slot, files);
    });
}

#[component]
pub fn ComposeSheet() -> Element {
    let session = use_context::<Signal<Option<Session>>>();
    let mut ctx = use_context::<Signal<ComposeContext>>();
    let index = use_context::<Signal<DraftsIndex>>();
    let mut posted_tick = use_context::<Signal<PostedTick>>();
    // The composer owns its target (reply / quote); `ctx` only carries
    // the *request* from whoever opened the sheet. That's what lets a
    // half-written reply survive the user opening "New post" and back.
    let mut reply_to = use_signal(|| None::<ReplyTarget>);
    let mut quote_to = use_signal(|| None::<QuoteTarget>);
    let mut text = use_signal(String::new);
    let attachments = use_signal::<Vec<AttachedImage>>(Vec::new);
    // Single video attachment on the first post (mutually exclusive
    // with images per the lexicon — bsky records carry one media slot).
    let mut video_attachment = use_signal::<Option<VideoAttachment>>(|| None);
    let mut posting = use_signal(|| false);
    // (current post, total) while a thread is publishing.
    let mut progress = use_signal(|| None::<(usize, usize)>);
    let mut error = use_signal(|| None::<String>);
    // Continuation posts (2nd, 3rd, …) of a self-thread. Each carries
    // its own text and — via `AttachedImage::slot` — its own images.
    let mut thread_extras = use_signal::<Vec<ExtraPost>>(Vec::new);
    // Id of the saved draft the composer is editing (None until the
    // first non-empty autosave).
    let draft_id = use_signal(|| None::<String>);
    let mut show_drafts = use_signal(|| false);
    let mut confirm_discard = use_signal(|| false);
    // A continuation post that should grab focus when it mounts (the
    // one just added by "+ Add post" or a split).
    let mut focus_extra = use_signal(|| None::<u64>);

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
    let mut link_card_dismissed = use_signal::<HashSet<String>>(Default::default);
    let mut link_card_seq = use_signal::<u64>(|| 0);

    let composer = Composer {
        session,
        reply_to,
        quote_to,
        text,
        extras: thread_extras,
        attachments,
        video: video_attachment,
        link_card,
        link_card_dismissed,
        draft_id,
        index,
        error,
    };

    // Open / close transitions. On open, resolve what the sheet should
    // show (keep the work in progress, resume a saved draft for this
    // target, or start blank) and consume the one-shot `resume_draft`
    // / `prefill` requests. On close, save immediately — the app may
    // be quit right after, before a debounced autosave would fire.
    let mut was_open = use_signal(|| false);
    use_effect(move || {
        let c = ctx.read().clone();
        let prev = *was_open.peek();
        if c.open != prev {
            was_open.set(c.open);
        }
        if !c.open {
            if prev {
                composer.flush();
                show_drafts.set(false);
            }
            return;
        }
        let resume = c.resume_draft.clone();
        let prefill = c.prefill.clone().filter(|p| !p.is_empty());
        if prev && resume.is_none() && c.prefill.is_none() {
            return;
        }
        if c.resume_draft.is_some() || c.prefill.is_some() {
            let mut w = ctx.write();
            w.resume_draft = None;
            w.prefill = None;
        }
        if let Some(id) = resume {
            composer.switch_to(&id);
        } else if !prev {
            composer.open_for(c.reply_to.clone(), c.quote_to.clone());
        }
        if let Some(p) = prefill {
            text.set(p);
        }
    });

    // Debounced autosave: any change to what a draft is made of saves
    // it 400ms after the last edit, off the UI thread.
    let mut autosave_seq = use_signal(|| 0u64);
    use_effect(move || {
        let _ = text.read();
        let _ = thread_extras.read();
        let _ = attachments.read();
        let _ = video_attachment.read();
        let _ = reply_to.read();
        let _ = quote_to.read();
        let seq = {
            let mut s = autosave_seq.write();
            *s = s.wrapping_add(1);
            *s
        };
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            if *autosave_seq.peek() != seq || *posting.peek() {
                return;
            }
            composer.save_in_background();
        });
    });

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
    // URL; when it changes (and isn't dismissed) we fetch the card after
    // a short pause. Failures are silent — a post with a bare link is
    // still fine, it just won't get a card.
    //
    // Once a card is attached it STAYS, even if the URL is later erased
    // from the text — people routinely paste a link to get the embed,
    // then delete the raw URL so only the card/quote shows. The card is
    // removed only by the explicit dismiss (✕), send, or reset — never
    // by the link leaving the text.
    use_effect(move || {
        let url = first_link_url(&text.read());
        // No (non-dismissed) URL in the text: keep whatever card is
        // already attached; there's nothing new to fetch.
        let Some(url) = url.filter(|u| !link_card_dismissed.read().contains(u)) else {
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
            let path = PathBuf::from(p);
            if path.is_file() {
                attach_images(attachments, ROOT_SLOT, vec![path]);
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
        let mut ctx_open = ctx;
        // Open compose for either: drag-enter (user sees highlight as
        // they hover) or pending-drop (user sees the attached image).
        if !ctx_open.peek().open {
            ctx_open.write().open_new();
        }
        if !has_pending {
            return;
        }
        let drained: Vec<PathBuf> = pending_drops.write().drain(..).collect();
        if video_attachment.peek().is_some() {
            error.set(Some(
                "A post can carry a video or images, not both — remove the video to attach images."
                    .into(),
            ));
            return;
        }
        let already = attachments
            .peek()
            .iter()
            .filter(|a| a.slot == ROOT_SLOT)
            .count();
        if already >= MAX_IMAGES {
            error.set(Some(format!(
                "Already at {MAX_IMAGES} images — dropped screenshot ignored."
            )));
            return;
        }
        let files: Vec<PathBuf> = drained.into_iter().filter(|p| p.is_file()).collect();
        attach_images(attachments, ROOT_SLOT, files);
    });

    let snap = ctx.read().clone();
    if !snap.open {
        return rsx! { Fragment {} };
    }

    let reply_snap = reply_to.read().clone();
    let quote_snap = quote_to.read().clone();
    let extras_snap = thread_extras.read().clone();
    let post_count = 1 + extras_snap.len();

    let len = text.read().chars().count();
    let remaining = MAX_LEN as i64 - len as i64;
    let over = remaining < 0;
    let over_by = -remaining;
    let attachments_snap = attachments.read().clone();
    let root_attachments = attachments_snap
        .iter()
        .filter(|a| a.slot == ROOT_SLOT)
        .count();
    let has_attachments = !attachments_snap.is_empty();
    let any_preparing = attachments_snap
        .iter()
        .any(|a| matches!(a.state, AttachmentState::Preparing));
    let any_failed = attachments_snap
        .iter()
        .any(|a| matches!(a.state, AttachmentState::Failed(_)));
    let has_video = video_attachment.read().is_some();
    let has_card = link_card.read().is_some() && root_attachments == 0 && !has_video;
    let over_posts = over_limit_posts(
        std::iter::once(text.read().as_str())
            .chain(extras_snap.iter().map(|e| e.text.as_str()))
            .collect::<Vec<_>>(),
    );
    // Empty only if no post has text or media. Image-only / video-only
    // / card-only posts are valid on bsky.
    let empty = text.read().trim().is_empty()
        && extras_snap.iter().all(|e| e.text.trim().is_empty())
        && !has_attachments
        && !has_video
        && !has_card;
    let at_image_cap = root_attachments >= MAX_IMAGES;

    // Submit flow (shared by the button and ⌘↵ in any post). Posts
    // the whole thread in order: post N replies to post N-1, and all
    // share one root (the replied-to thread's root when this is a
    // reply, else the first post). Blank continuation posts are
    // skipped. Everything is validated up front so a too-long post 3
    // can't fail after 1 and 2 are live.
    let do_submit = move || {
        if *posting.peek() {
            return;
        }
        let atts_now = attachments.peek().clone();
        if atts_now.iter().any(|a| {
            matches!(
                a.state,
                AttachmentState::Preparing | AttachmentState::Failed(_)
            )
        }) {
            return;
        }
        let root_text = text.peek().clone();
        let extras_now = thread_extras.peek().clone();
        let over_any = !over_limit_posts(
            std::iter::once(root_text.as_str())
                .chain(extras_now.iter().map(|e| e.text.as_str()))
                .collect::<Vec<_>>(),
        )
        .is_empty();
        if over_any {
            return;
        }
        let video_now = video_attachment.peek().clone();
        let card_now = link_card.peek().clone();
        let mut plan: Vec<OutPost> = Vec::new();
        let slots = std::iter::once((ROOT_SLOT, root_text))
            .chain(extras_now.into_iter().map(|e| (e.id, e.text)));
        for (slot, body) in slots {
            let images: Vec<(PreparedImage, String)> = atts_now
                .iter()
                .filter(|a| a.slot == slot)
                .filter_map(|a| match &a.state {
                    AttachmentState::Ready(p) => Some((p.clone(), a.alt.clone())),
                    _ => None,
                })
                .collect();
            let video = if slot == ROOT_SLOT {
                video_now.clone()
            } else {
                None
            };
            // The link card rides on the first post, and only when no
            // image / video owns its single media slot.
            let card = if slot == ROOT_SLOT && images.is_empty() && video.is_none() {
                card_now.clone()
            } else {
                None
            };
            if body.trim().is_empty() && images.is_empty() && video.is_none() && card.is_none() {
                continue;
            }
            plan.push(OutPost {
                slot,
                text: body,
                images,
                video,
                card,
            });
        }
        if plan.is_empty() {
            return;
        }
        let reply = reply_to.peek().clone();
        let quote = quote_to.peek().clone();
        posting.set(true);
        error.set(None);
        spawn(async move {
            let total = plan.len();
            let client = if crate::demo::is_active() {
                None
            } else {
                match fresh_client(session).await {
                    Some(c) => Some(c),
                    None => {
                        posting.set(false);
                        error.set(Some("Session expired — please sign in again.".into()));
                        return;
                    }
                }
            };
            // root = the thread root carried on the ReplyTarget (the
            // ancestor root for a deep reply). Setting root = parent
            // orphaned deep replies — see th-f603e2.
            let mut root_ref = reply.as_ref().map(|r| StrongRef {
                uri: r.root_uri.clone(),
                cid: r.root_cid.clone(),
            });
            let mut parent_ref = reply.as_ref().map(|r| StrongRef {
                uri: r.uri.clone(),
                cid: r.cid.clone(),
            });
            let mut posted: Vec<(u64, CreatedRecord, String)> = Vec::new();
            let mut failure: Option<String> = None;
            for (i, post) in plan.iter().enumerate() {
                progress.set(Some((i + 1, total)));
                let reply_ref = match (&root_ref, &parent_ref) {
                    (Some(root), Some(parent)) => Some(ReplyRef {
                        root: root.clone(),
                        parent: parent.clone(),
                    }),
                    _ => None,
                };
                let quote_ref = if i == 0 {
                    quote.as_ref().map(|q| StrongRef {
                        uri: q.uri.clone(),
                        cid: q.cid.clone(),
                    })
                } else {
                    None
                };
                let result = match &client {
                    Some(c) => publish_one(c, post, reply_ref.as_ref(), quote_ref.as_ref()).await,
                    None => demo_publish(i).await,
                };
                match result {
                    Ok(rec) => {
                        let this = StrongRef {
                            uri: rec.uri.clone(),
                            cid: rec.cid.clone(),
                        };
                        if root_ref.is_none() {
                            root_ref = Some(this.clone());
                        }
                        parent_ref = Some(this);
                        posted.push((post.slot, rec, post.text.clone()));
                    }
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            posting.set(false);
            progress.set(None);
            if !posted.is_empty() {
                posted_tick.with_mut(|t| t.0 = t.0.wrapping_add(1));
            }
            match (failure, root_ref) {
                (None, _) => {
                    // All live — nothing left to recover.
                    composer.discard();
                    reply_to.set(None);
                    quote_to.set(None);
                    ctx.write().open = false;
                }
                (Some(e), _) if posted.is_empty() => {
                    error.set(Some(format!("Couldn't post: {e}")));
                }
                (Some(e), Some(root)) => composer.keep_unposted(&posted, root, total, &e),
                (Some(e), None) => error.set(Some(format!("Couldn't post: {e}"))),
            }
        });
    };

    let mut do_submit_btn = do_submit;
    let mut do_submit_kbd = do_submit;

    // Split an over-long post into as many posts as it needs, right
    // after itself in the thread.
    let split_post = move |slot: u64| {
        let body = if slot == ROOT_SLOT {
            text.peek().clone()
        } else {
            match thread_extras.peek().iter().find(|e| e.id == slot) {
                Some(e) => e.text.clone(),
                None => return,
            }
        };
        let mut chunks = crate::drafts::split_for_thread(&body, MAX_LEN).into_iter();
        let Some(first) = chunks.next() else {
            return;
        };
        let rest: Vec<ExtraPost> = chunks.map(ExtraPost::new).collect();
        if rest.is_empty() {
            return;
        }
        let insert_at = if slot == ROOT_SLOT {
            text.set(first);
            0
        } else {
            let mut list = thread_extras.write();
            let Some(pos) = list.iter().position(|e| e.id == slot) else {
                return;
            };
            list[pos].text = first;
            pos + 1
        };
        let mut list = thread_extras.write();
        for (offset, post) in rest.into_iter().enumerate() {
            list.insert(insert_at + offset, post);
        }
    };

    let close = move |_evt| {
        ctx.write().open = false;
    };

    let add_post = move |_| {
        let post = ExtraPost::new(String::new());
        focus_extra.set(Some(post.id));
        thread_extras.write().push(post);
    };

    let placeholder = if reply_snap.is_some() {
        "Write your reply…"
    } else {
        "What's up?"
    };
    let base_title = if reply_snap.is_some() {
        "Reply"
    } else if quote_snap.is_some() {
        "Quote post"
    } else {
        "New post"
    };
    let title_text = if post_count > 1 {
        format!("{base_title} · thread of {post_count}")
    } else {
        base_title.to_string()
    };
    let button_text = match (reply_snap.is_some(), post_count > 1) {
        (true, false) => "Reply",
        (true, true) => "Reply with thread",
        (false, false) => "Post",
        (false, true) => "Post thread",
    };

    let textarea_class = if over {
        "input input--lg compose__textarea compose__textarea--over"
    } else {
        "input input--lg compose__textarea"
    };

    let post_disabled =
        empty || !over_posts.is_empty() || any_preparing || any_failed || *posting.read();

    // Drafts other than the one on screen — what the "Drafts" button
    // offers to switch to.
    let current_id = draft_id.read().clone();
    let other_drafts = index
        .read()
        .0
        .iter()
        .filter(|d| Some(&d.id) != current_id.as_ref())
        .count();
    let saved = current_id.is_some() && !empty;

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
        spawn(async move {
            let mut images = Vec::new();
            for name in names {
                // file_engine.files() returns paths on desktop;
                // skip anything that isn't a readable file.
                let path = PathBuf::from(&name);
                if !path.is_file() {
                    continue;
                }
                let ext = lower_ext(&path);
                // Video: replaces any prior video attachment (only one
                // video per post per the lexicon). Size-gated BEFORE
                // reading so a 4 GB drop can't OOM the renderer, and
                // read off the renderer thread.
                if video_mime(&ext).is_some() {
                    if attachments.peek().iter().any(|a| a.slot == ROOT_SLOT) {
                        error.set(Some(
                            "A post can carry a video or images, not both — remove the images to attach a video."
                                .into(),
                        ));
                        break;
                    }
                    let path_for_read = path.clone();
                    match tokio::task::spawn_blocking(move || {
                        read_video(&path_for_read, String::new())
                    })
                    .await
                    {
                        Ok(Ok(Some(v))) => video_attachment.set(Some(v)),
                        Ok(Err(msg)) => error.set(Some(msg)),
                        _ => error.set(Some("Couldn't read the dropped video file.".into())),
                    }
                    // One video per post — don't mix media types.
                    break;
                }
                if is_image_ext(&ext) {
                    images.push(path);
                }
            }
            if images.is_empty() {
                return;
            }
            if video_attachment.peek().is_some() {
                error.set(Some(
                    "A post can carry a video or images, not both — remove the video to attach images."
                        .into(),
                ));
                return;
            }
            attach_images(attachments, ROOT_SLOT, images);
        });
    };

    let index_snap = index.read().0.clone();
    let attach_title = if has_video {
        "Remove the video to attach images"
    } else if at_image_cap {
        "Image limit reached (4 max)"
    } else {
        "Attach image"
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
                    if saved {
                        span { class: "compose__saved",
                            title: "Everything you type is kept as a draft until it's posted or discarded.",
                            icons::Check { size: icons::Size::Sm }
                            "Draft saved"
                        }
                    }
                    if other_drafts > 0 || *show_drafts.read() {
                        button {
                            class: if *show_drafts.read() { "compose__head-btn compose__head-btn--active" } else { "compose__head-btn" },
                            title: "Your saved drafts",
                            onclick: move |_| {
                                let open = *show_drafts.peek();
                                show_drafts.set(!open);
                            },
                            "Drafts"
                            if other_drafts > 0 {
                                span { class: "compose__head-count", "{other_drafts}" }
                            }
                        }
                    }
                    if !empty {
                        button { class: "compose__head-btn",
                            title: "Start a new post — this one stays in Drafts",
                            onclick: move |_| {
                                composer.start_new();
                                show_drafts.set(false);
                            },
                            icons::Plus { size: icons::Size::Sm }
                            "New"
                        }
                    }
                    button { class: "compose__close",
                        title: "Close (Esc) — your draft is saved",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                if *show_drafts.read() {
                    DraftsList {
                        drafts: index_snap,
                        current: current_id.clone(),
                        on_open: move |id: String| {
                            composer.switch_to(&id);
                            show_drafts.set(false);
                        },
                        on_delete: move |id: String| {
                            if draft_id.peek().as_deref() == Some(id.as_str()) {
                                composer.discard();
                            } else {
                                if let Err(e) = crate::drafts::delete(&id) {
                                    tracing::warn!(error = %e, "compose: draft delete failed");
                                }
                                refresh_drafts_index(index, composer.account_did());
                            }
                        },
                    }
                } else {
                if let Some(parent) = reply_snap.as_ref() {
                    div { class: "compose__reply-context",
                        div { class: "compose__reply-author",
                            "Replying to "
                            span { class: "compose__reply-handle", "@{parent.handle}" }
                        }
                        if !parent.text.is_empty() {
                            p { class: "compose__reply-text", "{parent.text}" }
                        }
                    }
                }
                if let Some(q) = quote_snap.as_ref() {
                    div { class: "compose__quote-context",
                        div { class: "compose__reply-author",
                            "Quoting "
                            span { class: "compose__reply-handle", "@{q.handle}" }
                        }
                        p { class: "compose__reply-text", "{q.text}" }
                    }
                }
                div { class: "compose__textarea-wrap",
                    if post_count > 1 {
                        span { class: "compose__thread-label compose__thread-label--root", "1/{post_count}" }
                    }
                    textarea {
                        class: "{textarea_class}",
                        placeholder: "{placeholder}",
                        autofocus: true,
                        // `autofocus` alone is unreliable in the webview when
                        // the compose box mounts on click (it only fires on
                        // first page load), so focus explicitly on mount.
                        onmounted: move |evt: Event<MountedData>| {
                            spawn(async move {
                                let _ = evt.data().set_focus(true).await;
                            });
                        },
                        value: "{text}",
                        oninput: move |e| {
                            let v = e.value();
                            // Drive the @mention popover off the same
                            // event so we don't need a second listener.
                            mention_query.set(active_mention_prefix(&v).map(String::from));
                            // The autosave effect picks the change up.
                            text.set(v);
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
                                        // Close only the popover, not the
                                        // whole sheet.
                                        e.stop_propagation();
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
                                            text.set(new_text);
                                            mention_query.set(None);
                                            mention_results.set(Vec::new());
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
                                spawn_paste_clipboard_image(attachments, ROOT_SLOT);
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
                                                text.set(new_text);
                                                mention_query.set(None);
                                                mention_results.set(Vec::new());
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
                if over {
                    div { class: "compose__split-row",
                        span { "This post is {over_by} characters over." }
                        button { class: "compose__split",
                            title: "Break this post into a thread at sentence and word boundaries",
                            onclick: move |_| {
                                let mut split = split_post;
                                split(ROOT_SLOT);
                            },
                            "Split into thread"
                        }
                    }
                }
                if root_attachments > 0 {
                    AttachmentGrid { attachments, slot: ROOT_SLOT }
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
                if root_attachments == 0 && !has_video {
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
                // Continuation posts of the thread. Each is a full post:
                // its own counter, split, images, and paste target.
                if !extras_snap.is_empty() {
                    div { class: "compose__thread",
                        for (idx, extra) in extras_snap.iter().cloned().enumerate() {
                            ExtraPostEditor {
                                key: "{extra.id}",
                                position: idx + 2,
                                total: post_count,
                                post: extra.clone(),
                                extras: thread_extras,
                                attachments,
                                focus_extra,
                                on_submit: move |_: ()| {
                                    let mut submit = do_submit;
                                    submit();
                                },
                                on_split: move |slot: u64| {
                                    let mut split = split_post;
                                    split(slot);
                                },
                            }
                        }
                    }
                }
                div { class: "compose__bar",
                    button { class: "compose__thread-add",
                        title: "Add another post to this thread",
                        onclick: add_post,
                        icons::Plus { size: icons::Size::Sm }
                        " Add post"
                    }
                    button {
                        class: if at_image_cap || has_video { "compose__attach compose__attach--disabled" } else { "compose__attach" },
                        title: "{attach_title}",
                        disabled: at_image_cap || has_video,
                        onclick: move |_| pick_images_into(attachments, ROOT_SLOT),
                        icons::ImageIcon { size: icons::Size::Sm }
                    }
                    if !empty {
                        button {
                            class: if *confirm_discard.read() { "compose__discard compose__discard--confirm" } else { "compose__discard" },
                            title: "Discard this draft",
                            onclick: move |_| {
                                if *confirm_discard.peek() {
                                    confirm_discard.set(false);
                                    composer.discard();
                                } else {
                                    confirm_discard.set(true);
                                    spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                        confirm_discard.set(false);
                                    });
                                }
                            },
                            icons::Trash2 { size: icons::Size::Sm }
                            if *confirm_discard.read() { " Discard?" }
                        }
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
                            match *progress.read() {
                                Some((i, n)) if n > 1 => rsx! { "Posting {i}/{n}…" },
                                _ if has_attachments || has_video => rsx! { "Uploading…" },
                                _ => rsx! { "Posting…" },
                            }
                        } else {
                            "{button_text}"
                        }
                    }
                }
                if over_posts.iter().any(|&i| i > 0) {
                    div { class: "compose__error",
                        {
                            let which: Vec<String> = over_posts
                                .iter()
                                .filter(|&&i| i > 0)
                                .map(|i| (i + 1).to_string())
                                .collect();
                            let noun = if which.len() == 1 { "Post" } else { "Posts" };
                            format!("{noun} {} over {MAX_LEN} characters — shorten or split before posting.", which.join(", "))
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
}

/// One continuation post (2nd, 3rd, …) in the thread composer.
#[component]
fn ExtraPostEditor(
    position: usize,
    total: usize,
    post: ExtraPost,
    extras: Signal<Vec<ExtraPost>>,
    attachments: Signal<Vec<AttachedImage>>,
    focus_extra: Signal<Option<u64>>,
    on_submit: EventHandler<()>,
    on_split: EventHandler<u64>,
) -> Element {
    let id = post.id;
    let len = post.text.chars().count();
    let remaining = MAX_LEN as i64 - len as i64;
    let over = remaining < 0;
    let over_by = -remaining;
    let image_count = attachments.read().iter().filter(|a| a.slot == id).count();
    let mut extras_w = extras;
    let mut attachments_w = attachments;
    let mut focus = focus_extra;
    rsx! {
        div { class: "compose__thread-post",
            div { class: "compose__thread-row",
                span { class: "compose__thread-label", "{position}/{total}" }
                textarea {
                    class: if over { "input compose__thread-text compose__textarea--over" } else { "input compose__thread-text" },
                    placeholder: "Continue the thread…",
                    value: "{post.text}",
                    onmounted: move |evt: Event<MountedData>| {
                        if *focus.peek() == Some(id) {
                            focus.set(None);
                            spawn(async move {
                                let _ = evt.data().set_focus(true).await;
                            });
                        }
                    },
                    oninput: move |e| {
                        let v = e.value();
                        if let Some(slot) = extras_w.write().iter_mut().find(|p| p.id == id) {
                            slot.text = v;
                        }
                    },
                    onkeydown: move |e| {
                        let cmd = e.modifiers().meta() || e.modifiers().ctrl();
                        if cmd && e.key() == Key::Enter {
                            on_submit.call(());
                            return;
                        }
                        if cmd && e.key().to_string() == "v" {
                            spawn_paste_clipboard_image(attachments, id);
                        }
                    },
                }
                div { class: "compose__thread-tools",
                    span {
                        class: if over { "compose__thread-count compose__counter--over" } else { "compose__thread-count" },
                        "{remaining}"
                    }
                    button {
                        class: if image_count >= MAX_IMAGES { "compose__attach compose__attach--disabled" } else { "compose__attach" },
                        title: if image_count >= MAX_IMAGES { "Image limit reached (4 max)" } else { "Attach image to this post" },
                        disabled: image_count >= MAX_IMAGES,
                        onclick: move |_| pick_images_into(attachments, id),
                        icons::ImageIcon { size: icons::Size::Sm }
                    }
                    button { class: "compose__thread-remove",
                        title: "Remove this post from the thread",
                        onclick: move |_| {
                            extras_w.write().retain(|p| p.id != id);
                            attachments_w.write().retain(|a| a.slot != id);
                        },
                        icons::X { size: icons::Size::Sm }
                    }
                }
            }
            if over {
                div { class: "compose__split-row",
                    span { "{over_by} characters over." }
                    button { class: "compose__split",
                        onclick: move |_| on_split.call(id),
                        "Split into thread"
                    }
                }
            }
            if image_count > 0 {
                AttachmentGrid { attachments, slot: id }
            }
        }
    }
}

/// The compose sheet's saved-drafts list.
#[component]
fn DraftsList(
    drafts: Vec<Draft>,
    current: Option<String>,
    on_open: EventHandler<String>,
    on_delete: EventHandler<String>,
) -> Element {
    // Two-step delete: first click arms the row, second deletes.
    let mut armed = use_signal(|| None::<String>);
    if drafts.is_empty() {
        return rsx! {
            div { class: "compose__drafts-empty",
                "No saved drafts. Anything you start writing is kept here until you post or discard it."
            }
        };
    }
    rsx! {
        div { class: "compose__drafts",
            for d in drafts {
                {
                    let id_open = d.id.clone();
                    let id_delete = d.id.clone();
                    let is_current = current.as_deref() == Some(d.id.as_str());
                    let is_armed = armed.read().as_deref() == Some(d.id.as_str());
                    let preview = d.preview(140);
                    let label = d.label();
                    let ts = d.updated_at.to_rfc3339();
                    rsx! {
                        div { key: "{d.id}",
                            class: if is_current { "compose__draft compose__draft--current" } else { "compose__draft" },
                            button { class: "compose__draft-body",
                                title: "Open this draft",
                                onclick: move |_| on_open.call(id_open.clone()),
                                div { class: "compose__draft-meta",
                                    span { class: "compose__draft-label", "{label}" }
                                    if is_current {
                                        span { class: "compose__draft-badge", "editing" }
                                    }
                                    span { class: "compose__draft-time",
                                        icons::TimeAgo { text_at_render: String::new(), source_ts: Some(ts) }
                                    }
                                }
                                span { class: "compose__draft-preview", "{preview}" }
                            }
                            button {
                                class: if is_armed { "compose__draft-delete compose__draft-delete--armed" } else { "compose__draft-delete" },
                                title: if is_armed { "Click again to delete" } else { "Delete draft" },
                                onclick: move |_| {
                                    if is_armed {
                                        armed.set(None);
                                        on_delete.call(id_delete.clone());
                                    } else {
                                        armed.set(Some(id_delete.clone()));
                                    }
                                },
                                icons::Trash2 { size: icons::Size::Sm }
                                if is_armed { " Delete?" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Thumbnail grid for attached images. Each tile has a preview, an
/// alt-text textarea, and a small "X" to remove.
/// Only the images of one post (`slot`) in the thread are shown.
#[component]
fn AttachmentGrid(attachments: Signal<Vec<AttachedImage>>, slot: u64) -> Element {
    let snapshot: Vec<AttachedImage> = attachments
        .read()
        .iter()
        .filter(|a| a.slot == slot)
        .cloned()
        .collect();
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

/// Spawn the clipboard-paste image attach. Reads the clipboard on a
/// blocking thread, PNG-encodes the raw RGBA, drops it in `$TMPDIR`,
/// then funnels through the same `process_attachment` pipeline drag-drop
/// and the file picker use. Silent no-op when the clipboard holds no
/// image — the textarea's native paste handler still runs for text.
fn spawn_paste_clipboard_image(attachments: Signal<Vec<AttachedImage>>, slot: u64) {
    spawn(async move {
        let already = attachments.peek().iter().filter(|a| a.slot == slot).count();
        if already >= MAX_IMAGES {
            return;
        }
        // Read the clipboard on the MAIN thread. macOS NSPasteboard is
        // not thread-safe — touching it from a tokio worker (as a bare
        // spawn_blocking(read+encode) did) races the main thread's own
        // pasteboard access and traps in __NSFastEnumerationMutationHandler.
        // Dioxus polls spawned futures on the event-loop (main) thread, so
        // the arboard read here is on-main; only the PNG-encode + file
        // write, which is the actually-heavy part, goes to a worker.
        let rgba = match read_clipboard_image() {
            Ok(img) => img,
            _ => return,
        };
        let path = match tokio::task::spawn_blocking(move || encode_rgba_to_temp(rgba)).await {
            Ok(Ok(p)) => p,
            _ => return,
        };
        attach_images(attachments, slot, vec![path]);
    });
}

/// Pull the clipboard image as raw RGBA8. **Must run on the main thread**
/// — macOS NSPasteboard is main-thread-only and traps if read off-main.
/// Errors propagate as anyhow so the caller can discard any failure
/// (no-clipboard-image being the common case).
fn read_clipboard_image() -> anyhow::Result<image::RgbaImage> {
    let mut cb = arboard::Clipboard::new()?;
    let img = cb.get_image()?;
    image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
        .ok_or_else(|| anyhow::anyhow!("clipboard image dims/bytes mismatch"))
}

/// Blocking: PNG-encode the RGBA and write it to a uniquely-named file,
/// returning the path. Safe off the main thread — no pasteboard access
/// here. Pasted images go under the app's data dir (not `$TMPDIR`,
/// which macOS sweeps) because a draft holding one may be resumed days
/// later.
fn encode_rgba_to_temp(rgba: image::RgbaImage) -> anyhow::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")
        .map(|d| d.data_dir().join("pasted"))
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("smooblue-paste-{nanos}.png"));
    rgba.save_with_format(&path, image::ImageFormat::Png)?;
    Ok(path)
}

/// Single shared pipeline for a freshly-added attachment: prep image,
/// then in parallel run LLM describe + Apple Vision OCR. As each
/// finishes, write the result into the slot AND recompute the merged
/// alt text (unless the user has already typed). Idempotent if either
/// task fails — we just leave the slot's field empty.
///
/// `describe = false` skips the LLM + OCR pass entirely — used when
/// restoring a draft image that already has alt text.
async fn process_attachment(
    attachments: Signal<Vec<AttachedImage>>,
    id: u64,
    path: PathBuf,
    llm: Option<Arc<dyn AltTextProvider>>,
    describe: bool,
) {
    let llm = if describe { llm } else { None };
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
    let cfg_ocr = describe && cfg!(target_os = "macos");
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
    if !describe {
        return;
    }

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
