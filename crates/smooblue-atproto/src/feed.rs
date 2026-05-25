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
        "porn"
            | "sexual"
            | "nudity"
            | "graphic-media"
            | "sensitive"
            | "gore"
            | "graphic"
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
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
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
    /// preferences blob. Returns `None` if the user hasn't set any
    /// preferences (account is fresh, or just hasn't customized).
    pub fn saved_feeds(&self) -> Vec<SavedFeedItem> {
        for entry in &self.preferences {
            let ty = entry.get("$type").and_then(|v| v.as_str()).unwrap_or("");
            if ty == "app.bsky.actor.defs#savedFeedsPrefV2" {
                if let Some(items) = entry.get("items").and_then(|v| v.as_array()) {
                    let mut out = Vec::with_capacity(items.len());
                    for it in items {
                        if let Ok(sf) = serde_json::from_value::<SavedFeedItem>(it.clone()) {
                            out.push(sf);
                        }
                    }
                    return out;
                }
            }
        }
        Vec::new()
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
}
