//! Integration tests against the *real* Bluesky AppView at api.bsky.app.
//!
//! These tests don't need auth — they exercise the public read endpoints
//! that the smooblue client builds URLs / parses responses for, so any
//! shape drift (a renamed field, a new required arg) shows up here before
//! it crashes the deck UI.
//!
//! Marked `#[ignore]` so they don't run by default. To run:
//!
//!     # all real-bluesky tests
//!     cargo test --workspace -- --ignored --test-threads=1
//!
//!     # just one
//!     cargo test -p smooblue-atproto --test real_bluesky get_profile_of_bsky_app -- --ignored
//!
//! CI runs them on a nightly schedule (separate workflow) to keep them
//! out of the PR-check critical path while still catching upstream changes.

use serde_json::Value;
use std::time::Duration;

const APPVIEW: &str = "https://api.bsky.app";
/// A well-known, long-lived account used as a smoke target. If Bluesky ever
/// deletes this we'll need to update, but @bsky.app has been there since launch.
const TEST_ACTOR: &str = "bsky.app";

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("smooblue-tests/0.1 (+https://smoo.ai)")
        .timeout(Duration::from_secs(15))
        .build()
        .expect("client")
}

#[tokio::test]
#[ignore = "hits real api.bsky.app — run with `cargo test -- --ignored`"]
async fn resolve_handle_returns_a_did() {
    // `com.atproto.identity.resolveHandle` is what smooblue-oauth uses to
    // start the OAuth flow. Prove the URL + response shape work against prod.
    let url = format!("{APPVIEW}/xrpc/com.atproto.identity.resolveHandle?handle={TEST_ACTOR}");
    let resp: Value = http_client().get(&url).send().await.expect("send").json().await.expect("json");
    let did = resp["did"].as_str().expect("did field present");
    assert!(did.starts_with("did:plc:") || did.starts_with("did:web:"), "unexpected DID shape: {did}");
}

#[tokio::test]
#[ignore = "hits real api.bsky.app — run with `cargo test -- --ignored`"]
async fn get_profile_of_bsky_app() {
    // `app.bsky.actor.getProfile` — what a future Profile column will use.
    let url = format!("{APPVIEW}/xrpc/app.bsky.actor.getProfile?actor={TEST_ACTOR}");
    let resp: Value = http_client().get(&url).send().await.expect("send").json().await.expect("json");
    assert_eq!(resp["handle"].as_str(), Some(TEST_ACTOR), "handle mismatch in profile response");
    assert!(resp["did"].as_str().is_some(), "profile missing did");
}

#[tokio::test]
#[ignore = "hits real api.bsky.app — run with `cargo test -- --ignored`"]
async fn get_author_feed_decodes_into_our_types() {
    // `app.bsky.feed.getAuthorFeed` — the public-feed variant of
    // getTimeline. Critically, this *decodes the JSON into the smooblue
    // FeedResponse type* — if Bluesky renames or restructures a field
    // smooblue uses (`post.author.handle`, `post.record.text`, etc.), this
    // test fails with a serde error.
    let url = format!("{APPVIEW}/xrpc/app.bsky.feed.getAuthorFeed?actor={TEST_ACTOR}&limit=5");
    let body = http_client().get(&url).send().await.expect("send").text().await.expect("text");
    let feed: smooblue_atproto::FeedResponse = serde_json::from_str(&body).unwrap_or_else(|e| panic!("FeedResponse decode failed: {e}\nresponse body:\n{body}"));
    assert!(!feed.feed.is_empty(), "@bsky.app should have posts");
    // First post must have all the fields the PostCard renders.
    let first = &feed.feed[0].post;
    assert!(!first.uri.is_empty(), "uri missing");
    assert!(!first.cid.is_empty(), "cid missing");
    assert!(!first.author.handle.is_empty(), "handle missing");
    // display_name + relative_time should not panic on real data.
    let _ = first.display_name();
    let _ = first.relative_time();
}

#[tokio::test]
#[ignore = "hits real api.bsky.app — needs SMOOBLUE_TEST_BLUESKY_HANDLE + APP_PASSWORD env vars"]
async fn authenticated_timeline_via_app_password() {
    // Optional: when SMOOBLUE_TEST_BLUESKY_HANDLE + SMOOBLUE_TEST_BLUESKY_APP_PASSWORD
    // are set, exercise getTimeline (which DOES need auth) by minting a
    // legacy session via com.atproto.server.createSession.
    //
    // App passwords are legacy bearer auth, not OAuth/DPoP — useful for
    // an end-to-end smoke against the real timeline endpoint without
    // requiring browser interaction. The OAuth flow itself is covered by
    // the wiremock e2e tests in smooblue-oauth.
    let Ok(handle) = std::env::var("SMOOBLUE_TEST_BLUESKY_HANDLE") else {
        eprintln!("skipping: SMOOBLUE_TEST_BLUESKY_HANDLE not set");
        return;
    };
    let Ok(app_password) = std::env::var("SMOOBLUE_TEST_BLUESKY_APP_PASSWORD") else {
        eprintln!("skipping: SMOOBLUE_TEST_BLUESKY_APP_PASSWORD not set");
        return;
    };

    let http = http_client();

    // 1. Resolve handle → PDS endpoint (via api.bsky.app's resolveHandle +
    //    DID doc — but for app-password auth we just hit bsky.social directly).
    let create_session = "https://bsky.social/xrpc/com.atproto.server.createSession";
    let session_resp: Value = http
        .post(create_session)
        .json(&serde_json::json!({ "identifier": handle, "password": app_password }))
        .send()
        .await
        .expect("createSession send")
        .json()
        .await
        .expect("createSession json");
    let access_jwt = session_resp["accessJwt"].as_str().expect("createSession returned accessJwt");

    // 2. Hit getTimeline with Bearer auth.
    let url = format!("{APPVIEW}/xrpc/app.bsky.feed.getTimeline?limit=10");
    let body = http.get(&url).bearer_auth(access_jwt).send().await.expect("timeline send").text().await.expect("timeline text");
    let feed: smooblue_atproto::FeedResponse = serde_json::from_str(&body).unwrap_or_else(|e| panic!("getTimeline decode failed: {e}\nresponse body:\n{body}"));
    assert!(!feed.feed.is_empty(), "timeline should have posts for {handle}");
    // Spot-check a few fields PostCard depends on.
    for item in feed.feed.iter().take(3) {
        let _ = item.post.display_name();
        let _ = item.post.first_image_thumb();
        let _ = item.post.relative_time();
    }
}
