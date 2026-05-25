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
    ActorProfile, ActorViewerState, Embed, EmbedExternal, EmbedImage, EmbedKind, EmbedRecordView,
    FeedGeneratorView, FeedItem, Label, ListView, PostAuthor, PostRecord, PostView, SavedFeedItem,
    ThreadView,
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

/// Scale-test tiers for demo data. Controlled by
/// `SMOOBLUE_DEMO_SCALE=small|medium|large|huge|insane`. Default is
/// `small` (the realistic-looking 14-post timeline).
///
/// Used to stress-test the deck rendering / signal-subscription /
/// image-loading pipelines without needing a real test account
/// with thousands of posts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// 14 posts, 14 notifications — the curated showcase set.
    Small,
    /// 100 posts/column, 100 notifications.
    Medium,
    /// 500 posts/column, 500 notifications — exposes signal-subscription
    /// re-render cost (tick-driven timestamps).
    Large,
    /// 2000 posts/column, 2000 notifications — exposes image fan-out
    /// and Dioxus diff cost.
    Huge,
    /// 5000 posts/column, 5000 notifications — for "does it OOM?"
    /// curiosity. Not realistic but smoke-tests the worst case.
    Insane,
}

impl Scale {
    pub fn from_env() -> Self {
        match std::env::var("SMOOBLUE_DEMO_SCALE").as_deref() {
            Ok("medium") => Scale::Medium,
            Ok("large") => Scale::Large,
            Ok("huge") => Scale::Huge,
            Ok("insane") => Scale::Insane,
            _ => Scale::Small,
        }
    }

    /// How many posts each feed column should return.
    pub fn posts_per_column(self) -> usize {
        match self {
            Scale::Small => 14,
            Scale::Medium => 100,
            Scale::Large => 500,
            Scale::Huge => 2000,
            Scale::Insane => 5000,
        }
    }

    /// How many notifications to synthesize.
    pub fn notifications(self) -> usize {
        self.posts_per_column()
    }
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
        token_endpoint: None,
    }
}

/// A small, realistic-looking timeline for the Home column.
///
/// Posts are themed around smoo / Bluesky / Rust / atproto so the screenshot
/// reads as authentic. Timestamps are relative to *now* so they always render
/// as "2m" / "14m" / etc, never "yesterday".
pub fn home_feed() -> Vec<FeedItem> {
    let scale = Scale::from_env();
    let base = curated_home_feed();
    if scale == Scale::Small {
        return base;
    }
    // Scale up by repeating the curated set with unique IDs. Each
    // duplicate gets a fresh URI + slightly newer timestamp so the
    // tick-driven re-render and key-based diffing both have to do
    // real work.
    let target = scale.posts_per_column();
    let mut out = Vec::with_capacity(target);
    let now = chrono::Utc::now();
    for i in 0..target {
        let template = &base[i % base.len()];
        let mut item = template.clone();
        let ts = now - chrono::Duration::seconds((i as i64) * 7);
        item.post.uri = format!("at://did:plc:scale/app.bsky.feed.post/{i:06}");
        item.post.cid = format!("bafy-scale-{i}");
        item.post.indexed_at = Some(ts.to_rfc3339());
        item.post.record.created_at = Some(ts.to_rfc3339());
        out.push(item);
    }
    out
}

fn curated_home_feed() -> Vec<FeedItem> {
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
        // ── Rich-media exercise — one of each embed flavor so the
        // renderer's variants all get on-screen during demo screenshots.
        item_with_embed(
            "photo.bsky.social",
            "Photographer",
            Some("https://picsum.photos/seed/photog/80"),
            "Four-up grid from yesterday's walk. The 2x2 layout matches Bluesky's exactly.",
            &m(45),
            3, 7, 22,
            Embed::Known(EmbedKind::Images {
                images: vec![
                    embed_image(&img("wal1"), "Sunset over the bay"),
                    embed_image(&img("wal2"), "Mossy stone path"),
                    embed_image(&img("wal3"), "Birds on a wire"),
                    embed_image(&img("wal4"), "Coffee on a bench"),
                ],
            }),
        ),
        item_with_embed(
            "duo.bsky.social",
            "Duo",
            Some("https://picsum.photos/seed/duo/80"),
            "Side-by-side before/after — Apple Vision OCR vs the LLM scene description on the same image.",
            &m(72),
            5, 18, 84,
            Embed::Known(EmbedKind::Images {
                images: vec![
                    embed_image(&img("ocr-before"), "OCR result with the literal text"),
                    embed_image(&img("ocr-after"), "LLM description of the same image"),
                ],
            }),
        ),
        item_with_embed(
            "triptych.bsky.social",
            "Triptych",
            Some("https://picsum.photos/seed/trip/80"),
            "Three frames: tall left, two stacked on the right. Bluesky's 3-up layout in the wild.",
            &m(110),
            2, 4, 19,
            Embed::Known(EmbedKind::Images {
                images: vec![
                    embed_image(&img("frame-tall"), "Tall portrait"),
                    embed_image(&img("frame-mid"), "Mid landscape"),
                    embed_image(&img("frame-bot"), "Detail shot"),
                ],
            }),
        ),
        item_with_embed(
            "blog.bsky.social",
            "Blog",
            Some("https://picsum.photos/seed/blog/80"),
            "Wrote up the DPoP nonce loop pattern — gnarly the first time but ~30 lines once you see it.",
            &m(180),
            1, 6, 33,
            Embed::Known(EmbedKind::External {
                external: EmbedExternal {
                    uri: "https://smoo.ai/blog/atproto-dpop-rust".to_string(),
                    title: "ATproto DPoP-bound OAuth in Rust — the missing how-to".to_string(),
                    description: "Every public bsky example uses opaque tokens. Here's the DPoP-nonce retry loop in 30 lines of reqwest, plus the gotchas we hit shipping smooblue.".to_string(),
                    thumb: Some(img("ogimage")),
                },
            }),
        ),
        item_with_embed(
            "quoter.bsky.social",
            "Quoter",
            Some("https://picsum.photos/seed/quoter/80"),
            "This thread is gold:",
            &m(220),
            0, 12, 45,
            Embed::Known(EmbedKind::Record {
                record: EmbedRecordView::View {
                    uri: "at://did:plc:original/app.bsky.feed.post/q1".to_string(),
                    cid: "bafy-quoted".to_string(),
                    author: PostAuthor {
                        did: "did:plc:original".to_string(),
                        handle: "original.bsky.social".to_string(),
                        display_name: Some("Original Poster".to_string()),
                        avatar: Some("https://picsum.photos/seed/og/80".to_string()),
                    },
                    value: PostRecord {
                        text: "The thing nobody tells you about open-source desktop clients is that the build pipeline IS the product. Get cross-compilation + auto-update + signing right and you have a chance; get any one wrong and nobody installs.".to_string(),
                        created_at: Some(m(240)),
                    },
                    indexed_at: Some(m(240)),
                    embeds: Vec::new(),
                },
            }),
        ),
        item_with_embed(
            "videoer.bsky.social",
            "Videoer",
            Some("https://picsum.photos/seed/vid/80"),
            "Quick screen-cap of the alt-text auto-fill in action.",
            &m(280),
            3, 9, 41,
            Embed::Known(EmbedKind::Video {
                // Mux's public test HLS stream — small (256×144), 60s,
                // CORS-allowed, stable. Lets demo mode actually play
                // instead of poking at a fake bsky CDN URL.
                playlist: "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8".to_string(),
                thumbnail: Some(img("vid-thumb")),
                aspect_ratio: Some(smooblue_atproto::EmbedAspectRatio { width: 16, height: 9 }),
            }),
        ),
        // A post with a "graphic-media" label so the content-warning
        // interstitial renders in demo mode (tap to reveal).
        {
            let mut it = item(
                "labeler-demo.bsky.social",
                "Labeled post (demo)",
                Some("https://picsum.photos/seed/label/80"),
                "This post is marked as graphic-media — smooblue collapses it to a warning until you tap to reveal.",
                Some(&img("graphic-demo")),
                &m(320),
                0, 0, 0,
            );
            it.post.labels = vec![Label {
                src: "did:plc:moderation-demo".into(),
                uri: it.post.uri.clone(),
                cid: None,
                val: "graphic-media".into(),
                neg: false,
            }];
            it
        },
    ]
}

/// Demo notifications + a hydrated subject-post lookup. The compose
/// Notifications column expects both so each card can render the
/// post that gives the notification its context.
pub fn notifications_with_subjects() -> (
    Vec<Notification>,
    std::collections::HashMap<String, PostView>,
) {
    use std::collections::HashMap;
    let now = chrono::Utc::now();
    let m = |mins: i64| (now - chrono::Duration::minutes(mins)).to_rfc3339();

    let img = |seed: &str| format!("https://picsum.photos/seed/{seed}/600/400");

    // Three "your posts" that others engaged with — referenced by
    // multiple like/repost/quote notifications so we exercise the
    // many-likes-on-one-post case the real API hits hard.
    // The Apple-Vision post has an attached screenshot so the
    // notifications column shows that the embedded image renders
    // inside the quoted subject.
    let mut your_post_alt = synth_post("you.bsky.social", "You",
        "Shipped Apple Vision OCR + LLM scene description in smooblue compose today. Alt text is now one-click. 🎉",
        &m(15));
    your_post_alt.embed = Some(Embed::Known(EmbedKind::Images {
        images: vec![embed_image(&img("ocr-shot"), "Screenshot of smooblue compose with the AI-suggested alt text auto-filled in the textarea.")],
    }));

    let your_post_ship = synth_post("you.bsky.social", "You",
        "Made smooblue auto-fill alt text for screenshots and photos. Smoo LLM describes the scene, Apple Vision reads any text.",
        &m(60));
    let your_post_rust = synth_post("you.bsky.social", "You",
        "Dioxus 0.6 + objc2-vision = native macOS UI calling Vision.framework in a dozen lines. The objc2 family is excellent.",
        &m(180));

    // Reply / mention posts come from THEIR repo (not yours), keyed
    // by the notification.uri.
    let carol_reply = synth_post("carol.bsky.social", "Carol",
        "This is incredible — finally an alt-text workflow that doesn't feel like a chore. Are you open-sourcing?",
        &m(48));

    let devin_mention = synth_post("devinivy.com", "Devin Ivy",
        "@you the DPoP scheme handling in your atproto client is the cleanest Rust impl I've seen. Mind if I link it from the ATproto Rust thread?",
        &m(140));

    // Smoo's quote notification — their post text + an embedded
    // record of YOUR post that they quoted. Renders as: outer card
    // (their text, orange-bordered) → inner dashed-border card
    // (your quoted post). Exercises the nested-quote case end-to-end.
    let mut smoo_quote = synth_post("smoo.ai", "Smoo AI",
        "Built on top of our open observability stack — the OCR + LLM merge here is exactly the kind of agent-shaped UX we want everywhere.",
        &m(220));
    smoo_quote.embed = Some(Embed::Known(EmbedKind::Record {
        record: EmbedRecordView::View {
            uri: your_post_ship.uri.clone(),
            cid: your_post_ship.cid.clone(),
            author: your_post_ship.author.clone(),
            value: your_post_ship.record.clone(),
            indexed_at: your_post_ship.indexed_at.clone(),
            embeds: Vec::new(),
        },
    }));

    // Build the notification list.  Each item points at a specific
    // subject URI so the hydration map actually matches.
    // Build a sequence that exercises grouping: 6 consecutive likes on
    // the same post (the alt-text screenshot post) collapse into one
    // card "Alice, Dioxus and 4 others liked your post" with stacked
    // avatars. Then mix in singletons (reply / mention / quote) +
    // a 3-actor follow group.
    let items = vec![
        // ── Group: 6 likes on your_post_alt → one card with avatar stack
        notif(
            "alice.bsky.social",
            "Alice Mendez",
            Some("https://picsum.photos/seed/alice/80"),
            "like",
            &m(1),
            false,
            Some(your_post_alt.uri.clone()),
            None,
        ),
        notif(
            "dioxuslabs.com",
            "Dioxus",
            Some("https://picsum.photos/seed/dx/80"),
            "like",
            &m(3),
            false,
            Some(your_post_alt.uri.clone()),
            None,
        ),
        notif(
            "rustlang.bsky.social",
            "Rust",
            Some("https://picsum.photos/seed/rust/80"),
            "like",
            &m(5),
            false,
            Some(your_post_alt.uri.clone()),
            None,
        ),
        notif(
            "photo.bsky.social",
            "Photographer",
            Some("https://picsum.photos/seed/photog/80"),
            "like",
            &m(6),
            false,
            Some(your_post_alt.uri.clone()),
            None,
        ),
        notif(
            "duo.bsky.social",
            "Duo",
            Some("https://picsum.photos/seed/duo/80"),
            "like",
            &m(8),
            true,
            Some(your_post_alt.uri.clone()),
            None,
        ),
        notif(
            "smoo.ai",
            "Smoo AI",
            Some("https://picsum.photos/seed/smoo/80"),
            "like",
            &m(10),
            true,
            Some(your_post_alt.uri.clone()),
            None,
        ),
        // ── Group: 2 reposts of your_post_rust
        notif(
            "rustlang.bsky.social",
            "Rust",
            Some("https://picsum.photos/seed/rust/80"),
            "repost",
            &m(15),
            false,
            Some(your_post_rust.uri.clone()),
            None,
        ),
        notif(
            "dioxuslabs.com",
            "Dioxus",
            Some("https://picsum.photos/seed/dx/80"),
            "repost",
            &m(18),
            false,
            Some(your_post_rust.uri.clone()),
            None,
        ),
        // ── Singleton: reply
        notif(
            "carol.bsky.social",
            "Carol",
            Some("https://picsum.photos/seed/carol/80"),
            "reply",
            &m(48),
            true,
            Some(your_post_alt.uri.clone()),
            Some(carol_reply.uri.clone()),
        ),
        // ── Group: 3 follows
        notif(
            "bob.bsky.social",
            "Bob",
            Some("https://picsum.photos/seed/bob/80"),
            "follow",
            &m(60),
            false,
            None,
            None,
        ),
        notif(
            "triptych.bsky.social",
            "Triptych",
            Some("https://picsum.photos/seed/trip/80"),
            "follow",
            &m(65),
            false,
            None,
            None,
        ),
        notif(
            "blog.bsky.social",
            "Blog",
            Some("https://picsum.photos/seed/blog/80"),
            "follow",
            &m(70),
            true,
            None,
            None,
        ),
        // ── Singleton: mention
        notif(
            "devinivy.com",
            "Devin Ivy",
            Some("https://picsum.photos/seed/devin/80"),
            "mention",
            &m(140),
            true,
            None,
            Some(devin_mention.uri.clone()),
        ),
        // ── Singleton: quote
        notif(
            "smoo.ai",
            "Smoo AI",
            Some("https://picsum.photos/seed/smoo/80"),
            "quote",
            &m(220),
            true,
            Some(your_post_ship.uri.clone()),
            Some(smoo_quote.uri.clone()),
        ),
    ];

    let mut subjects: HashMap<String, PostView> = HashMap::new();
    for p in [
        your_post_alt,
        your_post_ship,
        your_post_rust,
        carol_reply,
        devin_mention,
        smoo_quote,
    ] {
        subjects.insert(p.uri.clone(), p);
    }
    (items, subjects)
}

fn synth_post(handle: &str, display: &str, text: &str, ts: &str) -> PostView {
    PostView {
        uri: format!(
            "at://did:plc:demo-{handle}/app.bsky.feed.post/{}",
            ts.replace(':', "-")
        ),
        cid: "bafy-demo".into(),
        author: PostAuthor {
            did: format!("did:plc:demo-{handle}"),
            handle: handle.to_string(),
            display_name: Some(display.to_string()),
            avatar: Some(format!("https://picsum.photos/seed/{handle}/80")),
        },
        record: smooblue_atproto::PostRecord {
            text: text.to_string(),
            created_at: Some(ts.to_string()),
        },
        embed: None,
        reply_count: 0,
        repost_count: 0,
        like_count: 0,
        quote_count: 0,
        indexed_at: Some(ts.to_string()),
        viewer: None,
        labels: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
// `reason_subject` — for like/repost/quote: AT-URI of YOUR post they
// engaged with. For reply: AT-URI of YOUR parent post.
// `notif_uri` — override the notification's own URI so the hydration
// lookup finds a specific synthetic post (for reply/mention/quote
// where we want to render their text).
fn notif(
    handle: &str,
    display: &str,
    avatar: Option<&str>,
    reason: &str,
    ts: &str,
    is_read: bool,
    reason_subject: Option<String>,
    notif_uri: Option<String>,
) -> Notification {
    let uri = notif_uri.unwrap_or_else(|| format!("at://did:plc:demo/{reason}/{handle}-{ts}"));
    Notification {
        uri,
        cid: "bafy-demo".into(),
        author: PostAuthor {
            did: format!("did:plc:demo-{handle}"),
            handle: handle.to_string(),
            display_name: Some(display.to_string()),
            avatar: avatar.map(String::from),
        },
        reason: reason.to_string(),
        reason_subject,
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
            quote_count: 0,
            viewer: None,
            labels: Vec::new(),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn item_with_embed(
    handle: &str,
    display: &str,
    avatar: Option<&str>,
    text: &str,
    ts: &str,
    replies: i64,
    reposts: i64,
    likes: i64,
    embed: Embed,
) -> FeedItem {
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
            embed: Some(embed),
            indexed_at: Some(ts.to_string()),
            reply_count: replies,
            repost_count: reposts,
            like_count: likes,
            quote_count: 0,
            viewer: None,
            labels: Vec::new(),
        },
    }
}

/// Synthesize a thread for the given URI. Used when the demo mode is
/// active so the ThreadSheet has real content to render without going
/// to the network. Always returns the same shape — a focused post
/// with 1 parent ancestor and 3 replies (one nested) — but uses the
/// requested URI as the focus so click-to-focus inside the tree works.
pub fn thread_for(focus_uri: &str) -> ThreadView {
    let now = chrono::Utc::now();
    let m = |mins: i64| (now - chrono::Duration::minutes(mins)).to_rfc3339();

    let root = synth_post(
        "founder.bsky.social",
        "Founder",
        "Hot take: every desktop Bluesky client should be free + native + open-source. The web client is fine but a deck experience belongs in a 30MB binary, not a browser tab.",
        &m(360),
    );
    let parent = synth_post(
        "you.bsky.social",
        "You",
        "Agreed — and DPoP-bound OAuth means we can ship one without renegotiating session security from scratch every time someone forks.",
        &m(220),
    );
    let mut focused = synth_post(
        "you.bsky.social",
        "You",
        "Just landed thread view in smooblue. Click any post → modal opens with parent chain + focused post + replies tree. Same PostCard everywhere so likes/reposts/reply all keep working inside the modal.",
        &m(15),
    );
    // Override URI so click-to-focus on this very post in the demo
    // re-uses the same thread.
    focused.uri = focus_uri.to_string();

    let reply1 = synth_post(
        "carol.bsky.social",
        "Carol",
        "Showing the parent chain is huge. Half the time I see a notification, I have no idea what it was replying to.",
        &m(10),
    );
    let reply1_child = synth_post(
        "you.bsky.social",
        "You",
        "Yep — that's exactly why we hydrated reason_subject for notifications too. Same idea, different surface.",
        &m(8),
    );
    let reply2 = synth_post(
        "rustlang.bsky.social",
        "Rust",
        "Nice. The recursive ThreadView decode with #[serde(tag)] is a clean way to handle the notFound / blocked variants without panicking on shapes.",
        &m(6),
    );
    let reply3 = synth_post(
        "dioxuslabs.com",
        "Dioxus",
        "use_resource keyed on the focused URI is exactly the pattern — clicking a reply re-fires the fetch automatically.",
        &m(3),
    );

    let make_post = |post: PostView,
                     parent: Option<Box<ThreadView>>,
                     replies: Option<Vec<ThreadView>>| ThreadView::Post {
        post,
        parent,
        replies,
    };

    let root_node = make_post(root, None, None);
    let parent_node = make_post(parent, Some(Box::new(root_node)), None);
    make_post(
        focused,
        Some(Box::new(parent_node)),
        Some(vec![
            make_post(
                reply1,
                None,
                Some(vec![make_post(reply1_child, None, None)]),
            ),
            make_post(reply2, None, None),
            make_post(reply3, None, None),
        ]),
    )
}

/// Demo: synthetic engagement data for the EngagementSheet
/// (likes/reposters/quotes). Returns the same `Loaded` variant the
/// real fetch path produces.
pub fn engagement_for(kind: &crate::state::Engagement) -> crate::components::engagement::Loaded {
    use crate::components::engagement::Loaded;
    let avatar = |seed: &str| Some(format!("https://picsum.photos/seed/{seed}/80"));
    let actor = |handle: &str, display: &str, seed: &str| PostAuthor {
        did: format!("did:plc:demo-{handle}"),
        handle: handle.to_string(),
        display_name: Some(display.to_string()),
        avatar: avatar(seed),
    };
    match kind {
        crate::state::Engagement::Likes(_) | crate::state::Engagement::Reposters(_) => {
            Loaded::Actors(vec![
                actor("alice.bsky.social", "Alice Mendez", "alice"),
                actor("rustlang.bsky.social", "Rust", "rust"),
                actor("dioxuslabs.com", "Dioxus", "dx"),
                actor("carol.bsky.social", "Carol", "carol"),
                actor("devinivy.com", "Devin Ivy", "devin"),
                actor("bob.bsky.social", "Bob", "bob"),
                actor("photo.bsky.social", "Photographer", "photog"),
                actor("smoo.ai", "Smoo AI", "smoo"),
            ])
        }
        crate::state::Engagement::Quotes(_) => {
            Loaded::Posts(home_feed().into_iter().take(3).collect())
        }
    }
}

/// Demo: a handful of suggested actors for the Suggestions column,
/// shaped like the real getSuggestions response so the
/// SuggestionRow renders end-to-end (avatar + name + handle + bio +
/// Follow button) without network.
pub fn suggestions() -> Vec<ActorProfile> {
    let make = |handle: &str, display: &str, seed: &str, bio: &str| ActorProfile {
        did: format!("did:plc:demo-suggest-{handle}"),
        handle: handle.to_string(),
        display_name: Some(display.to_string()),
        description: Some(bio.to_string()),
        avatar: Some(format!("https://picsum.photos/seed/{seed}/80")),
        banner: None,
        followers_count: Some(0),
        follows_count: Some(0),
        posts_count: Some(0),
        viewer: Some(ActorViewerState::default()),
        pinned_post: None,
    };
    vec![
        make("paul.frazee.com", "Paul Frazee", "paul",
             "ATproto core — bsky.app eng. Toolsmith."),
        make("emily.bsky.team", "Emily L", "emily",
             "Bluesky engineer working on the social graph + moderation."),
        make("rsms.me", "Rasmus Andersson", "rasmus",
             "Designer, programmer. Inter font, Figma alumni. Currently building stuff."),
        make("jay.bsky.team", "Jay Graber", "jay",
             "Bluesky CEO. Federated social since before it was cool."),
        make("dan.abramov.fyi", "Dan Abramov", "dan",
             "React, Redux. Currently independent. Trying to understand things."),
        make("simonw.bsky.social", "Simon Willison", "simon",
             "Co-creator of Django, creator of Datasette + sqlite-utils. Lots of writing about LLMs."),
        make("rustlang.bsky.social", "Rust", "rustlang",
             "The Rust Programming Language. github.com/rust-lang."),
        make("dioxuslabs.com", "Dioxus", "dioxus",
             "React-style Rust GUI. Web + desktop + mobile + LiveView from one codebase."),
    ]
}

/// Demo: a handful of saved feeds for the SavedFeedsSheet, paired
/// with their resolved generator views. Two pinned + two unpinned
/// so the picker exercises both visual states.
pub fn saved_feeds() -> Vec<(SavedFeedItem, Option<FeedGeneratorView>)> {
    let make = |uri: &str, name: &str, desc: &str, seed: &str, pinned: bool| {
        let saved = SavedFeedItem {
            kind: "feed".into(),
            value: uri.into(),
            pinned,
            id: Some(format!("demo-{seed}")),
        };
        let view = FeedGeneratorView {
            uri: uri.into(),
            cid: format!("bafy-demo-{seed}"),
            did: format!("did:plc:demo-{seed}"),
            display_name: name.into(),
            description: Some(desc.into()),
            avatar: Some(format!("https://picsum.photos/seed/{seed}/80")),
            creator: PostAuthor {
                did: format!("did:plc:demo-{seed}-creator"),
                handle: format!("{seed}.bsky.social"),
                display_name: Some(format!("{seed} creator")),
                avatar: None,
            },
            like_count: 0,
        };
        (saved, Some(view))
    };
    vec![
        make("at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.generator/whats-hot",
             "Discover",
             "The default 'what's hot' feed across Bluesky.",
             "discover", true),
        make("at://did:plc:demo/app.bsky.feed.generator/rust-makers",
             "Rust makers",
             "Posts from people building things in Rust. Curated weekly.",
             "rust-makers", true),
        make("at://did:plc:demo/app.bsky.feed.generator/indy-sports",
             "Indianapolis Sports",
             "All things Indy — Pacers, Colts, Indiana Fever, IndyCar.",
             "indy-sports", false),
        make("at://did:plc:demo/app.bsky.feed.generator/dev-news",
             "Developer news",
             "GitHub launches, language releases, conference talks.",
             "dev-news", false),
    ]
}

/// Demo: a couple of curated lists owned by the user, for the lists
/// section of the SavedFeedsSheet.
pub fn own_lists() -> Vec<ListView> {
    let creator = PostAuthor {
        did: "did:plc:demo-you".into(),
        handle: "you.bsky.social".into(),
        display_name: Some("You".into()),
        avatar: None,
    };
    let mklist = |name: &str, desc: &str, count: u64, seed: &str| ListView {
        uri: format!("at://did:plc:demo-you/app.bsky.graph.list/{seed}"),
        cid: format!("bafy-demo-list-{seed}"),
        creator: creator.clone(),
        name: name.into(),
        purpose: "app.bsky.graph.defs#curatelist".into(),
        description: Some(desc.into()),
        avatar: Some(format!("https://picsum.photos/seed/list-{seed}/80")),
        list_item_count: Some(count),
    };
    vec![
        mklist("Rust people", "Folks doing interesting Rust work.", 47, "rust"),
        mklist("Indy 500 cooks", "Indianapolis food writers + chefs I follow.", 12, "indy-cooks"),
    ]
}

/// Demo: known-followers list ("mutuals") for the profile sheet.
/// Used when SMOOBLUE_DEMO=1 — returns a handful of fake mutuals so
/// the social-proof row renders without network.
pub fn known_followers_for(_actor: &str) -> Vec<PostAuthor> {
    let avatar = |seed: &str| Some(format!("https://picsum.photos/seed/{seed}/80"));
    vec![
        PostAuthor {
            did: "did:plc:demo-alice".into(),
            handle: "alice.bsky.social".into(),
            display_name: Some("Alice Mendez".into()),
            avatar: avatar("alice"),
        },
        PostAuthor {
            did: "did:plc:demo-dx".into(),
            handle: "dioxuslabs.com".into(),
            display_name: Some("Dioxus".into()),
            avatar: avatar("dx"),
        },
        PostAuthor {
            did: "did:plc:demo-rust".into(),
            handle: "rustlang.bsky.social".into(),
            display_name: Some("Rust".into()),
            avatar: avatar("rust"),
        },
        PostAuthor {
            did: "did:plc:demo-carol".into(),
            handle: "carol.bsky.social".into(),
            display_name: Some("Carol".into()),
            avatar: avatar("carol"),
        },
        PostAuthor {
            did: "did:plc:demo-devin".into(),
            handle: "devinivy.com".into(),
            display_name: Some("Devin Ivy".into()),
            avatar: avatar("devin"),
        },
    ]
}

/// Demo: synthetic profile (ActorProfile + first page of their feed)
/// for the ProfileSheet so SMOOBLUE_DEBUG_OPEN_PROFILE=demo renders
/// the full layout without network. The handle in the focus signal
/// is used loosely — we always return the "you" profile here.
pub fn profile_for(actor: &str) -> (ActorProfile, Vec<FeedItem>) {
    let display = if actor.contains(':') || actor == "you.bsky.social" {
        ("You", "you.bsky.social", "did:plc:demo-you")
    } else {
        // Use the handle the user passed in as the display, so demo
        // also exercises the lookup-by-arbitrary-actor path.
        ("Demo Actor", actor, "did:plc:demo-other")
    };
    let profile = ActorProfile {
        did: display.2.to_string(),
        handle: display.1.to_string(),
        display_name: Some(display.0.to_string()),
        description: Some(
            "Building Smooblue — a native multi-column Bluesky client in Rust + Dioxus.\nOAuth + DPoP, single-binary, ~30MB. Open source.\n\nsmoo.ai/smooblue".to_string(),
        ),
        avatar: Some("https://picsum.photos/seed/you/200".to_string()),
        banner: Some("https://picsum.photos/seed/banner-you/1200/400".to_string()),
        followers_count: Some(2_341),
        follows_count: Some(184),
        posts_count: Some(427),
        viewer: Some(ActorViewerState {
            following: None,
            followed_by: Some("at://did:plc:demo-other/app.bsky.graph.follow/x".to_string()),
            muted: Some(false),
            blocked_by: Some(false),
            blocking: None,
        }),
        pinned_post: None,
    };
    // Re-use the same demo feed (first three posts are "yours" in the
    // home_feed timeline anyway, so the profile reads as authentic).
    let feed = home_feed();
    // Demo a pinned post by pointing at whichever URI comes back
    // first — gives the chip something concrete to render against.
    let mut profile = profile;
    if let Some(first) = feed.first() {
        profile.pinned_post = Some(smooblue_atproto::feed::PinnedPostRef {
            uri: first.post.uri.clone(),
            cid: Some(first.post.cid.clone()),
        });
    }
    (profile, feed)
}

fn embed_image(url: &str, alt: &str) -> EmbedImage {
    EmbedImage {
        thumb: url.to_string(),
        fullsize: url.to_string(),
        alt: alt.to_string(),
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
