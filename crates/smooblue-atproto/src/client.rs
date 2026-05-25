//! XRPC client with DPoP-bound auth + nonce retry.

use crate::error::AtError;
use crate::feed::FeedResponse;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use smooblue_oauth::Session;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Response from `com.atproto.repo.createRecord` — the URI of the new
/// record (which callers need to later delete it for unlike/unrepost).
#[derive(Clone, Debug, Deserialize)]
pub struct CreatedRecord {
    pub uri: String,
    pub cid: String,
}

/// Strong reference to a post (AT-URI + CID) — used wherever the bsky
/// lexicon needs to cite an existing record (reply parents, repost
/// subjects, like subjects).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrongRef {
    pub uri: String,
    pub cid: String,
}

/// Reply context (`reply.root` + `reply.parent` per the
/// `app.bsky.feed.post` lexicon). For first-level replies the root and
/// parent are usually the same post.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyRef {
    pub root: StrongRef,
    pub parent: StrongRef,
}

/// What `com.atproto.repo.uploadBlob` returns — a CID-bearing blob ref
/// that the bsky lexicon embeds verbatim inside the post record.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BlobRef {
    #[serde(rename = "$type")]
    #[serde(default = "default_blob_type")]
    pub kind: String,
    #[serde(rename = "ref")]
    pub link: BlobLink,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub size: u64,
}

fn default_blob_type() -> String {
    "blob".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BlobLink {
    #[serde(rename = "$link")]
    pub cid: String,
}

/// Single image attached to a post (`app.bsky.embed.images` item).
#[derive(Clone, Debug, Serialize)]
pub struct PostImage {
    /// Serialized as `image` per the lexicon
    /// (`app.bsky.embed.images#image`). The previous field name
    /// `blob` shipped a record the AppView rejected with
    /// `Missing required key "image"`.
    #[serde(rename = "image")]
    pub blob: BlobRef,
    /// Screen-reader description. Empty string is valid but discouraged.
    pub alt: String,
    /// Aspect-ratio hint so clients can lay out the placeholder without
    /// downloading the full thumbnail.
    #[serde(rename = "aspectRatio", skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<AspectRatio>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

/// Lightweight AT-URI breakdown: `at://<did>/<collection>/<rkey>`.
pub(crate) struct AtUriParts<'a> {
    pub did: &'a str,
    pub collection: &'a str,
    pub rkey: &'a str,
}

/// Split an AT-URI into its three path parts. Returns `None` if the URI
/// doesn't match `at://<did>/<collection>/<rkey>`.
pub(crate) fn parse_at_uri(uri: &str) -> Option<AtUriParts<'_>> {
    let rest = uri.strip_prefix("at://")?;
    let mut parts = rest.splitn(3, '/');
    let did = parts.next()?;
    let collection = parts.next()?;
    let rkey = parts.next()?;
    if did.is_empty() || collection.is_empty() || rkey.is_empty() {
        return None;
    }
    Some(AtUriParts {
        did,
        collection,
        rkey,
    })
}

#[derive(Clone)]
pub struct AtClient {
    http: reqwest::Client,
    session: Arc<Mutex<Session>>,
    appview: Url,
}

impl AtClient {
    pub fn new(session: Session, appview: Url) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("smooblue/0.1 (+https://smoo.ai)")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client builds");
        Self {
            http,
            session: Arc::new(Mutex::new(session)),
            appview,
        }
    }

    pub fn with_http(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// Read-only access to the current session (e.g., DID for display).
    pub fn session(&self) -> Session {
        self.session.lock().clone()
    }

    /// `app.bsky.feed.getTimeline` — the Home column feed.
    pub async fn get_timeline(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<FeedResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getTimeline")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.notification.listNotifications` — backs the Notifications column.
    pub async fn list_notifications(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::notifications::NotificationsResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.notification.listNotifications")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.actor.getProfile` — full profile view (display name, avatar,
    /// description, follower counts). Used by the CRM opt-in flow and the
    /// (forthcoming) Profile column.
    pub async fn get_profile(&self, actor: &str) -> Result<crate::feed::ActorProfile, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.actor.getProfile")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut().append_pair("actor", actor);
        self.get_json(&url).await
    }

    /// `app.bsky.feed.getAuthorFeed` — for profile / single-author columns.
    pub async fn get_author_feed(
        &self,
        actor: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<FeedResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getAuthorFeed")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("actor", actor)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.feed.searchPosts` — text search across all posts.
    pub async fn search_posts(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<FeedResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.searchPosts")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        // searchPosts returns `posts: [PostView]` — wrap into FeedResponse so
        // the column renderer can stay generic.
        #[derive(serde::Deserialize)]
        struct SearchResp {
            #[serde(default)]
            posts: Vec<crate::feed::PostView>,
            cursor: Option<String>,
        }
        let r: SearchResp = self.get_json(&url).await?;
        Ok(FeedResponse {
            cursor: r.cursor,
            feed: r
                .posts
                .into_iter()
                .map(|p| crate::feed::FeedItem { post: p, reply: None, reason: None })
                .collect(),
        })
    }

    /// `app.bsky.feed.getLikes` — paginated list of actors who liked
    /// a post. Backs the "tap heart count → see who liked" modal.
    pub async fn get_likes(
        &self,
        post_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::feed::LikesResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getLikes")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("uri", post_uri)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.feed.getRepostedBy` — paginated list of actors who
    /// reposted a post.
    pub async fn get_reposted_by(
        &self,
        post_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::feed::RepostedByResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getRepostedBy")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("uri", post_uri)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.feed.getQuotes` — paginated list of posts that
    /// quote a given post. Returns full PostViews so the caller can
    /// render them with the standard PostCard.
    pub async fn get_quotes(
        &self,
        post_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::feed::QuotesResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getQuotes")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("uri", post_uri)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.graph.getLists` — curated lists owned by `actor`.
    /// Used by the lists picker to show the signed-in user their
    /// own lists for adding as columns.
    pub async fn get_lists(
        &self,
        actor: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::feed::ListsResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.graph.getLists")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("actor", actor)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.actor.getPreferences` — the user's preferences blob,
    /// including saved feeds + lists. Backs the saved-feeds picker.
    pub async fn get_preferences(&self) -> Result<crate::feed::PreferencesResponse, AtError> {
        let url = self
            .appview
            .join("/xrpc/app.bsky.actor.getPreferences")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.get_json(&url).await
    }

    /// `app.bsky.feed.getFeedGenerators` — batch-resolve up to 25
    /// feed-generator URIs into display views (name, description,
    /// avatar). Pair with [`Self::get_preferences`] to render the
    /// saved-feeds picker.
    pub async fn get_feed_generators(
        &self,
        feed_uris: &[String],
    ) -> Result<crate::feed::FeedGeneratorsResponse, AtError> {
        if feed_uris.is_empty() {
            return Ok(crate::feed::FeedGeneratorsResponse { feeds: Vec::new() });
        }
        let mut out_feeds: Vec<crate::feed::FeedGeneratorView> = Vec::new();
        // The lexicon caps at 25 URIs per call.
        for chunk in feed_uris.chunks(25) {
            let mut url = self
                .appview
                .join("/xrpc/app.bsky.feed.getFeedGenerators")
                .map_err(|e| AtError::Decode(e.to_string()))?;
            {
                let mut q = url.query_pairs_mut();
                for u in chunk {
                    q.append_pair("feeds", u);
                }
            }
            let r: crate::feed::FeedGeneratorsResponse = self.get_json(&url).await?;
            out_feeds.extend(r.feeds);
        }
        Ok(crate::feed::FeedGeneratorsResponse { feeds: out_feeds })
    }

    /// `app.bsky.actor.getSuggestions` — a personalized list of
    /// actors the AppView thinks the viewer might want to follow.
    /// Backs the "Suggested follows" column.
    pub async fn get_suggestions(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::feed::SuggestionsResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.actor.getSuggestions")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.graph.getKnownFollowers` — actors followed by the
    /// signed-in viewer who also follow the given subject. The
    /// "mutuals" set, used for the "Followed by alice, bob and N
    /// others you follow" social proof on a profile sheet.
    pub async fn get_known_followers(
        &self,
        actor: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<crate::feed::KnownFollowersResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.graph.getKnownFollowers")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("actor", actor)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.feed.getPostThread` — full thread context for a
    /// single post. Returns the focused post plus its parent chain
    /// (up to `parent_height` ancestors) and replies (up to `depth`
    /// levels deep). Defaults follow Bluesky's: parent_height=10,
    /// depth=6.
    pub async fn get_post_thread(
        &self,
        uri: &str,
        depth: u32,
        parent_height: u32,
    ) -> Result<crate::feed::ThreadView, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getPostThread")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("uri", uri)
            .append_pair("depth", &depth.to_string())
            .append_pair("parentHeight", &parent_height.to_string());
        let r: crate::feed::GetPostThreadResponse = self.get_json(&url).await?;
        Ok(r.thread)
    }

    /// `app.bsky.feed.getPosts` — batch hydrate up to 25 posts by
    /// AT-URI in one round trip. Used by the Notifications column to
    /// render the subject post under each "liked / replied to / etc."
    /// notification.
    ///
    /// If more than 25 URIs are passed, makes multiple calls and
    /// concatenates. Silently drops any URIs the server can't
    /// resolve — the caller looks up by URI so missing entries just
    /// render unhydrated.
    pub async fn get_posts(&self, uris: &[String]) -> Result<Vec<crate::feed::PostView>, AtError> {
        if uris.is_empty() {
            return Ok(Vec::new());
        }
        #[derive(serde::Deserialize)]
        struct R {
            #[serde(default)]
            posts: Vec<crate::feed::PostView>,
        }
        let mut out: Vec<crate::feed::PostView> = Vec::with_capacity(uris.len());
        for chunk in uris.chunks(25) {
            let mut url = self
                .appview
                .join("/xrpc/app.bsky.feed.getPosts")
                .map_err(|e| AtError::Decode(e.to_string()))?;
            {
                let mut q = url.query_pairs_mut();
                for u in chunk {
                    q.append_pair("uris", u);
                }
            }
            let r: R = self.get_json(&url).await?;
            out.extend(r.posts);
        }
        Ok(out)
    }

    /// `app.bsky.feed.getListFeed` — posts from members of a curated
    /// list. Reuses the standard FeedResponse shape so the column
    /// renderer doesn't need a special branch.
    pub async fn get_list_feed(
        &self,
        list_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<FeedResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getListFeed")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("list", list_uri)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.feed.getFeed` — fetch a custom feed (e.g. "Indianapolis
    /// Sports 1"). `feed_uri` is the AT-URI of the feed generator record.
    pub async fn get_feed(
        &self,
        feed_uri: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<FeedResponse, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/app.bsky.feed.getFeed")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("feed", feed_uri)
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `app.bsky.notification.updateSeen` — POST that marks all
    /// notifications older than `seen_at` as read. Smooblue fires
    /// this when the Notifications column comes into view (with
    /// `now()` as the timestamp) so the unread badge clears
    /// automatically — same UX as bsky.app.
    pub async fn update_seen(&self, seen_at: chrono::DateTime<chrono::Utc>) -> Result<(), AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.notification.updateSeen")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = serde_json::json!({ "seenAt": seen_at.to_rfc3339() });
        // Endpoint returns an empty body on success — but post_json
        // expects JSON, so deserialize into a serde_json::Value
        // (accepts {}, []  or even an empty string after our
        // body-read logic).
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// `app.bsky.notification.getUnreadCount` — cheap call for the
    /// hybrid Notifications polling (poll this every few seconds; only
    /// fetch the full list when the count actually changes).
    pub async fn get_unread_count(&self) -> Result<u32, AtError> {
        let url = self
            .appview
            .join("/xrpc/app.bsky.notification.getUnreadCount")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        #[derive(serde::Deserialize)]
        struct R {
            count: u32,
        }
        let r: R = self.get_json(&url).await?;
        Ok(r.count)
    }

    /// Create a top-level post (`app.bsky.feed.post` via
    /// `com.atproto.repo.createRecord`). Returns the new record's
    /// AT-URI + CID so callers can immediately wire likes/reposts/replies.
    pub async fn create_post(&self, text: &str) -> Result<CreatedRecord, AtError> {
        self.create_post_full(text, None, &[], &[], None).await
    }

    /// Same as [`Self::create_post`] but adds a reply context. The
    /// `root` is the top of the thread; the `parent` is the post being
    /// directly replied to (often the same for first-level replies).
    pub async fn create_post_with_reply(
        &self,
        text: &str,
        reply: Option<&ReplyRef>,
    ) -> Result<CreatedRecord, AtError> {
        self.create_post_full(text, reply, &[], &[], None).await
    }

    /// Full post creation — text + optional reply + optional image
    /// attachments + optional rich-text facets + optional quote
    /// target (a StrongRef to the post being quoted). Pass at most 4
    /// images per the bsky lexicon. Each image's `blob` field must
    /// come from a prior [`Self::upload_blob`] call.
    ///
    /// Embed combinations:
    /// - text only          → no embed
    /// - text + images      → `app.bsky.embed.images`
    /// - text + quote       → `app.bsky.embed.record`
    /// - text + quote + img → `app.bsky.embed.recordWithMedia`
    /// (Mutually exclusive on the wire — `recordWithMedia` is bsky's
    /// "quote AND image" carrier.)
    pub async fn create_post_full(
        &self,
        text: &str,
        reply: Option<&ReplyRef>,
        images: &[PostImage],
        facets: &[crate::richtext::Facet],
        quote: Option<&StrongRef>,
    ) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().did.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let mut record = serde_json::json!({
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": created_at,
        });
        if let Some(r) = reply {
            record["reply"] = serde_json::json!({
                "root":   { "uri": r.root.uri,   "cid": r.root.cid },
                "parent": { "uri": r.parent.uri, "cid": r.parent.cid },
            });
        }
        // Embed: pick the combination that matches what's set.
        let trimmed_images = if images.is_empty() {
            None
        } else {
            Some(&images[..images.len().min(4)])
        };
        match (quote, trimmed_images) {
            (Some(qr), Some(imgs)) => {
                record["embed"] = serde_json::json!({
                    "$type": "app.bsky.embed.recordWithMedia",
                    "record": {
                        "$type": "app.bsky.embed.record",
                        "record": { "uri": qr.uri, "cid": qr.cid }
                    },
                    "media": {
                        "$type": "app.bsky.embed.images",
                        "images": imgs,
                    }
                });
            }
            (Some(qr), None) => {
                record["embed"] = serde_json::json!({
                    "$type": "app.bsky.embed.record",
                    "record": { "uri": qr.uri, "cid": qr.cid },
                });
            }
            (None, Some(imgs)) => {
                record["embed"] = serde_json::json!({
                    "$type": "app.bsky.embed.images",
                    "images": imgs,
                });
            }
            (None, None) => {}
        }
        if !facets.is_empty() {
            record["facets"] = serde_json::to_value(facets)
                .map_err(|e| AtError::Decode(format!("facet serialize: {e}")))?;
        }
        let body = serde_json::json!({
            "repo": did,
            "collection": "app.bsky.feed.post",
            "record": record,
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.createRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.post_json(&url, &body).await
    }

    /// `com.atproto.identity.resolveHandle` — turn a bsky handle into
    /// its DID. Used by the rich-text pipeline to convert
    /// `@alice.bsky.social` mention candidates into the DID-bearing
    /// `mention` facet feature the lexicon expects.
    pub async fn resolve_handle(&self, handle: &str) -> Result<String, AtError> {
        let mut url = self
            .appview
            .join("/xrpc/com.atproto.identity.resolveHandle")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut().append_pair("handle", handle);
        #[derive(serde::Deserialize)]
        struct R {
            did: String,
        }
        let r: R = self.get_json(&url).await?;
        Ok(r.did)
    }

    /// Run the full rich-text detection + resolution pipeline on
    /// `text`. Returns a `Vec<Facet>` ready to pass to
    /// [`Self::create_post_full`]:
    ///
    /// - Detects @mentions / links / #tags via [`crate::richtext::detect_facet_candidates`].
    /// - Resolves each mention's handle to a DID via [`Self::resolve_handle`].
    ///   Mentions whose handles don't resolve (typo, deleted account)
    ///   are silently skipped — the literal `@text` stays in the post
    ///   body but doesn't become a clickable facet.
    /// - Dedupes handles so the same `@alice` mentioned 3× only fires
    ///   one resolveHandle call.
    pub async fn build_facets_from_text(
        &self,
        text: &str,
    ) -> Result<Vec<crate::richtext::Facet>, AtError> {
        use crate::richtext::{Facet, FacetFeature, FacetIndex, FacetKind};
        let candidates = crate::richtext::detect_facet_candidates(text);
        // Resolve unique handles in parallel — typical post has 0-2
        // mentions so this is a small fan-out.
        let mut handle_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for c in &candidates {
            if let FacetKind::Mention { handle } = &c.kind {
                handle_set.insert(handle.clone());
            }
        }
        let mut resolved: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for h in handle_set {
            // Sequential resolution — fan-out with futures::join_all
            // is fine here too but a typical post has 0-2 mentions
            // and sequential keeps the error surface simple.
            if let Ok(did) = self.resolve_handle(&h).await {
                resolved.insert(h, did);
            }
        }
        let mut out = Vec::with_capacity(candidates.len());
        for c in candidates {
            let feature = match c.kind {
                FacetKind::Mention { handle } => match resolved.get(&handle) {
                    Some(did) => FacetFeature::Mention { did: did.clone() },
                    None => continue, // unresolved → skip the facet
                },
                FacetKind::Link { uri } => FacetFeature::Link { uri },
                FacetKind::Tag { tag } => FacetFeature::Tag { tag },
            };
            out.push(Facet {
                index: FacetIndex {
                    byte_start: c.byte_start,
                    byte_end: c.byte_end,
                },
                features: vec![feature],
            });
        }
        Ok(out)
    }

    /// Upload raw image bytes to the user's PDS via
    /// `com.atproto.repo.uploadBlob`. The endpoint is unusual: body is
    /// the raw bytes (not multipart, not JSON) and Content-Type is the
    /// image's mime. The returned [`BlobRef`] is what
    /// `app.bsky.embed.images` cites.
    pub async fn upload_blob(&self, bytes: Vec<u8>, mime: &str) -> Result<BlobRef, AtError> {
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.uploadBlob")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        #[derive(Deserialize)]
        struct R {
            blob: BlobRef,
        }
        let r: R = self.post_bytes(&url, bytes, mime).await?;
        Ok(r.blob)
    }

    /// Create a like (`app.bsky.feed.like`). Returns the new record's URI so
    /// the caller can pass it back to [`Self::delete_record`] to unlike.
    pub async fn create_like(
        &self,
        post_uri: &str,
        post_cid: &str,
    ) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().did.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let body = serde_json::json!({
            "repo": did,
            "collection": "app.bsky.feed.like",
            "record": {
                "$type": "app.bsky.feed.like",
                "subject": { "uri": post_uri, "cid": post_cid },
                "createdAt": created_at,
            }
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.createRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.post_json(&url, &body).await
    }

    /// File a moderation report against an account or post via
    /// `com.atproto.moderation.createReport`. The bsky lexicon
    /// expects a `subject` (StrongRef for a post, `{ $type: ...,
    /// did }` for an account) and a `reasonType` (lexicon-defined
    /// enum string). Optional free-text `reason`.
    ///
    /// Returns `()` because we don't currently surface the report
    /// ID anywhere — the toast just confirms "report sent" and the
    /// moderation team takes it from there.
    pub async fn create_report_account(
        &self,
        subject_did: &str,
        reason_type: &str,
        reason: &str,
    ) -> Result<(), AtError> {
        let url = self
            .session_pds_url("/xrpc/com.atproto.moderation.createReport")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = serde_json::json!({
            "reasonType": reason_type,
            "reason": reason,
            "subject": {
                "$type": "com.atproto.admin.defs#repoRef",
                "did": subject_did,
            }
        });
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// File a report against a specific post (via StrongRef) rather
    /// than the whole account. Same lexicon — different subject shape.
    pub async fn create_report_post(
        &self,
        post_uri: &str,
        post_cid: &str,
        reason_type: &str,
        reason: &str,
    ) -> Result<(), AtError> {
        let url = self
            .session_pds_url("/xrpc/com.atproto.moderation.createReport")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = serde_json::json!({
            "reasonType": reason_type,
            "reason": reason,
            "subject": {
                "$type": "com.atproto.repo.strongRef",
                "uri": post_uri,
                "cid": post_cid,
            }
        });
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// Read the signed-in user's `app.bsky.actor.profile` record via
    /// `com.atproto.repo.getRecord`. Used by the profile-editor sheet
    /// to round-trip the existing fields without dropping any (the
    /// profile record may carry labels, joinedViaStarterPack, etc.
    /// that we don't want to clobber on save).
    ///
    /// Returns `(record_value, cid_for_swap)`. The CID is what
    /// `putRecord` expects as `swapRecord` so the write is atomic.
    pub async fn get_profile_record(&self) -> Result<(serde_json::Value, String), AtError> {
        let did = self.session.lock().did.clone();
        let mut url = self
            .session_pds_url("/xrpc/com.atproto.repo.getRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("repo", &did)
            .append_pair("collection", "app.bsky.actor.profile")
            .append_pair("rkey", "self");
        let v: serde_json::Value = self.get_json(&url).await?;
        let cid = v
            .get("cid")
            .and_then(|c| c.as_str())
            .ok_or_else(|| AtError::Decode("getRecord response missing cid".into()))?
            .to_string();
        let value = v.get("value").cloned().unwrap_or_else(|| serde_json::json!({}));
        Ok((value, cid))
    }

    /// Overwrite the signed-in user's `app.bsky.actor.profile` record
    /// with `new_value`, atomic against `swap_cid` so a concurrent
    /// edit from another client fails fast instead of silently
    /// clobbering. Caller is responsible for preserving unchanged
    /// fields from the prior record.
    pub async fn put_profile_record(
        &self,
        new_value: serde_json::Value,
        swap_cid: &str,
    ) -> Result<(), AtError> {
        let did = self.session.lock().did.clone();
        let body = serde_json::json!({
            "repo": did,
            "collection": "app.bsky.actor.profile",
            "rkey": "self",
            "swapRecord": swap_cid,
            "record": new_value,
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.putRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// Fetch trending topics via `app.bsky.unspecced.getTrendingTopics`.
    /// `unspecced` endpoints are bsky-AppView-internal — they're not
    /// in the public lexicon, so the shape is best-effort and may
    /// change without warning. We treat it as opt-in surface area:
    /// silent failure on the UI side, no retries.
    pub async fn get_trending_topics(&self) -> Result<crate::feed::TrendingTopicsResponse, AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.unspecced.getTrendingTopics?limit=25")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.get_json(&url).await
    }

    /// Browse popular feed generators via
    /// `app.bsky.unspecced.getPopularFeedGenerators`. Same caveats as
    /// trending topics — unspecced surface.
    pub async fn get_popular_feed_generators(
        &self,
    ) -> Result<crate::feed::FeedGeneratorsResponse, AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.unspecced.getPopularFeedGenerators?limit=30")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.get_json(&url).await
    }

    /// List the signed-in user's muted actors via
    /// `app.bsky.graph.getMutes`. Returns the first page (limit 100).
    /// The settings UI doesn't paginate yet — a single page is plenty
    /// for any reasonable mute list (the largest in practice is
    /// dozens, not thousands).
    pub async fn get_mutes(&self) -> Result<crate::feed::MutedActorsResponse, AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.graph.getMutes?limit=100")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.get_json(&url).await
    }

    /// List the signed-in user's blocked actors via
    /// `app.bsky.graph.getBlocks`. Same single-page approach as
    /// [`Self::get_mutes`].
    pub async fn get_blocks(&self) -> Result<crate::feed::BlockedActorsResponse, AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.graph.getBlocks?limit=100")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.get_json(&url).await
    }

    /// Mute an actor (`app.bsky.graph.muteActor`). Procedure call,
    /// not a createRecord — bsky tracks mutes server-side as a
    /// preference, so there's no record to later delete. Use
    /// [`Self::unmute_actor`] to reverse.
    pub async fn mute_actor(&self, did: &str) -> Result<(), AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.graph.muteActor")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = serde_json::json!({ "actor": did });
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// Symmetric with [`Self::mute_actor`].
    pub async fn unmute_actor(&self, did: &str) -> Result<(), AtError> {
        let url = self
            .session_pds_url("/xrpc/app.bsky.graph.unmuteActor")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = serde_json::json!({ "actor": did });
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// Block an actor (`app.bsky.graph.block`). Unlike mute, this IS
    /// a createRecord — blocks are public records on the user's repo
    /// (because the other party needs to see them to know not to
    /// interact). Returns the AT-URI; pass it to
    /// [`Self::delete_record`] to unblock.
    pub async fn create_block(&self, subject_did: &str) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().did.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let body = serde_json::json!({
            "repo": did,
            "collection": "app.bsky.graph.block",
            "record": {
                "$type": "app.bsky.graph.block",
                "subject": subject_did,
                "createdAt": created_at,
            }
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.createRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.post_json(&url, &body).await
    }

    /// Create a follow (`app.bsky.graph.follow`). Returns the new
    /// record's URI so the caller can pass it back to
    /// [`Self::delete_record`] to unfollow. `subject_did` is the DID
    /// of the actor being followed.
    pub async fn create_follow(&self, subject_did: &str) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().did.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let body = serde_json::json!({
            "repo": did,
            "collection": "app.bsky.graph.follow",
            "record": {
                "$type": "app.bsky.graph.follow",
                "subject": subject_did,
                "createdAt": created_at,
            }
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.createRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.post_json(&url, &body).await
    }

    /// Create a repost (`app.bsky.feed.repost`). Symmetric with [`Self::create_like`].
    pub async fn create_repost(
        &self,
        post_uri: &str,
        post_cid: &str,
    ) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().did.clone();
        let created_at = chrono::Utc::now().to_rfc3339();
        let body = serde_json::json!({
            "repo": did,
            "collection": "app.bsky.feed.repost",
            "record": {
                "$type": "app.bsky.feed.repost",
                "subject": { "uri": post_uri, "cid": post_cid },
                "createdAt": created_at,
            }
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.createRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        self.post_json(&url, &body).await
    }

    /// Delete a record by its AT-URI (`at://<did>/<collection>/<rkey>`). Used
    /// to unlike / unrepost / delete a post.
    pub async fn delete_record(&self, at_uri: &str) -> Result<(), AtError> {
        let parsed =
            parse_at_uri(at_uri).ok_or_else(|| AtError::Decode(format!("bad at-uri: {at_uri}")))?;
        let body = serde_json::json!({
            "repo": parsed.did,
            "collection": parsed.collection,
            "rkey": parsed.rkey,
        });
        let url = self
            .session_pds_url("/xrpc/com.atproto.repo.deleteRecord")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// Build a URL against the session's PDS (writes must go to the PDS,
    /// not the AppView).
    fn session_pds_url(&self, path: &str) -> Result<Url, url::ParseError> {
        let pds = self.session.lock().pds.clone();
        Url::parse(&pds)?.join(path)
    }

    /// POST raw bytes with a custom Content-Type (e.g. `image/jpeg`),
    /// DPoP-signed with the same nonce-retry loop as [`Self::post_json`].
    /// Used for `com.atproto.repo.uploadBlob` which doesn't take JSON.
    async fn post_bytes<T: DeserializeOwned>(
        &self,
        url: &Url,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<T, AtError> {
        let mut nonce = self.session.lock().dpop_nonce.clone();
        for _ in 0..2 {
            let (access, dpop_key) = {
                let s = self.session.lock();
                if s.is_expired() {
                    return Err(AtError::SessionExpired);
                }
                (s.access_token.clone(), s.dpop_key()?)
            };
            let proof =
                dpop_key.sign_proof("POST", url.as_str(), nonce.as_deref(), Some(&access))?;
            let resp = self
                .http
                .post(url.clone())
                .header("Authorization", format!("DPoP {}", access))
                .header("DPoP", proof)
                .header("Content-Type", content_type)
                .body(bytes.clone())
                .send()
                .await?;
            let status = resp.status();
            let server_nonce = resp
                .headers()
                .get("DPoP-Nonce")
                .and_then(|h| h.to_str().ok())
                .map(String::from);
            if let Some(n) = &server_nonce {
                self.session.lock().dpop_nonce = Some(n.clone());
            }
            if status.is_success() {
                let body = resp.text().await?;
                return serde_json::from_str(&body).map_err(AtError::from);
            }
            let resp_body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, status = %status, "smooblue: failed reading response body");
                    String::new()
                }
            };
            if (status == 401 || status == 400) && resp_body.contains("use_dpop_nonce") {
                if server_nonce.is_some() {
                    nonce = server_nonce;
                    continue;
                }
                return Err(AtError::MissingDpopNonce);
            }
            return Err(AtError::Status {
                status: status.as_u16(),
                body: resp_body,
            });
        }
        Err(AtError::MissingDpopNonce)
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        url: &Url,
        body: &serde_json::Value,
    ) -> Result<T, AtError> {
        let mut nonce = self.session.lock().dpop_nonce.clone();
        for _ in 0..2 {
            let (access, dpop_key) = {
                let s = self.session.lock();
                if s.is_expired() {
                    return Err(AtError::SessionExpired);
                }
                (s.access_token.clone(), s.dpop_key()?)
            };
            let proof =
                dpop_key.sign_proof("POST", url.as_str(), nonce.as_deref(), Some(&access))?;
            let resp = self
                .http
                .post(url.clone())
                .header("Authorization", format!("DPoP {}", access))
                .header("DPoP", proof)
                .json(body)
                .send()
                .await?;
            let status = resp.status();
            let server_nonce = resp
                .headers()
                .get("DPoP-Nonce")
                .and_then(|h| h.to_str().ok())
                .map(String::from);
            if let Some(n) = &server_nonce {
                self.session.lock().dpop_nonce = Some(n.clone());
            }
            if status.is_success() {
                let body = resp.text().await?;
                return serde_json::from_str(&body).map_err(AtError::from);
            }
            let resp_body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, status = %status, "smooblue: failed reading response body");
                    String::new()
                }
            };
            if (status == 401 || status == 400) && resp_body.contains("use_dpop_nonce") {
                if server_nonce.is_some() {
                    nonce = server_nonce;
                    continue;
                }
                return Err(AtError::MissingDpopNonce);
            }
            return Err(AtError::Status {
                status: status.as_u16(),
                body: resp_body,
            });
        }
        Err(AtError::MissingDpopNonce)
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &Url) -> Result<T, AtError> {
        let mut nonce = self.session.lock().dpop_nonce.clone();

        for _ in 0..2 {
            let (access, dpop_key) = {
                let s = self.session.lock();
                if s.is_expired() {
                    return Err(AtError::SessionExpired);
                }
                (s.access_token.clone(), s.dpop_key()?)
            };
            let proof =
                dpop_key.sign_proof("GET", url.as_str(), nonce.as_deref(), Some(&access))?;
            // Per RFC 9449, the Authorization scheme MUST be literally "DPoP"
            // (not "Bearer", not whatever token_type the server happened to
            // return). Some servers return token_type="Bearer" even for
            // DPoP-bound tokens; forcing the scheme here keeps us correct.
            let resp = self
                .http
                .get(url.clone())
                .header("Authorization", format!("DPoP {}", access))
                .header("DPoP", proof)
                .send()
                .await?;

            let status = resp.status();
            let server_nonce = resp
                .headers()
                .get("DPoP-Nonce")
                .and_then(|h| h.to_str().ok())
                .map(String::from);
            if let Some(n) = &server_nonce {
                self.session.lock().dpop_nonce = Some(n.clone());
            }

            if status.is_success() {
                let body = resp.text().await?;
                return serde_json::from_str(&body).map_err(AtError::from);
            }

            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, status = %status, "smooblue: failed reading response body");
                    String::new()
                }
            };
            if (status == 401 || status == 400) && body.contains("use_dpop_nonce") {
                if server_nonce.is_some() {
                    nonce = server_nonce;
                    continue;
                }
                return Err(AtError::MissingDpopNonce);
            }
            return Err(AtError::Status {
                status: status.as_u16(),
                body,
            });
        }
        Err(AtError::MissingDpopNonce)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smooblue_oauth::dpop::DpopKey;
    use std::sync::atomic::{AtomicU32, Ordering};
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn fake_session(pds: &str) -> Session {
        let k = DpopKey::generate();
        Session {
            did: "did:plc:test".into(),
            handle: "alice.bsky.test".into(),
            pds: pds.into(),
            issuer: pds.into(),
            access_token: "at-xyz".into(),
            refresh_token: "rt-xyz".into(),
            token_type: "DPoP".into(),
            expires_at: chrono::Utc::now().timestamp() + 3600,
            dpop_pem: k.to_pkcs8_pem().unwrap(),
            dpop_nonce: None,
            token_endpoint: None,
        }
    }

    #[tokio::test]
    async fn get_timeline_decodes_feed_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/app.bsky.feed.getTimeline"))
            .and(header_exists("Authorization"))
            .and(header_exists("DPoP"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "feed": [{
                    "post": {
                        "uri": "at://x", "cid": "y",
                        "author": { "did": "d", "handle": "alice.bsky.test", "displayName": "Alice" },
                        "record": { "text": "hi" }
                    }
                }],
                "cursor": "c1"
            })))
            .mount(&server)
            .await;

        let client = AtClient::new(
            fake_session(&server.uri()),
            Url::parse(&server.uri()).unwrap(),
        );
        let feed = client.get_timeline(None, 30).await.unwrap();
        assert_eq!(feed.feed.len(), 1);
        assert_eq!(feed.feed[0].post.author.handle, "alice.bsky.test");
        assert_eq!(feed.cursor.as_deref(), Some("c1"));
    }

    #[tokio::test]
    async fn retries_on_use_dpop_nonce() {
        let server = MockServer::start().await;
        let calls: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        Mock::given(method("GET"))
            .and(path("/xrpc/app.bsky.feed.getTimeline"))
            .respond_with(move |_req: &Request| {
                let n = calls_c.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    ResponseTemplate::new(401)
                        .insert_header("DPoP-Nonce", "fresh-nonce")
                        .set_body_json(serde_json::json!({ "error": "use_dpop_nonce" }))
                } else {
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "feed": [], "cursor": null }))
                }
            })
            .mount(&server)
            .await;

        let client = AtClient::new(
            fake_session(&server.uri()),
            Url::parse(&server.uri()).unwrap(),
        );
        let feed = client.get_timeline(None, 5).await.unwrap();
        assert_eq!(feed.feed.len(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "expected nonce retry");
        // Session must be mutated to remember the nonce.
        assert_eq!(client.session().dpop_nonce.as_deref(), Some("fresh-nonce"));
    }

    #[test]
    fn parse_at_uri_round_trip() {
        let p = parse_at_uri("at://did:plc:abc/app.bsky.feed.post/3kr2x").unwrap();
        assert_eq!(p.did, "did:plc:abc");
        assert_eq!(p.collection, "app.bsky.feed.post");
        assert_eq!(p.rkey, "3kr2x");
        assert!(parse_at_uri("https://example.com").is_none());
        assert!(parse_at_uri("at://did:plc:abc/app.bsky.feed.post").is_none());
        assert!(parse_at_uri("at://did:plc:abc//rkey").is_none());
    }

    #[tokio::test]
    async fn create_post_hits_pds_with_correct_body() {
        let pds = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.createRecord"))
            .and(header_exists("Authorization"))
            .and(header_exists("DPoP"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["collection"], "app.bsky.feed.post");
                assert_eq!(body["repo"], "did:plc:test");
                assert_eq!(body["record"]["text"], "hello smooblue");
                assert_eq!(body["record"]["$type"], "app.bsky.feed.post");
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "uri": "at://did:plc:test/app.bsky.feed.post/abc",
                    "cid": "bafy..."
                }))
            })
            .mount(&pds)
            .await;
        // Use a different appview URL to prove writes go to the PDS, not appview.
        let appview = MockServer::start().await;
        let client = AtClient::new(
            fake_session(&pds.uri()),
            Url::parse(&appview.uri()).unwrap(),
        );
        let rec = client.create_post("hello smooblue").await.unwrap();
        assert_eq!(rec.uri, "at://did:plc:test/app.bsky.feed.post/abc");
    }

    #[tokio::test]
    async fn upload_blob_hits_pds_with_image_mime_and_raw_body() {
        let pds = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.uploadBlob"))
            .and(header_exists("Authorization"))
            .and(header_exists("DPoP"))
            .respond_with(|req: &Request| {
                assert_eq!(
                    req.headers.get("content-type").map(|v| v.to_str().unwrap()),
                    Some("image/jpeg"),
                    "content-type must echo the image mime, not application/json"
                );
                assert!(!req.body.is_empty(), "body must be the raw image bytes");
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "blob": {
                        "$type": "blob",
                        "ref":   { "$link": "bafyJPG" },
                        "mimeType": "image/jpeg",
                        "size": req.body.len(),
                    }
                }))
            })
            .mount(&pds)
            .await;
        let appview = MockServer::start().await;
        let client = AtClient::new(
            fake_session(&pds.uri()),
            Url::parse(&appview.uri()).unwrap(),
        );
        let blob = client
            .upload_blob(vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 1, 2, 3], "image/jpeg")
            .await
            .unwrap();
        assert_eq!(blob.link.cid, "bafyJPG");
        assert_eq!(blob.mime_type, "image/jpeg");
    }

    #[tokio::test]
    async fn create_post_with_images_embeds_them() {
        let pds = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.createRecord"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let embed = &body["record"]["embed"];
                assert_eq!(embed["$type"], "app.bsky.embed.images");
                assert_eq!(embed["images"].as_array().unwrap().len(), 1);
                assert_eq!(embed["images"][0]["alt"], "a cat sitting on a keyboard");
                // Lexicon key is "image" (not "blob") — bsky AppView
                // rejected records with the wrong key as
                // `Missing required key "image"`.
                assert_eq!(embed["images"][0]["image"]["$type"], "blob");
                assert_eq!(embed["images"][0]["image"]["ref"]["$link"], "bafyJPG");
                assert_eq!(embed["images"][0]["aspectRatio"]["width"], 1600);
                assert_eq!(embed["images"][0]["aspectRatio"]["height"], 900);
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "uri": "at://did:plc:test/app.bsky.feed.post/abc",
                    "cid": "bafy..."
                }))
            })
            .mount(&pds)
            .await;
        let appview = MockServer::start().await;
        let client = AtClient::new(
            fake_session(&pds.uri()),
            Url::parse(&appview.uri()).unwrap(),
        );
        let img = PostImage {
            blob: BlobRef {
                kind: "blob".into(),
                link: BlobLink {
                    cid: "bafyJPG".into(),
                },
                mime_type: "image/jpeg".into(),
                size: 1234,
            },
            alt: "a cat sitting on a keyboard".into(),
            aspect_ratio: Some(AspectRatio {
                width: 1600,
                height: 900,
            }),
        };
        let rec = client
            .create_post_full("look at this cat", None, std::slice::from_ref(&img), &[], None)
            .await
            .unwrap();
        assert_eq!(rec.uri, "at://did:plc:test/app.bsky.feed.post/abc");
    }

    #[tokio::test]
    async fn create_post_caps_at_four_images() {
        let pds = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/xrpc/com.atproto.repo.createRecord"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let len = body["record"]["embed"]["images"].as_array().unwrap().len();
                assert_eq!(len, 4, "must trim to 4 even if caller passes more");
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "uri": "at://x/app.bsky.feed.post/y", "cid": "c"
                }))
            })
            .mount(&pds)
            .await;
        let appview = MockServer::start().await;
        let client = AtClient::new(
            fake_session(&pds.uri()),
            Url::parse(&appview.uri()).unwrap(),
        );
        let img = PostImage {
            blob: BlobRef {
                kind: "blob".into(),
                link: BlobLink { cid: "bafy".into() },
                mime_type: "image/jpeg".into(),
                size: 1,
            },
            alt: "".into(),
            aspect_ratio: None,
        };
        let many = vec![
            img.clone(),
            img.clone(),
            img.clone(),
            img.clone(),
            img.clone(),
            img,
        ];
        client
            .create_post_full("six images", None, &many, &[], None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_posts_batches_to_25_per_call() {
        let server = MockServer::start().await;
        let calls = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        Mock::given(method("GET"))
            .and(path("/xrpc/app.bsky.feed.getPosts"))
            .respond_with(move |req: &Request| {
                calls_c.fetch_add(1, Ordering::SeqCst);
                let q = req.url.query_pairs().filter(|(k, _)| k == "uris").count();
                assert!(q <= 25, "must split into ≤25 URIs per call, got {q}");
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "posts": [
                        {
                            "uri": "at://x", "cid": "c",
                            "author": { "did": "d", "handle": "alice.bsky.test" },
                            "record": { "text": "yo" }
                        }
                    ]
                }))
            })
            .mount(&server)
            .await;
        let client = AtClient::new(
            fake_session(&server.uri()),
            Url::parse(&server.uri()).unwrap(),
        );
        // 30 URIs → expect 2 calls (25 + 5).
        let uris: Vec<String> = (0..30)
            .map(|i| format!("at://did:plc:x/app.bsky.feed.post/{i}"))
            .collect();
        let out = client.get_posts(&uris).await.unwrap();
        // Two pages × 1 post each in the mock = 2 posts.
        assert_eq!(out.len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn get_posts_no_op_on_empty_input() {
        let server = MockServer::start().await;
        let client = AtClient::new(
            fake_session(&server.uri()),
            Url::parse(&server.uri()).unwrap(),
        );
        let out = client.get_posts(&[]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn get_post_thread_decodes_parent_chain_and_replies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/xrpc/app.bsky.feed.getPostThread"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "thread": {
                    "$type": "app.bsky.feed.defs#threadViewPost",
                    "post": {
                        "uri": "at://focus", "cid": "fcid",
                        "author": { "did": "df", "handle": "focus.bsky.social" },
                        "record": { "text": "focused post text" }
                    },
                    "parent": {
                        "$type": "app.bsky.feed.defs#threadViewPost",
                        "post": {
                            "uri": "at://parent", "cid": "pcid",
                            "author": { "did": "dp", "handle": "parent.bsky.social" },
                            "record": { "text": "parent text" }
                        }
                    },
                    "replies": [
                        {
                            "$type": "app.bsky.feed.defs#threadViewPost",
                            "post": {
                                "uri": "at://reply1", "cid": "r1",
                                "author": { "did": "dr", "handle": "replier.bsky.social" },
                                "record": { "text": "first reply" }
                            }
                        },
                        {
                            "$type": "app.bsky.feed.defs#notFoundPost",
                            "uri": "at://deleted"
                        }
                    ]
                }
            })))
            .mount(&server)
            .await;
        let client = AtClient::new(
            fake_session(&server.uri()),
            Url::parse(&server.uri()).unwrap(),
        );
        let thread = client.get_post_thread("at://focus", 6, 10).await.unwrap();
        let chain = thread.parent_chain();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].post().unwrap().uri, "at://parent");
        let crate::feed::ThreadView::Post { replies, .. } = &thread else {
            panic!("expected Post");
        };
        let replies = replies.as_ref().unwrap();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].post().unwrap().uri, "at://reply1");
        assert!(matches!(
            replies[1],
            crate::feed::ThreadView::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn fails_when_session_expired() {
        let server = MockServer::start().await;
        let mut s = fake_session(&server.uri());
        s.expires_at = chrono::Utc::now().timestamp() - 10;
        let client = AtClient::new(s, Url::parse(&server.uri()).unwrap());
        let err = client.get_timeline(None, 1).await.unwrap_err();
        assert!(matches!(err, AtError::SessionExpired));
    }
}
