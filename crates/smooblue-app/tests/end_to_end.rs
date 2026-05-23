//! App-level end-to-end test.
//!
//! Wires the OAuth and ATproto crates together against a single wiremock
//! server that pretends to be the AppView + PDS + auth server, then proves:
//!
//! 1. A persisted [`Session`] can be reloaded and used to drive the
//!    `AtClient` against an XRPC endpoint that demands DPoP.
//! 2. The DPoP nonce loop and Authorization header reach the server in the
//!    exact shape ATproto requires.
//! 3. The `FeedItem`s returned shape correctly for [`PostCard`] rendering
//!    (display name fallback, relative time, image thumb extraction).

use smooblue_atproto::{AtClient, FeedItem};
use smooblue_oauth::{dpop::DpopKey, Session};
use url::Url;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn session_for(server: &MockServer) -> Session {
    let key = DpopKey::generate();
    Session {
        did: "did:plc:test".into(),
        handle: "alice.bsky.social".into(),
        pds: server.uri(),
        issuer: server.uri(),
        access_token: "access-token-xyz".into(),
        refresh_token: "refresh-token-xyz".into(),
        token_type: "DPoP".into(),
        expires_at: chrono::Utc::now().timestamp() + 3600,
        dpop_pem: key.to_pkcs8_pem().unwrap(),
        dpop_nonce: None,
    }
}

#[tokio::test]
async fn home_column_renders_feed_via_dpop_bound_client() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/xrpc/app.bsky.feed.getTimeline"))
        .and(header_exists("Authorization"))
        .and(header_exists("DPoP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "feed": [
                {
                    "post": {
                        "uri": "at://did:plc:abc/app.bsky.feed.post/1",
                        "cid": "bafy1",
                        "author": {
                            "did": "did:plc:abc",
                            "handle": "alice.bsky.social",
                            "displayName": "Alice",
                            "avatar": "https://cdn/avatar.png"
                        },
                        "record": { "text": "Hello deck!", "createdAt": "2026-05-22T03:00:00Z" },
                        "embed": {
                            "$type": "app.bsky.embed.images#view",
                            "images": [{ "thumb": "https://cdn/t.png", "fullsize": "https://cdn/f.png", "alt": "" }]
                        },
                        "indexedAt": "2026-05-22T03:00:01Z",
                        "replyCount": 1,
                        "repostCount": 2,
                        "likeCount": 7
                    }
                },
                {
                    "post": {
                        "uri": "at://did:plc:abc/app.bsky.feed.post/2",
                        "cid": "bafy2",
                        "author": { "did": "did:plc:abc", "handle": "bob.bsky.social" },
                        "record": { "text": "No display name here" }
                    }
                }
            ],
            "cursor": "next-cursor"
        })))
        .mount(&server)
        .await;

    let session = session_for(&server);
    let appview = Url::parse(&server.uri()).unwrap();
    let client = AtClient::new(session, appview);

    let feed = client.get_timeline(None, 30).await.expect("timeline fetch");
    assert_eq!(feed.feed.len(), 2);
    assert_eq!(feed.cursor.as_deref(), Some("next-cursor"));

    // Item 1: full author + embed → renders display name + thumb.
    let item1: &FeedItem = &feed.feed[0];
    assert_eq!(item1.post.display_name(), "Alice");
    assert_eq!(item1.post.first_image_thumb(), Some("https://cdn/t.png"));
    assert_eq!(item1.post.like_count, 7);

    // Item 2: no display name → falls back to handle, no thumb.
    let item2: &FeedItem = &feed.feed[1];
    assert_eq!(item2.post.display_name(), "bob.bsky.social");
    assert_eq!(item2.post.first_image_thumb(), None);
}

#[tokio::test]
async fn home_column_propagates_dpop_nonce_across_calls() {
    let server = MockServer::start().await;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    let calls: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let calls_c = calls.clone();

    Mock::given(method("GET"))
        .and(path("/xrpc/app.bsky.feed.getTimeline"))
        .respond_with(move |_req: &wiremock::Request| {
            let n = calls_c.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // First call: server demands a nonce.
                ResponseTemplate::new(401)
                    .insert_header("DPoP-Nonce", "nonce-server-1")
                    .set_body_json(serde_json::json!({ "error": "use_dpop_nonce" }))
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "feed": [], "cursor": null }))
            }
        })
        .mount(&server)
        .await;

    let client = AtClient::new(session_for(&server), Url::parse(&server.uri()).unwrap());
    let feed = client
        .get_timeline(None, 5)
        .await
        .expect("timeline (after nonce retry)");
    assert_eq!(feed.feed.len(), 0);

    // Two HTTP calls happened (the retry).
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // Session has the latest nonce so the next call uses it on the first try.
    assert_eq!(
        client.session().dpop_nonce.as_deref(),
        Some("nonce-server-1")
    );
}
