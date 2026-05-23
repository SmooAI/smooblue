//! Bluesky feed types — subset needed to render a deck column.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct FeedResponse {
    #[serde(default)]
    pub feed: Vec<FeedItem>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedItem {
    pub post: PostView,
}

/// `app.bsky.actor.defs#profileViewDetailed` — full profile shape.
#[derive(Debug, Clone, Deserialize)]
pub struct ActorProfile {
    pub did: String,
    pub handle: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(rename = "followersCount", default)]
    pub followers_count: Option<u64>,
    #[serde(rename = "followsCount", default)]
    pub follows_count: Option<u64>,
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
    Record(serde_json::Value),
    #[serde(rename = "app.bsky.embed.recordWithMedia#view")]
    RecordWithMedia(serde_json::Value),
    #[serde(rename = "app.bsky.embed.video#view")]
    Video {
        playlist: String,
        thumbnail: Option<String>,
    },
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
