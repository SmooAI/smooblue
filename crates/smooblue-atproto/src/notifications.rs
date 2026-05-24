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

/// A group of notifications collapsed into one card. Matches bsky.app's
/// rendering: when 20 people like the same post, you see one row
/// ("Alice, Bob and 18 others liked your post") with stacked avatars,
/// not 20 separate rows.
///
/// Grouping rule:
/// - **like / repost** → group by `reason_subject` (same post → one
///   group); newest indexedAt wins for sort order.
/// - **follow** → group by reason alone; the subject is the viewer
///   so reason_subject is irrelevant.
/// - **reply / mention / quote** → never group — each has unique
///   content the user actually wants to read.
/// - **starterpack-joined** → group by reason; each item is one
///   joiner.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationGroup {
    /// Shared `reason` of the items in this group.
    pub reason: String,
    /// Shared `reason_subject` for grouped reasons; `None` for follows
    /// and for the rare grouped row with mixed subjects.
    pub reason_subject: Option<String>,
    /// Newest-first list of notifications in this group. Always at
    /// least one. The first item's actor is the "headline" name in
    /// "Alice, Bob and 18 others …".
    pub items: Vec<Notification>,
    /// Newest `indexedAt` across items — used for sort order across
    /// groups so a fresh like floats the group to the top.
    pub latest_at: Option<String>,
}

impl NotificationGroup {
    /// Convenience: total actor count in this group.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// `true` once at least one item in the group is unread. Used
    /// to drive the unread highlight on the card.
    pub fn any_unread(&self) -> bool {
        self.items.iter().any(|n| !n.is_read)
    }
}

/// Whether a reason should collapse multiple items into one group.
fn groupable(reason: &str) -> bool {
    matches!(reason, "like" | "repost" | "follow" | "starterpack-joined")
}

/// Group an incoming notifications page into `NotificationGroup`s
/// preserving the API's chronological order (newest first).
///
/// Algorithm:
/// 1. Iterate items in input order (newest first per AppView).
/// 2. For each item, compute a bucket key:
///    - For groupable reasons: `(reason, reason_subject)`
///    - For non-groupable reasons (reply/mention/quote): a unique
///      key per item, so each one stays a singleton group.
/// 3. Append to the existing group if the key matches; otherwise
///    start a new group. We deliberately *don't* sort across groups
///    — preserving input order keeps "newest at top" semantics.
pub fn group_notifications(items: Vec<Notification>) -> Vec<NotificationGroup> {
    let mut out: Vec<NotificationGroup> = Vec::new();
    for n in items {
        let key: Option<(String, Option<String>)> = if groupable(&n.reason) {
            Some((n.reason.clone(), n.reason_subject.clone()))
        } else {
            None
        };
        // Try to append to the most recent matching group. Only the
        // last group is a candidate — we never reorder, so a
        // matching group earlier in the list means there was an
        // ungroupable item in between that broke the run, and we
        // want to keep both groups distinct (preserves the user's
        // sense of chronology).
        if let Some(key) = key.as_ref() {
            if let Some(last) = out.last_mut() {
                if last.reason == key.0 && last.reason_subject == key.1 {
                    last.items.push(n);
                    continue;
                }
            }
        }
        out.push(NotificationGroup {
            reason: n.reason.clone(),
            reason_subject: n.reason_subject.clone(),
            latest_at: n.indexed_at.clone(),
            items: vec![n],
        });
    }
    out
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

    fn notif(reason: &str, subject: Option<&str>, handle: &str) -> Notification {
        Notification {
            uri: format!("at://{handle}/{reason}"),
            cid: "c".into(),
            author: PostAuthor {
                did: format!("did:plc:{handle}"),
                handle: handle.into(),
                display_name: None,
                avatar: None,
            },
            reason: reason.into(),
            reason_subject: subject.map(String::from),
            indexed_at: None,
            is_read: false,
        }
    }

    #[test]
    fn group_likes_on_same_post_collapse_into_one_group() {
        let items = vec![
            notif("like", Some("at://post-A"), "alice"),
            notif("like", Some("at://post-A"), "bob"),
            notif("like", Some("at://post-A"), "carol"),
        ];
        let groups = group_notifications(items);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count(), 3);
        assert_eq!(groups[0].reason, "like");
        assert_eq!(groups[0].reason_subject.as_deref(), Some("at://post-A"));
    }

    #[test]
    fn group_likes_on_different_posts_stay_separate() {
        let items = vec![
            notif("like", Some("at://post-A"), "alice"),
            notif("like", Some("at://post-B"), "bob"),
            notif("like", Some("at://post-A"), "carol"),
        ];
        let groups = group_notifications(items);
        // Three groups because the chronology was A, B, A — we don't
        // reorder, so the second A doesn't merge with the first.
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].items[0].author.handle, "alice");
        assert_eq!(groups[1].items[0].author.handle, "bob");
        assert_eq!(groups[2].items[0].author.handle, "carol");
    }

    #[test]
    fn replies_never_group_even_on_same_thread() {
        let items = vec![
            notif("reply", Some("at://post-A"), "alice"),
            notif("reply", Some("at://post-A"), "bob"),
        ];
        let groups = group_notifications(items);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].count(), 1);
        assert_eq!(groups[1].count(), 1);
    }

    #[test]
    fn follows_group_across_subjects() {
        let items = vec![
            notif("follow", None, "alice"),
            notif("follow", None, "bob"),
            notif("follow", None, "carol"),
        ];
        let groups = group_notifications(items);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count(), 3);
        assert_eq!(groups[0].reason, "follow");
    }

    #[test]
    fn mixed_stream_groups_correctly() {
        let items = vec![
            notif("like", Some("at://post-A"), "alice"),
            notif("like", Some("at://post-A"), "bob"),
            notif("reply", Some("at://post-A"), "carol"),
            notif("like", Some("at://post-A"), "dave"),  // post-reply break
            notif("repost", Some("at://post-A"), "eve"),
        ];
        let groups = group_notifications(items);
        // 4 groups: (alice+bob likes), (carol reply), (dave like), (eve repost)
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[0].count(), 2); // alice + bob
        assert_eq!(groups[0].reason, "like");
        assert_eq!(groups[1].count(), 1); // carol reply
        assert_eq!(groups[1].reason, "reply");
        assert_eq!(groups[2].count(), 1); // dave like (broke the run)
        assert_eq!(groups[3].count(), 1); // eve repost
    }

    #[test]
    fn group_any_unread_reflects_items() {
        let mut n1 = notif("like", Some("at://x"), "alice");
        n1.is_read = true;
        let mut n2 = notif("like", Some("at://x"), "bob");
        n2.is_read = false;
        let groups = group_notifications(vec![n1, n2]);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].any_unread(), "should be unread because bob is unread");
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
