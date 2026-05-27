//! Bluesky feed types — subset needed to render a deck column.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct FeedResponse {
    #[serde(default)]
    pub feed: Vec<FeedItem>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FeedItem {
    pub post: PostView,
    /// Raw `replyRef` blob from `app.bsky.feed.defs#feedViewPost`.
    /// Kept as a Value (not a typed struct) so a slightly-off shape
    /// from the AppView never blows up feed decode for the whole
    /// page. Pull a parent handle out via [`Self::reply_parent_handle`].
    #[serde(default)]
    pub reply: Option<serde_json::Value>,
    /// Raw `reason` blob (reasonRepost / reasonPin / unknown future
    /// variants). Same defensive read pattern as `reply`. Use
    /// [`Self::reposter_display`] to pull out the reposter name.
    #[serde(default)]
    pub reason: Option<serde_json::Value>,
}

impl FeedItem {
    /// Display name of the reposter when this row is surfaced via
    /// `reasonRepost`, otherwise None. Falls back to the handle if
    /// no displayName is set on the actor.
    pub fn reposter_display(&self) -> Option<String> {
        let r = self.reason.as_ref()?;
        let ty = r.get("$type").and_then(|v| v.as_str())?;
        if ty != "app.bsky.feed.defs#reasonRepost" {
            return None;
        }
        let by = r.get("by")?;
        let display = by.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
        let handle = by.get("handle").and_then(|v| v.as_str()).unwrap_or("");
        if !display.is_empty() {
            Some(display.to_string())
        } else if !handle.is_empty() {
            Some(handle.to_string())
        } else {
            None
        }
    }

    /// DID of the reposter — used as a key-disambiguator so two
    /// reposts of the same post in the same page don't collide.
    pub fn reposter_did(&self) -> Option<String> {
        let r = self.reason.as_ref()?;
        let ty = r.get("$type").and_then(|v| v.as_str())?;
        if ty != "app.bsky.feed.defs#reasonRepost" {
            return None;
        }
        r.get("by")
            .and_then(|by| by.get("did"))
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    /// Handle of the post being replied to (parent), or None when
    /// the row isn't a reply or the parent is unavailable
    /// (notFound/blocked).
    pub fn reply_parent_handle(&self) -> Option<String> {
        let parent = self.reply.as_ref()?.get("parent")?;
        // Skip notFoundPost / blockedPost shapes by checking $type.
        let ty = parent.get("$type").and_then(|v| v.as_str()).unwrap_or("");
        if ty.ends_with("#notFoundPost") || ty.ends_with("#blockedPost") {
            return None;
        }
        parent
            .get("author")
            .and_then(|a| a.get("handle"))
            .and_then(|v| v.as_str())
            .map(String::from)
    }
}

/// `app.bsky.actor.defs#profileViewDetailed` — full profile shape.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ActorProfile {
    pub did: String,
    pub handle: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    /// Wide banner image shown behind the avatar in profile views.
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(rename = "followersCount", default)]
    pub followers_count: Option<u64>,
    #[serde(rename = "followsCount", default)]
    pub follows_count: Option<u64>,
    #[serde(rename = "postsCount", default)]
    pub posts_count: Option<u64>,
    /// Per-viewer relationship — whether the signed-in user follows
    /// this actor, is followed back, has them muted/blocked.
    #[serde(default)]
    pub viewer: Option<ActorViewerState>,
    /// Optional StrongRef to a post this actor has pinned to the top
    /// of their profile. `app.bsky.actor.profile.pinnedPost`.
    #[serde(rename = "pinnedPost", default)]
    pub pinned_post: Option<PinnedPostRef>,
}

/// Tiny StrongRef shape used by ActorProfile.pinnedPost.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PinnedPostRef {
    pub uri: String,
    #[serde(default)]
    pub cid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ActorViewerState {
    /// AT-URI of the viewer's follow record, if they follow this actor.
    /// Pass to `delete_record` to unfollow.
    #[serde(default)]
    pub following: Option<String>,
    /// AT-URI of this actor's follow-record pointing back at the
    /// viewer (i.e. the "follows-you" badge condition).
    #[serde(rename = "followedBy", default)]
    pub followed_by: Option<String>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub blocked_by: Option<bool>,
    #[serde(default)]
    pub blocking: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PostView {
    pub uri: String,
    pub cid: String,
    pub author: PostAuthor,
    pub record: PostRecord,
    #[serde(default)]
    pub embed: Option<Embed>,
    #[serde(rename = "indexedAt", default)]
    pub indexed_at: Option<String>,
    #[serde(rename = "replyCount", default)]
    pub reply_count: i64,
    #[serde(rename = "repostCount", default)]
    pub repost_count: i64,
    #[serde(rename = "likeCount", default)]
    pub like_count: i64,
    #[serde(rename = "quoteCount", default)]
    pub quote_count: i64,
    /// Per-viewer state. Tells us whether the *signed-in* user has
    /// already liked or reposted this post, and the AT-URI of their
    /// like/repost record (used to undo it).
    #[serde(default)]
    pub viewer: Option<PostViewerState>,
    /// Moderation labels attached to this post (NSFW, graphic-media,
    /// etc.). Empty for the overwhelming majority of posts. When
    /// non-empty, the renderer shows a content-warning interstitial
    /// before the post body.
    #[serde(default)]
    pub labels: Vec<Label>,
}

/// One moderation label attached to a post by the bsky moderation
/// service or a third-party labeler. The `val` field is the
/// canonical label name ("porn", "sexual", "graphic-media",
/// "nudity", "sensitive", "gore"). `neg: true` means the labeler
/// negated a previous label of the same name (i.e. retracted it).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Label {
    /// DID of the labeler (e.g. the bsky mod team or a third-party).
    pub src: String,
    /// AT-URI the label applies to (the post itself, usually).
    pub uri: String,
    #[serde(default)]
    pub cid: Option<String>,
    /// Canonical label name. Compare against well-known values for
    /// rendering decisions.
    pub val: String,
    /// `true` means this label retracts a prior label of the same
    /// `val` from the same `src`. Renderers should treat the post
    /// as un-labeled in that case.
    #[serde(default)]
    pub neg: bool,
}

impl PostView {
    /// `true` when the post carries any "show me before I see this"
    /// moderation label (porn / sexual / nudity / graphic-media /
    /// sensitive / gore / etc.). Accounts for `neg: true` retractions.
    pub fn needs_content_warning(&self) -> bool {
        if self.labels.is_empty() {
            return false;
        }
        let mut effective: std::collections::HashSet<(&str, &str)> =
            std::collections::HashSet::new();
        for l in &self.labels {
            if l.neg {
                effective.remove(&(l.src.as_str(), l.val.as_str()));
            } else {
                effective.insert((l.src.as_str(), l.val.as_str()));
            }
        }
        effective.iter().any(|(_, val)| is_warning_label(val))
    }

    /// Comma-joined list of distinct warning labels on this post,
    /// used as the heading text for the interstitial. Empty when
    /// `needs_content_warning()` is `false`.
    pub fn warning_label_summary(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for l in &self.labels {
            if l.neg {
                continue;
            }
            if is_warning_label(&l.val) && seen.insert(l.val.as_str()) {
                out.push(label_display(&l.val));
            }
        }
        out.join(" · ")
    }
}

/// Canonical "show user a warning" labels. Mirrors bsky.app's
/// default behavior — these are the labels that get interstitials
/// rather than being silently hidden or shown as-is.
fn is_warning_label(val: &str) -> bool {
    matches!(
        val,
        "porn" | "sexual" | "nudity" | "graphic-media" | "sensitive" | "gore" | "graphic"
    )
}

/// User-facing display name for a label. Falls back to the raw `val`
/// for unknown labels so a future bsky-coined label still renders
/// something readable.
fn label_display(val: &str) -> String {
    match val {
        "porn" => "Adult content".into(),
        "sexual" => "Suggestive".into(),
        "nudity" => "Nudity".into(),
        "graphic-media" => "Graphic media".into(),
        "sensitive" => "Sensitive".into(),
        "gore" | "graphic" => "Graphic".into(),
        other => other.replace('-', " "),
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct PostViewerState {
    /// AT-URI of the viewer's like record, if they liked this post.
    #[serde(default)]
    pub like: Option<String>,
    /// AT-URI of the viewer's repost record, if they reposted.
    #[serde(default)]
    pub repost: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PostAuthor {
    pub did: String,
    pub handle: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PostRecord {
    #[serde(default)]
    pub text: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    /// Raw `facets` array from the lexicon — `[{ index: { byteStart,
    /// byteEnd }, features: [{ $type: "...#mention" | "#link" | "#tag",
    /// did | uri | tag }] }, ...]`. Kept as a Value so a slightly-off
    /// shape from one weird post doesn't blow up the whole feed
    /// (same defensive pattern as FeedItem.reply / .reason). Parse
    /// with [`PostRecord::resolved_facets`] for rendering.
    #[serde(default)]
    pub facets: Option<serde_json::Value>,
}

/// One styled segment of post text — the renderer walks an ordered
/// list of these and either emits a plain `<span>` or a clickable
/// `<button>` per segment.
#[derive(Debug, Clone, PartialEq)]
pub enum FacetSegment {
    /// Plain unstyled text.
    Text(String),
    /// `@handle.example` — click opens the actor's profile (we ship
    /// the resolved DID along so the click site doesn't have to
    /// resolveHandle again).
    Mention { text: String, did: String },
    /// `https://…` — click opens in the system browser via the
    /// safe_open allowlist.
    Link { text: String, uri: String },
    /// `#hashtag` — click opens a search column for the tag.
    Tag { text: String, tag: String },
}

impl PostRecord {
    /// Walk `text` byte-by-byte, slicing it into [`FacetSegment`]s
    /// at the byteStart / byteEnd offsets the lexicon ships. Out-of-
    /// bound or overlapping facets fall through to plain text — we
    /// never panic on a bad facet.
    pub fn resolved_facets(&self) -> Vec<FacetSegment> {
        let bytes = self.text.as_bytes();
        let Some(arr) = self.facets.as_ref().and_then(|v| v.as_array()) else {
            return vec![FacetSegment::Text(self.text.clone())];
        };
        // Collect (start, end, kind) tuples, ignoring malformed ones.
        let mut ranges: Vec<(usize, usize, FacetKind)> = Vec::new();
        for f in arr {
            let Some(idx) = f.get("index") else { continue };
            let Some(start) = idx.get("byteStart").and_then(|v| v.as_u64()) else {
                continue;
            };
            let Some(end) = idx.get("byteEnd").and_then(|v| v.as_u64()) else {
                continue;
            };
            let (start, end) = (start as usize, end as usize);
            if end > bytes.len() || start >= end {
                continue;
            }
            // Multiple features per facet is rare but legal — pick the
            // first one we know how to render.
            let Some(features) = f.get("features").and_then(|v| v.as_array()) else {
                continue;
            };
            let mut kind = None;
            for feat in features {
                let ty = feat.get("$type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "app.bsky.richtext.facet#mention" => {
                        if let Some(d) = feat.get("did").and_then(|v| v.as_str()) {
                            kind = Some(FacetKind::Mention(d.to_string()));
                            break;
                        }
                    }
                    "app.bsky.richtext.facet#link" => {
                        if let Some(u) = feat.get("uri").and_then(|v| v.as_str()) {
                            kind = Some(FacetKind::Link(u.to_string()));
                            break;
                        }
                    }
                    "app.bsky.richtext.facet#tag" => {
                        if let Some(t) = feat.get("tag").and_then(|v| v.as_str()) {
                            kind = Some(FacetKind::Tag(t.to_string()));
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(k) = kind {
                ranges.push((start, end, k));
            }
        }
        // Sort by start; drop ranges that overlap a previous one.
        ranges.sort_by_key(|(s, _, _)| *s);
        let mut deduped: Vec<(usize, usize, FacetKind)> = Vec::new();
        for (s, e, k) in ranges {
            if deduped.last().map(|(_, pe, _)| *pe).unwrap_or(0) > s {
                continue;
            }
            deduped.push((s, e, k));
        }
        // Walk byte ranges + slice the text by valid UTF-8 char
        // boundaries. If a facet's byte offsets don't align to char
        // boundaries (shouldn't happen but lexicon doesn't validate)
        // we drop the facet rather than panicking.
        let mut out: Vec<FacetSegment> = Vec::new();
        let mut cursor = 0;
        for (s, e, kind) in deduped {
            if !self.text.is_char_boundary(s) || !self.text.is_char_boundary(e) {
                continue;
            }
            if s > cursor {
                out.push(FacetSegment::Text(self.text[cursor..s].to_string()));
            }
            let chunk = self.text[s..e].to_string();
            out.push(match kind {
                FacetKind::Mention(did) => FacetSegment::Mention { text: chunk, did },
                FacetKind::Link(uri) => FacetSegment::Link { text: chunk, uri },
                FacetKind::Tag(tag) => FacetSegment::Tag { text: chunk, tag },
            });
            cursor = e;
        }
        if cursor < bytes.len() && self.text.is_char_boundary(cursor) {
            out.push(FacetSegment::Text(self.text[cursor..].to_string()));
        }
        if out.is_empty() {
            out.push(FacetSegment::Text(self.text.clone()));
        }
        out
    }
}

enum FacetKind {
    Mention(String),
    Link(String),
    Tag(String),
}

/// Outer untagged-enum wrapper so an embed whose `$type` we don't
/// model decodes to `Unknown(Value)` instead of failing the whole
/// post decode. The size delta between variants is intentional —
/// `Known` carries the real thing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum Embed {
    Known(EmbedKind),
    Unknown(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "$type")]
pub enum EmbedKind {
    #[serde(rename = "app.bsky.embed.images#view")]
    Images {
        #[serde(default)]
        images: Vec<EmbedImage>,
    },
    #[serde(rename = "app.bsky.embed.external#view")]
    External { external: EmbedExternal },
    #[serde(rename = "app.bsky.embed.record#view")]
    Record { record: EmbedRecordView },
    #[serde(rename = "app.bsky.embed.recordWithMedia#view")]
    RecordWithMedia {
        record: EmbedRecordWrapper,
        media: Box<EmbedMedia>,
    },
    #[serde(rename = "app.bsky.embed.video#view")]
    Video {
        playlist: String,
        thumbnail: Option<String>,
        #[serde(rename = "aspectRatio", default)]
        aspect_ratio: Option<EmbedAspectRatio>,
    },
}

/// Inner-media variant for `recordWithMedia` (a quoted post that
/// itself has images/video/link attached to the *outer* post). Same
/// shape as the top-level [`EmbedKind`] but without the record/quote
/// branches — no triple-nested quotes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "$type")]
pub enum EmbedMedia {
    #[serde(rename = "app.bsky.embed.images#view")]
    Images {
        #[serde(default)]
        images: Vec<EmbedImage>,
    },
    #[serde(rename = "app.bsky.embed.external#view")]
    External { external: EmbedExternal },
    #[serde(rename = "app.bsky.embed.video#view")]
    Video {
        playlist: String,
        thumbnail: Option<String>,
        #[serde(rename = "aspectRatio", default)]
        aspect_ratio: Option<EmbedAspectRatio>,
    },
}

/// Wrapper that mirrors `app.bsky.embed.recordWithMedia#view`'s inner
/// `record` shape: `{ "$type": "app.bsky.embed.record#view", "record": ... }`.
/// Keeping it as a struct (rather than flattening) so the JSON path
/// matches the lexicon 1:1.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EmbedRecordWrapper {
    pub record: EmbedRecordView,
}

/// `app.bsky.embed.record#view`'s inner `record` field. The lexicon
/// has several variants depending on whether the quoted record is a
/// regular post, deleted, blocked, etc.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "$type")]
#[allow(clippy::large_enum_variant)]
pub enum EmbedRecordView {
    /// Successfully resolved quoted post.
    #[serde(rename = "app.bsky.embed.record#viewRecord")]
    View {
        uri: String,
        cid: String,
        author: PostAuthor,
        value: PostRecord,
        #[serde(rename = "indexedAt", default)]
        indexed_at: Option<String>,
        /// Embeds nested inside the quoted post. Renderer should
        /// handle ONLY images / external links here (no double-quotes)
        /// to avoid runaway nesting.
        #[serde(default)]
        embeds: Vec<EmbedKind>,
    },
    /// Quoted post was deleted.
    #[serde(rename = "app.bsky.embed.record#viewNotFound")]
    NotFound { uri: String },
    /// Viewer is blocked from seeing the quoted post.
    #[serde(rename = "app.bsky.embed.record#viewBlocked")]
    Blocked { uri: String },
    /// Author detached the quote.
    #[serde(rename = "app.bsky.embed.record#viewDetached")]
    Detached { uri: String },
    /// Unknown variant (forward-compat — e.g. quoted feed generators,
    /// lists, starter packs). Caller renders a generic fallback.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EmbedAspectRatio {
    pub width: u32,
    pub height: u32,
}

/// `app.bsky.graph.getLists` response — the user's own curated
/// lists (each list is a set of accounts they've grouped). Adding
/// one as a column hits `app.bsky.feed.getListFeed`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListsResponse {
    #[serde(default)]
    pub lists: Vec<ListView>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ListView {
    pub uri: String,
    pub cid: String,
    pub creator: PostAuthor,
    pub name: String,
    /// `"modlist"` (mute/block list) or `"curatelist"` (subscribe-able).
    /// Only curatelists make sense as a column (a modlist would just
    /// show muted/blocked accounts' posts).
    pub purpose: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(rename = "listItemCount", default)]
    pub list_item_count: Option<u64>,
}

/// `app.bsky.actor.getPreferences` response — opaque preferences
/// blob; we only care about the savedFeedsPrefV2 entry.
#[derive(Debug, Clone, Deserialize)]
pub struct PreferencesResponse {
    #[serde(default)]
    pub preferences: Vec<serde_json::Value>,
}

/// One entry in the `app.bsky.actor.defs#savedFeedsPrefV2` list.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SavedFeedItem {
    /// `feed`, `list`, or `timeline`.
    #[serde(rename = "type")]
    pub kind: String,
    /// AT-URI of the feed generator / list, or the literal "following"
    /// for the timeline entry.
    pub value: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub id: Option<String>,
}

impl PreferencesResponse {
    /// Pull the user's saved-feeds list out of the opaque
    /// preferences blob. Handles both shapes the lexicon ships with:
    ///
    /// - `app.bsky.actor.defs#savedFeedsPrefV2` — newer, items array
    ///   with `{ type, value, pinned, id }` per entry.
    /// - `app.bsky.actor.defs#savedFeedsPref` (V1) — older, parallel
    ///   `saved: [uri,...]` + `pinned: [uri,...]` arrays. Old accounts
    ///   that never re-saved a feed still have this shape, and skipping
    ///   it left the picker looking empty even when the user clearly
    ///   had feeds on bsky.app.
    ///
    /// V2 wins when both are present (it's what bsky.app writes today).
    pub fn saved_feeds(&self) -> Vec<SavedFeedItem> {
        let mut v2: Option<Vec<SavedFeedItem>> = None;
        let mut v1: Option<Vec<SavedFeedItem>> = None;
        for entry in &self.preferences {
            let ty = entry.get("$type").and_then(|v| v.as_str()).unwrap_or("");
            match ty {
                "app.bsky.actor.defs#savedFeedsPrefV2" => {
                    if let Some(items) = entry.get("items").and_then(|v| v.as_array()) {
                        let mut out = Vec::with_capacity(items.len());
                        for it in items {
                            if let Ok(sf) = serde_json::from_value::<SavedFeedItem>(it.clone()) {
                                out.push(sf);
                            }
                        }
                        v2 = Some(out);
                    }
                }
                "app.bsky.actor.defs#savedFeedsPref" => {
                    let pinned_uris: std::collections::HashSet<String> = entry
                        .get("pinned")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let saved_uris: Vec<String> = entry
                        .get("saved")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    // V1 saved entries are all feed generators (V1
                    // predates lists in saved-feeds).
                    let mut out: Vec<SavedFeedItem> = saved_uris
                        .into_iter()
                        .map(|uri| SavedFeedItem {
                            pinned: pinned_uris.contains(&uri),
                            kind: "feed".into(),
                            value: uri,
                            id: None,
                        })
                        .collect();
                    // Surface pinned ones first to match bsky.app order.
                    out.sort_by_key(|s| !s.pinned);
                    v1 = Some(out);
                }
                _ => {}
            }
        }
        v2.or(v1).unwrap_or_default()
    }
}

/// `app.bsky.feed.getFeedGenerators` — resolve a batch of feed-generator
/// URIs into displayable view objects (name, description, avatar).
#[derive(Debug, Clone, Deserialize)]
pub struct FeedGeneratorsResponse {
    #[serde(default)]
    pub feeds: Vec<FeedGeneratorView>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FeedGeneratorView {
    pub uri: String,
    pub cid: String,
    pub did: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    pub creator: PostAuthor,
    #[serde(rename = "likeCount", default)]
    pub like_count: u64,
}

/// `app.bsky.actor.getSuggestions` response — personalized list of
/// actors the AppView thinks the viewer might want to follow.
/// `ActorProfile` (the detailed view) is what the AppView returns,
/// so this carries description + counts + viewer state for the
/// Follow button to render correctly.
#[derive(Debug, Clone, Deserialize)]
pub struct SuggestionsResponse {
    #[serde(default)]
    pub actors: Vec<ActorProfile>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `app.bsky.unspecced.getTrendingTopics` response. Two parallel
/// lists: `topics` (what's surging right now) and `suggested`
/// (curated evergreen topics to follow). Both surface in the
/// browser sheet as taps that open a search column.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TrendingTopicsResponse {
    #[serde(default)]
    pub topics: Vec<TrendingTopic>,
    #[serde(default)]
    pub suggested: Vec<TrendingTopic>,
}

/// One entry from getTrendingTopics. `topic` is the user-facing
/// label; `link` is the bsky deep-link (e.g., `/search?q=...` or
/// `/profile/...`) and is what the AppView would navigate to in
/// the browser. We just open the matching search column on click.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TrendingTopic {
    pub topic: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
}

/// Single-page response shape for `app.bsky.graph.getMutes`. Same
/// shape as Suggestions — a vec of profile views plus a cursor.
#[derive(Debug, Clone, Deserialize)]
pub struct MutedActorsResponse {
    #[serde(default)]
    pub mutes: Vec<ActorProfile>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Single-page response shape for `app.bsky.graph.getBlocks`.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockedActorsResponse {
    #[serde(default)]
    pub blocks: Vec<ActorProfile>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Paginated list of actors who liked a post — backs the "tap heart
/// count → see who liked" modal. The `Like` view is the like *record*
/// (with createdAt etc.), but the only field we actually render is
/// `actor`.
#[derive(Debug, Clone, Deserialize)]
pub struct LikesResponse {
    #[serde(default)]
    pub likes: Vec<LikeView>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LikeView {
    pub actor: PostAuthor,
    #[serde(rename = "indexedAt", default)]
    pub indexed_at: Option<String>,
}

/// Paginated list of actors who reposted a post.
#[derive(Debug, Clone, Deserialize)]
pub struct RepostedByResponse {
    #[serde(rename = "repostedBy", default)]
    pub reposted_by: Vec<PostAuthor>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Paginated list of posts that quote a given post.
#[derive(Debug, Clone, Deserialize)]
pub struct QuotesResponse {
    #[serde(default)]
    pub posts: Vec<PostView>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// `app.bsky.graph.getKnownFollowers` — actors followed by the
/// viewer who ALSO follow the given subject. The "mutuals" set.
#[derive(Debug, Clone, Deserialize)]
pub struct KnownFollowersResponse {
    #[serde(default)]
    pub followers: Vec<PostAuthor>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Response wrapper for `app.bsky.feed.getPostThread`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GetPostThreadResponse {
    pub thread: ThreadView,
}

/// One node of a Bluesky thread. The lexicon discriminates on
/// `$type`; we cover the common cases and treat unknowns as the
/// `Other` terminal so a brand-new server variant doesn't crash the
/// client.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "$type")]
#[allow(clippy::large_enum_variant)]
pub enum ThreadView {
    /// A real post in the thread. May have a parent (ascending the
    /// reply chain) and replies (descending — sorted server-side).
    #[serde(rename = "app.bsky.feed.defs#threadViewPost")]
    Post {
        post: PostView,
        /// Box because the chain is unbounded and Rust enums need a
        /// known size.
        #[serde(default)]
        parent: Option<Box<ThreadView>>,
        #[serde(default)]
        replies: Option<Vec<ThreadView>>,
    },
    /// Parent / reply was deleted.
    #[serde(rename = "app.bsky.feed.defs#notFoundPost")]
    NotFound { uri: String },
    /// Viewer is blocked from seeing this part of the thread.
    #[serde(rename = "app.bsky.feed.defs#blockedPost")]
    Blocked { uri: String },
    /// Future-proof — any unknown `$type` collapses here. Renderer
    /// treats it as a silent terminator.
    #[serde(other)]
    Other,
}

impl ThreadView {
    /// Walk `parent` chain, collecting URIs from closest-parent up to
    /// thread root. Useful for breadcrumb-style rendering.
    pub fn parent_chain(&self) -> Vec<&ThreadView> {
        let mut out = Vec::new();
        let mut cur = match self {
            ThreadView::Post {
                parent: Some(p), ..
            } => Some(p.as_ref()),
            _ => None,
        };
        while let Some(node) = cur {
            out.push(node);
            cur = match node {
                ThreadView::Post {
                    parent: Some(p), ..
                } => Some(p.as_ref()),
                _ => None,
            };
        }
        out
    }

    /// The PostView at THIS node, if this is a Post variant.
    pub fn post(&self) -> Option<&PostView> {
        match self {
            ThreadView::Post { post, .. } => Some(post),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EmbedImage {
    pub thumb: String,
    pub fullsize: String,
    #[serde(default)]
    pub alt: String,
    /// Per-image dimensions from the lexicon. Used by the renderer
    /// to reserve a correctly-shaped placeholder before the image
    /// decodes — without this single-image embeds reflow from
    /// 0-height to the decoded height while scrolling, producing
    /// the "flashing" jank in long threads.
    #[serde(rename = "aspectRatio", default)]
    pub aspect_ratio: Option<EmbedAspectRatio>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EmbedExternal {
    pub uri: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub thumb: Option<String>,
}

impl PostView {
    /// Display name with handle fallback. Bluesky users can omit display names.
    pub fn display_name(&self) -> &str {
        self.author
            .display_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.author.handle)
    }

    /// First image thumbnail, if the post embeds images. Used to render
    /// the column-style preview without pulling in a full embed renderer.
    pub fn first_image_thumb(&self) -> Option<&str> {
        match &self.embed {
            Some(Embed::Known(EmbedKind::Images { images })) => {
                images.first().map(|i| i.thumb.as_str())
            }
            Some(Embed::Known(EmbedKind::External { external })) => external.thumb.as_deref(),
            _ => None,
        }
    }

    /// Compact relative time ("2m", "1h", "3d") for column rendering.
    pub fn relative_time(&self) -> String {
        let raw = self
            .indexed_at
            .as_deref()
            .or(self.record.created_at.as_deref());
        let Some(s) = raw else { return String::new() };
        let Ok(ts) = chrono::DateTime::parse_from_rfc3339(s) else {
            return String::new();
        };
        let now = chrono::Utc::now();
        let delta = now.signed_duration_since(ts.with_timezone(&chrono::Utc));
        if delta.num_seconds() < 60 {
            format!("{}s", delta.num_seconds().max(0))
        } else if delta.num_minutes() < 60 {
            format!("{}m", delta.num_minutes())
        } else if delta.num_hours() < 24 {
            format!("{}h", delta.num_hours())
        } else if delta.num_days() < 30 {
            format!("{}d", delta.num_days())
        } else {
            format!("{}mo", delta.num_days() / 30)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_feed_response() {
        let body = serde_json::json!({
            "feed": [
                {
                    "post": {
                        "uri": "at://did:plc:abc/app.bsky.feed.post/1",
                        "cid": "bafy123",
                        "author": {
                            "did": "did:plc:abc",
                            "handle": "alice.bsky.social",
                            "displayName": "Alice",
                            "avatar": "https://cdn/avatar.png"
                        },
                        "record": {
                            "text": "Hello deck!",
                            "createdAt": "2026-05-22T03:00:00Z"
                        },
                        "embed": {
                            "$type": "app.bsky.embed.images#view",
                            "images": [{ "thumb": "https://cdn/t.png", "fullsize": "https://cdn/f.png", "alt": "alt" }]
                        },
                        "indexedAt": "2026-05-22T03:00:01Z",
                        "replyCount": 3,
                        "repostCount": 5,
                        "likeCount": 12
                    }
                }
            ],
            "cursor": "next-cursor-123"
        });
        let parsed: FeedResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.feed.len(), 1);
        assert_eq!(parsed.cursor.as_deref(), Some("next-cursor-123"));
        let p = &parsed.feed[0].post;
        assert_eq!(p.display_name(), "Alice");
        assert_eq!(p.first_image_thumb(), Some("https://cdn/t.png"));
        assert_eq!(p.like_count, 12);
    }

    #[test]
    fn display_name_falls_back_to_handle() {
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "bob.bsky.social" },
            "record": { "text": "" }
        }))
        .unwrap();
        assert_eq!(p.display_name(), "bob.bsky.social");
        assert_eq!(p.first_image_thumb(), None);
    }

    #[test]
    fn relative_time_for_recent_post() {
        let now = chrono::Utc::now();
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "h" },
            "record": { "text": "" },
            "indexedAt": (now - chrono::Duration::minutes(5)).to_rfc3339()
        }))
        .unwrap();
        assert_eq!(p.relative_time(), "5m");
    }

    #[test]
    fn record_embed_decodes_quoted_post() {
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "h" },
            "record": { "text": "look at this quote" },
            "embed": {
                "$type": "app.bsky.embed.record#view",
                "record": {
                    "$type": "app.bsky.embed.record#viewRecord",
                    "uri": "at://did:plc:q/app.bsky.feed.post/1",
                    "cid": "qcid",
                    "author": { "did": "did:plc:q", "handle": "quoted.bsky.social", "displayName": "Quoted" },
                    "value": { "text": "the quoted text", "createdAt": "2026-05-01T00:00:00Z" },
                    "indexedAt": "2026-05-01T00:00:01Z"
                }
            }
        })).unwrap();
        match p.embed {
            Some(Embed::Known(EmbedKind::Record { record })) => match record {
                EmbedRecordView::View {
                    uri, author, value, ..
                } => {
                    assert_eq!(uri, "at://did:plc:q/app.bsky.feed.post/1");
                    assert_eq!(author.handle, "quoted.bsky.social");
                    assert_eq!(value.text, "the quoted text");
                }
                _ => panic!("expected View variant"),
            },
            other => panic!("expected Record embed, got {other:?}"),
        }
    }

    #[test]
    fn record_with_media_decodes_quote_plus_image() {
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "h" },
            "record": { "text": "quoted with my own image" },
            "embed": {
                "$type": "app.bsky.embed.recordWithMedia#view",
                "record": {
                    "record": {
                        "$type": "app.bsky.embed.record#viewRecord",
                        "uri": "at://q",
                        "cid": "qcid",
                        "author": { "did": "did:plc:q", "handle": "qa.bsky.social" },
                        "value": { "text": "inside quote" }
                    }
                },
                "media": {
                    "$type": "app.bsky.embed.images#view",
                    "images": [{ "thumb": "https://t", "fullsize": "https://f", "alt": "a" }]
                }
            }
        }))
        .unwrap();
        let Some(Embed::Known(EmbedKind::RecordWithMedia { record, media })) = p.embed else {
            panic!("expected RecordWithMedia");
        };
        let EmbedRecordView::View { value, .. } = record.record else {
            panic!("expected View variant");
        };
        assert_eq!(value.text, "inside quote");
        match *media {
            EmbedMedia::Images { images } => {
                assert_eq!(images.len(), 1);
                assert_eq!(images[0].fullsize, "https://f");
            }
            other => panic!("expected Images media, got {other:?}"),
        }
    }

    #[test]
    fn record_embed_not_found_decodes() {
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "h" },
            "record": { "text": "this quoted post was deleted" },
            "embed": {
                "$type": "app.bsky.embed.record#view",
                "record": {
                    "$type": "app.bsky.embed.record#viewNotFound",
                    "uri": "at://deleted",
                    "notFound": true
                }
            }
        }))
        .unwrap();
        let Some(Embed::Known(EmbedKind::Record { record })) = p.embed else {
            panic!("expected Record embed");
        };
        assert!(matches!(record, EmbedRecordView::NotFound { .. }));
    }

    #[test]
    fn video_embed_decodes_with_aspect_ratio() {
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "h" },
            "record": { "text": "" },
            "embed": {
                "$type": "app.bsky.embed.video#view",
                "playlist": "https://cdn/video.m3u8",
                "thumbnail": "https://cdn/thumb.jpg",
                "aspectRatio": { "width": 1920, "height": 1080 }
            }
        }))
        .unwrap();
        let Some(Embed::Known(EmbedKind::Video {
            playlist,
            thumbnail,
            aspect_ratio,
        })) = p.embed
        else {
            panic!("expected Video embed");
        };
        assert_eq!(playlist, "https://cdn/video.m3u8");
        assert_eq!(thumbnail.as_deref(), Some("https://cdn/thumb.jpg"));
        let ar = aspect_ratio.unwrap();
        assert_eq!(ar.width, 1920);
        assert_eq!(ar.height, 1080);
    }

    #[test]
    fn unknown_embeds_decode_to_unknown_variant() {
        let p: PostView = serde_json::from_value(serde_json::json!({
            "uri": "at://x", "cid": "y",
            "author": { "did": "d", "handle": "h" },
            "record": { "text": "" },
            "embed": { "$type": "app.bsky.embed.something.new", "weird": true }
        }))
        .unwrap();
        assert!(matches!(p.embed, Some(Embed::Unknown(_))));
    }

    // ── PreferencesResponse: V1 + V2 shapes ────────────────────────

    #[test]
    fn saved_feeds_parses_v2_items() {
        let prefs: PreferencesResponse = serde_json::from_value(serde_json::json!({
            "preferences": [
                {
                    "$type": "app.bsky.actor.defs#savedFeedsPrefV2",
                    "items": [
                        { "type": "feed", "value": "at://did:plc:a/app.bsky.feed.generator/x", "pinned": true,  "id": "id1" },
                        { "type": "list", "value": "at://did:plc:b/app.bsky.graph.list/y",      "pinned": false, "id": "id2" },
                    ]
                }
            ]
        })).unwrap();
        let saved = prefs.saved_feeds();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].kind, "feed");
        assert!(saved[0].pinned);
        assert_eq!(saved[1].kind, "list");
        assert!(!saved[1].pinned);
    }

    #[test]
    fn saved_feeds_falls_back_to_v1_when_v2_absent() {
        // V1 = parallel saved[] + pinned[] arrays of URIs. Older
        // accounts that never re-saved a feed still have this shape.
        let prefs: PreferencesResponse = serde_json::from_value(serde_json::json!({
            "preferences": [
                {
                    "$type": "app.bsky.actor.defs#savedFeedsPref",
                    "saved":  ["at://feed/a", "at://feed/b", "at://feed/c"],
                    "pinned": ["at://feed/b"]
                }
            ]
        }))
        .unwrap();
        let saved = prefs.saved_feeds();
        assert_eq!(saved.len(), 3);
        // V1 entries are all treated as kind=feed.
        assert!(saved.iter().all(|s| s.kind == "feed"));
        // Pinned entries surface first (matches bsky.app order).
        assert!(saved[0].pinned, "first row should be the pinned feed");
        assert_eq!(saved[0].value, "at://feed/b");
    }

    #[test]
    fn saved_feeds_prefers_v2_when_both_shapes_present() {
        // Some accounts have both blobs sitting in their prefs.
        // V2 is what bsky.app writes today; pick it.
        let prefs: PreferencesResponse = serde_json::from_value(serde_json::json!({
            "preferences": [
                {
                    "$type": "app.bsky.actor.defs#savedFeedsPref",
                    "saved":  ["at://old/v1"], "pinned": []
                },
                {
                    "$type": "app.bsky.actor.defs#savedFeedsPrefV2",
                    "items": [{ "type": "feed", "value": "at://new/v2", "pinned": true }]
                }
            ]
        }))
        .unwrap();
        let saved = prefs.saved_feeds();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].value, "at://new/v2");
    }

    #[test]
    fn saved_feeds_empty_when_no_prefs_match() {
        let prefs: PreferencesResponse = serde_json::from_value(serde_json::json!({
            "preferences": [
                { "$type": "app.bsky.actor.defs#interestsPref", "tags": ["rust"] }
            ]
        }))
        .unwrap();
        assert!(prefs.saved_feeds().is_empty());
    }

    // ── FeedItem reply / reason helpers ────────────────────────────

    #[test]
    fn feed_item_reposter_extracts_display_and_did() {
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reason": {
                "$type": "app.bsky.feed.defs#reasonRepost",
                "by": { "did": "did:plc:reposter", "handle": "rp.bsky.social", "displayName": "Reposter Pat" }
            }
        })).unwrap();
        assert_eq!(item.reposter_display().as_deref(), Some("Reposter Pat"));
        assert_eq!(item.reposter_did().as_deref(), Some("did:plc:reposter"));
    }

    #[test]
    fn feed_item_reposter_falls_back_to_handle_when_no_display_name() {
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reason": {
                "$type": "app.bsky.feed.defs#reasonRepost",
                "by": { "did": "did:plc:rp", "handle": "rp.bsky.social" }
            }
        }))
        .unwrap();
        assert_eq!(item.reposter_display().as_deref(), Some("rp.bsky.social"));
    }

    #[test]
    fn feed_item_reposter_none_for_non_repost_reason() {
        // Custom feed generators sometimes attach a "reasonPin" or
        // similar tag we don't model. Should NOT surface as a repost.
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reason": { "$type": "app.bsky.feed.defs#reasonPin" }
        }))
        .unwrap();
        assert!(item.reposter_display().is_none());
        assert!(item.reposter_did().is_none());
    }

    #[test]
    fn feed_item_reply_parent_handle_extracts_author() {
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reply": {
                "parent": {
                    "uri": "at://parent",
                    "cid": "pcid",
                    "author": { "did": "did:plc:parent", "handle": "parent.bsky.social" },
                    "record": { "text": "original post" }
                }
            }
        }))
        .unwrap();
        assert_eq!(
            item.reply_parent_handle().as_deref(),
            Some("parent.bsky.social")
        );
    }

    #[test]
    fn feed_item_reply_parent_handle_none_for_not_found_post() {
        // Reply to a deleted post: lexicon ships a #notFoundPost
        // shape instead of a real PostView. Renderer should treat
        // this as "no parent" rather than crashing.
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reply": {
                "parent": {
                    "$type": "app.bsky.feed.defs#notFoundPost",
                    "uri": "at://gone",
                    "notFound": true
                }
            }
        }))
        .unwrap();
        assert!(item.reply_parent_handle().is_none());
    }

    #[test]
    fn feed_item_reply_parent_handle_none_for_blocked_post() {
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reply": {
                "parent": {
                    "$type": "app.bsky.feed.defs#blockedPost",
                    "uri": "at://blocked",
                    "blocked": true
                }
            }
        }))
        .unwrap();
        assert!(item.reply_parent_handle().is_none());
    }

    #[test]
    fn feed_item_with_no_reply_or_reason_returns_none() {
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            }
        }))
        .unwrap();
        assert!(item.reposter_display().is_none());
        assert!(item.reposter_did().is_none());
        assert!(item.reply_parent_handle().is_none());
    }

    #[test]
    fn feed_item_with_garbage_reason_doesnt_panic() {
        // Defensive: someone could ship a reason with no $type, or
        // a $type that's not a string. We should silently return
        // None, not unwrap.
        let item: FeedItem = serde_json::from_value(serde_json::json!({
            "post": {
                "uri": "at://x", "cid": "y",
                "author": { "did": "d", "handle": "h" },
                "record": { "text": "" }
            },
            "reason": { "weird": "no $type at all" }
        }))
        .unwrap();
        assert!(item.reposter_display().is_none());
        assert!(item.reposter_did().is_none());
    }

    // ── PostRecord.resolved_facets ─────────────────────────────────

    fn rec(text: &str, facets: serde_json::Value) -> PostRecord {
        PostRecord {
            text: text.into(),
            created_at: None,
            facets: Some(facets),
        }
    }

    #[test]
    fn facets_none_returns_single_text_segment() {
        let r = PostRecord {
            text: "hello world".into(),
            created_at: None,
            facets: None,
        };
        let s = r.resolved_facets();
        assert_eq!(s.len(), 1);
        matches!(&s[0], FacetSegment::Text(t) if t == "hello world");
    }

    #[test]
    fn mention_facet_resolves_to_did() {
        // "hi @alice." — bytes 3..9 are "@alice".
        let r = rec(
            "hi @alice.",
            serde_json::json!([{
                "index": { "byteStart": 3, "byteEnd": 9 },
                "features": [{
                    "$type": "app.bsky.richtext.facet#mention",
                    "did": "did:plc:alice"
                }]
            }]),
        );
        let s = r.resolved_facets();
        assert_eq!(s.len(), 3);
        assert!(matches!(&s[0], FacetSegment::Text(t) if t == "hi "));
        assert!(
            matches!(&s[1], FacetSegment::Mention { text, did } if text == "@alice" && did == "did:plc:alice")
        );
        assert!(matches!(&s[2], FacetSegment::Text(t) if t == "."));
    }

    #[test]
    fn link_facet_resolves_to_uri() {
        let r = rec(
            "go here",
            serde_json::json!([{
                "index": { "byteStart": 3, "byteEnd": 7 },
                "features": [{
                    "$type": "app.bsky.richtext.facet#link",
                    "uri": "https://example.com/x"
                }]
            }]),
        );
        let s = r.resolved_facets();
        assert!(s.iter().any(|seg| matches!(seg, FacetSegment::Link { text, uri } if text == "here" && uri == "https://example.com/x")));
    }

    #[test]
    fn tag_facet_strips_hash_from_text() {
        let r = rec(
            "love #rust today",
            serde_json::json!([{
                "index": { "byteStart": 5, "byteEnd": 10 },
                "features": [{ "$type": "app.bsky.richtext.facet#tag", "tag": "rust" }]
            }]),
        );
        let s = r.resolved_facets();
        // The rendered segment text includes the hash (we slice
        // straight from the post body); the tag value sans hash is
        // what's used for searching.
        assert!(s.iter().any(
            |seg| matches!(seg, FacetSegment::Tag { text, tag } if text == "#rust" && tag == "rust")
        ));
    }

    #[test]
    fn out_of_bounds_facets_are_dropped() {
        // byteEnd past end of text — drop this facet, fall through
        // to plain text rendering.
        let r = rec(
            "short",
            serde_json::json!([{
                "index": { "byteStart": 0, "byteEnd": 999 },
                "features": [{
                    "$type": "app.bsky.richtext.facet#mention",
                    "did": "did:plc:nope"
                }]
            }]),
        );
        let s = r.resolved_facets();
        assert_eq!(s.len(), 1);
        assert!(matches!(&s[0], FacetSegment::Text(t) if t == "short"));
    }

    #[test]
    fn malformed_facets_dont_panic() {
        let r = rec(
            "hello",
            serde_json::json!([
                { "weird": "no index or features" },
                { "index": { "byteStart": "not-a-number", "byteEnd": 5 } },
                { "index": { "byteStart": 0, "byteEnd": 3 } },  // no features
            ]),
        );
        let s = r.resolved_facets();
        // Everything malformed got dropped — full text falls through.
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn overlapping_facets_keep_first() {
        // Two facets covering overlapping ranges — first one wins,
        // second is dropped. Prevents the renderer from producing
        // nested / corrupt segments.
        let r = rec(
            "hello world bsky",
            serde_json::json!([
                {
                    "index": { "byteStart": 0, "byteEnd": 11 },
                    "features": [{ "$type": "app.bsky.richtext.facet#tag", "tag": "hw" }]
                },
                {
                    "index": { "byteStart": 6, "byteEnd": 11 },
                    "features": [{ "$type": "app.bsky.richtext.facet#tag", "tag": "world" }]
                }
            ]),
        );
        let s = r.resolved_facets();
        // Should be: tag("hello world") + text(" bsky"). NOT three
        // overlapping segments.
        assert_eq!(s.len(), 2);
        assert!(matches!(&s[0], FacetSegment::Tag { text, .. } if text == "hello world"));
    }

    #[test]
    fn unicode_text_with_emoji_handles_byte_offsets() {
        // "👋 @alice" — the wave is 4 bytes, " @alice" is bytes 5..11.
        let r = rec(
            "👋 @alice",
            serde_json::json!([{
                "index": { "byteStart": 5, "byteEnd": 11 },
                "features": [{
                    "$type": "app.bsky.richtext.facet#mention",
                    "did": "did:plc:alice"
                }]
            }]),
        );
        let s = r.resolved_facets();
        assert!(s
            .iter()
            .any(|seg| matches!(seg, FacetSegment::Mention { text, .. } if text == "@alice")));
    }
}
