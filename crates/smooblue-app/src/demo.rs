//! Demo mode — synthesized data for screenshots, docs, and slowmo tours.
//!
//! Activated by setting `SMOOBLUE_DEMO=1`. When active:
//! - `state::use_bootstrap` injects a synthetic [`Session`] so the app skips
//!   straight past the login view.
//! - `Column::fetch` returns canned [`FeedItem`]s instead of calling the
//!   AppView.
//!
//! This keeps OAuth + the live network out of the loop entirely, which is
//! what we want for demos, screenshots, and UI screen-recording.

use smooblue_atproto::feed::{
    Embed, EmbedImage, EmbedKind, FeedItem, PostAuthor, PostRecord, PostView,
};
use smooblue_atproto::Notification;
use smooblue_oauth::{dpop::DpopKey, Session};

/// True when the binary was launched with `SMOOBLUE_DEMO=1`.
pub fn is_active() -> bool {
    matches!(
        std::env::var("SMOOBLUE_DEMO").as_deref(),
        Ok("1" | "true" | "yes")
    )
}

/// A throwaway session for demo mode — never used for real network calls.
pub fn fake_session() -> Session {
    let k = DpopKey::generate();
    Session {
        did: "did:plc:demo".into(),
        handle: "you.smoo.ai".into(),
        pds: "https://demo.invalid".into(),
        issuer: "https://demo.invalid".into(),
        access_token: "demo-access".into(),
        refresh_token: "demo-refresh".into(),
        token_type: "DPoP".into(),
        expires_at: chrono::Utc::now().timestamp() + 86_400,
        dpop_pem: k.to_pkcs8_pem().unwrap_or_default(),
        dpop_nonce: None,
    }
}

/// A small, realistic-looking timeline for the Home column.
///
/// Posts are themed around smoo / Bluesky / Rust / atproto so the screenshot
/// reads as authentic. Timestamps are relative to *now* so they always render
/// as "2m" / "14m" / etc, never "yesterday".
pub fn home_feed() -> Vec<FeedItem> {
    let now = chrono::Utc::now();
    let m = |mins: i64| (now - chrono::Duration::minutes(mins)).to_rfc3339();

    // Public Picsum demo images — stable, free, and used for placeholder
    // imagery elsewhere on the web. Different seeds → different photos.
    let img = |seed: &str| format!("https://picsum.photos/seed/{seed}/600/400");

    vec![
        item(
            "smoo.ai",
            "Smoo AI",
            Some("https://picsum.photos/seed/smoo/80"),
            "Just shipped Smooblue — a native multi-column Bluesky client in Rust + Dioxus. OAuth + DPoP, single-binary, ~30MB. Open source.\n\ngithub.com/SmooAI/smooblue",
            Some(&img("smooblue-deck")),
            &m(2),
            8,
            34,
            127,
        ),
        item(
            "alice.bsky.social",
            "Alice Mendez",
            Some("https://picsum.photos/seed/alice/80"),
            "morning ritual: espresso, then `cargo test --workspace` to make sure I didn't break anything yesterday ☕️",
            Some(&img("espresso")),
            &m(14),
            2,
            5,
            41,
        ),
        item(
            "rustlang.bsky.social",
            "Rust",
            Some("https://picsum.photos/seed/rust/80"),
            "Reminder: `cargo clippy --fix` will apply most lints for you. Try it on an old branch — surprising amount of dead code falls out.",
            None,
            &m(31),
            12,
            78,
            312,
        ),
        item(
            "devinivy.com",
            "Devin Ivy",
            Some("https://picsum.photos/seed/devin/80"),
            "Reading new third-party Bluesky clients popping up this week. The OAuth + DPoP path is finally getting some momentum 🎉",
            None,
            &m(58),
            6,
            22,
            89,
        ),
        item(
            "dioxuslabs.com",
            "Dioxus",
            Some("https://picsum.photos/seed/dx/80"),
            "Anyone shipping a non-trivial Dioxus desktop app this year — would love to hear what worked + what's still rough. DM open.",
            Some(&img("dioxus")),
            &m(92),
            18,
            14,
            61,
        ),
        item(
            "bob.bsky.social",
            "Bob",
            Some("https://picsum.photos/seed/bob/80"),
            "what's the smallest serious Rust GUI binary you've shipped? trying to keep mine under 25MB",
            None,
            &m(140),
            9,
            3,
            17,
        ),
        item(
            "carol.bsky.social",
            "Carol",
            Some("https://picsum.photos/seed/carol/80"),
            "TIL atproto's identity model: handle → DID → DID doc → PDS. Handle is mutable, DID is forever. Surprisingly clean.",
            Some(&img("identity")),
            &m(210),
            4,
            11,
            48,
        ),
    ]
}

/// Demo notifications for the Notifications column.
pub fn notifications() -> Vec<Notification> {
    let now = chrono::Utc::now();
    let m = |mins: i64| (now - chrono::Duration::minutes(mins)).to_rfc3339();
    vec![
        notif(
            "alice.bsky.social",
            "Alice Mendez",
            Some("https://picsum.photos/seed/alice/80"),
            "like",
            &m(1),
            false,
        ),
        notif(
            "rustlang.bsky.social",
            "Rust",
            Some("https://picsum.photos/seed/rust/80"),
            "repost",
            &m(7),
            false,
        ),
        notif(
            "bob.bsky.social",
            "Bob",
            Some("https://picsum.photos/seed/bob/80"),
            "follow",
            &m(22),
            false,
        ),
        notif(
            "carol.bsky.social",
            "Carol",
            Some("https://picsum.photos/seed/carol/80"),
            "reply",
            &m(48),
            true,
        ),
        notif(
            "dioxuslabs.com",
            "Dioxus",
            Some("https://picsum.photos/seed/dx/80"),
            "like",
            &m(95),
            true,
        ),
        notif(
            "devinivy.com",
            "Devin Ivy",
            Some("https://picsum.photos/seed/devin/80"),
            "mention",
            &m(140),
            true,
        ),
        notif(
            "smoo.ai",
            "Smoo AI",
            Some("https://picsum.photos/seed/smoo/80"),
            "quote",
            &m(220),
            true,
        ),
    ]
}

fn notif(
    handle: &str,
    display: &str,
    avatar: Option<&str>,
    reason: &str,
    ts: &str,
    is_read: bool,
) -> Notification {
    Notification {
        uri: format!("at://did:plc:demo/{reason}/{handle}-{ts}"),
        cid: "bafy-demo".into(),
        author: PostAuthor {
            did: format!("did:plc:demo-{handle}"),
            handle: handle.to_string(),
            display_name: Some(display.to_string()),
            avatar: avatar.map(String::from),
        },
        reason: reason.to_string(),
        reason_subject: Some("at://did:plc:demo/app.bsky.feed.post/sample".into()),
        indexed_at: Some(ts.to_string()),
        is_read,
    }
}

#[allow(clippy::too_many_arguments)]
fn item(
    handle: &str,
    display: &str,
    avatar: Option<&str>,
    text: &str,
    thumb: Option<&str>,
    ts: &str,
    replies: i64,
    reposts: i64,
    likes: i64,
) -> FeedItem {
    let embed = thumb.map(|url| {
        Embed::Known(EmbedKind::Images {
            images: vec![EmbedImage {
                thumb: url.to_string(),
                fullsize: url.to_string(),
                alt: String::new(),
            }],
        })
    });
    FeedItem {
        post: PostView {
            uri: format!("at://did:plc:demo/app.bsky.feed.post/{handle}-{ts}"),
            cid: "bafy-demo".into(),
            author: PostAuthor {
                did: format!("did:plc:demo-{handle}"),
                handle: handle.to_string(),
                display_name: Some(display.to_string()),
                avatar: avatar.map(String::from),
            },
            record: PostRecord {
                text: text.to_string(),
                created_at: Some(ts.to_string()),
            },
            embed,
            indexed_at: Some(ts.to_string()),
            reply_count: replies,
            repost_count: reposts,
            like_count: likes,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_feed_has_realistic_data() {
        let feed = home_feed();
        assert!(
            feed.len() >= 5,
            "demo feed should have enough posts to fill a column"
        );
        for item in &feed {
            assert!(!item.post.author.handle.is_empty());
            assert!(!item.post.record.text.is_empty());
            // Relative-time renderer shouldn't panic on demo timestamps.
            let _ = item.post.relative_time();
        }
    }

    #[test]
    fn is_active_respects_truthy_values() {
        // No env var means inactive.
        std::env::remove_var("SMOOBLUE_DEMO");
        assert!(!is_active());
        std::env::set_var("SMOOBLUE_DEMO", "1");
        assert!(is_active());
        std::env::set_var("SMOOBLUE_DEMO", "yes");
        assert!(is_active());
        std::env::set_var("SMOOBLUE_DEMO", "0");
        assert!(!is_active());
        std::env::remove_var("SMOOBLUE_DEMO");
    }
}
