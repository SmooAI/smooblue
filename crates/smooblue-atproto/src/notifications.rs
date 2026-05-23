//! Bluesky notifications — `app.bsky.notification.listNotifications`.

use crate::feed::PostAuthor;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationsResponse {
    #[serde(default)]
    pub notifications: Vec<Notification>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Notification {
    pub uri: String,
    pub cid: String,
    pub author: PostAuthor,
    /// One of: `"like"`, `"repost"`, `"follow"`, `"mention"`, `"reply"`, `"quote"`, `"starterpack-joined"`.
    pub reason: String,
    /// AT-URI of the subject (post the notification refers to), if applicable.
    #[serde(rename = "reasonSubject", default)]
    pub reason_subject: Option<String>,
    #[serde(rename = "indexedAt", default)]
    pub indexed_at: Option<String>,
    /// True until the user marks it read.
    #[serde(rename = "isRead", default)]
    pub is_read: bool,
}

impl Notification {
    /// Compact relative time ("2m", "1h"), matching `PostView::relative_time`.
    pub fn relative_time(&self) -> String {
        let Some(s) = &self.indexed_at else {
            return String::new();
        };
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

    /// Display verb for the notification reason.
    pub fn reason_phrase(&self) -> &'static str {
        match self.reason.as_str() {
            "like" => "liked your post",
            "repost" => "reposted your post",
            "follow" => "followed you",
            "mention" => "mentioned you",
            "reply" => "replied to your post",
            "quote" => "quoted your post",
            "starterpack-joined" => "joined via your starter pack",
            _ => "interacted with you",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_notifications_response() {
        let body = serde_json::json!({
            "notifications": [
                {
                    "uri": "at://did:plc:abc/app.bsky.feed.like/1",
                    "cid": "bafy1",
                    "author": { "did": "did:plc:abc", "handle": "alice.bsky.social", "displayName": "Alice" },
                    "reason": "like",
                    "reasonSubject": "at://did:plc:me/app.bsky.feed.post/1",
                    "indexedAt": chrono::Utc::now().to_rfc3339(),
                    "isRead": false
                }
            ],
            "cursor": "next"
        });
        let parsed: NotificationsResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.notifications.len(), 1);
        assert_eq!(parsed.notifications[0].reason_phrase(), "liked your post");
        assert!(!parsed.notifications[0].is_read);
    }

    #[test]
    fn reason_phrase_covers_known_reasons() {
        for (reason, expected) in [
            ("like", "liked your post"),
            ("repost", "reposted your post"),
            ("follow", "followed you"),
            ("mention", "mentioned you"),
            ("reply", "replied to your post"),
            ("quote", "quoted your post"),
            ("starterpack-joined", "joined via your starter pack"),
            ("unknown-future-reason", "interacted with you"),
        ] {
            let n = Notification {
                uri: "x".into(),
                cid: "y".into(),
                author: PostAuthor {
                    did: "d".into(),
                    handle: "h".into(),
                    display_name: None,
                    avatar: None,
                },
                reason: reason.to_string(),
                reason_subject: None,
                indexed_at: None,
                is_read: false,
            };
            assert_eq!(n.reason_phrase(), expected, "reason={reason}");
        }
    }
}
