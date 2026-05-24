//! XRPC client with DPoP-bound auth + nonce retry.

use crate::error::AtError;
use crate::feed::FeedResponse;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use smooblue_oauth::Session;
use std::sync::{Arc, Mutex};
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
    Some(AtUriParts { did, collection, rkey })
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
        self.session.lock().unwrap().clone()
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
                .map(|p| crate::feed::FeedItem { post: p })
                .collect(),
        })
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
        self.create_post_with_reply(text, None).await
    }

    /// Same as [`Self::create_post`] but adds a reply context. The
    /// `root` is the top of the thread; the `parent` is the post being
    /// directly replied to (often the same for first-level replies).
    pub async fn create_post_with_reply(
        &self,
        text: &str,
        reply: Option<&ReplyRef>,
    ) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().unwrap().did.clone();
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

    /// Create a like (`app.bsky.feed.like`). Returns the new record's URI so
    /// the caller can pass it back to [`Self::delete_record`] to unlike.
    pub async fn create_like(&self, post_uri: &str, post_cid: &str) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().unwrap().did.clone();
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

    /// Create a repost (`app.bsky.feed.repost`). Symmetric with [`Self::create_like`].
    pub async fn create_repost(&self, post_uri: &str, post_cid: &str) -> Result<CreatedRecord, AtError> {
        let did = self.session.lock().unwrap().did.clone();
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
        let parsed = parse_at_uri(at_uri).ok_or_else(|| AtError::Decode(format!("bad at-uri: {at_uri}")))?;
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
        let pds = self.session.lock().unwrap().pds.clone();
        Url::parse(&pds)?.join(path)
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        url: &Url,
        body: &serde_json::Value,
    ) -> Result<T, AtError> {
        let mut nonce = self.session.lock().unwrap().dpop_nonce.clone();
        for _ in 0..2 {
            let (access, dpop_key) = {
                let s = self.session.lock().unwrap();
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
                self.session.lock().unwrap().dpop_nonce = Some(n.clone());
            }
            if status.is_success() {
                let body = resp.text().await?;
                return serde_json::from_str(&body).map_err(AtError::from);
            }
            let resp_body = resp.text().await.unwrap_or_default();
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
        let mut nonce = self.session.lock().unwrap().dpop_nonce.clone();

        for _ in 0..2 {
            let (access, dpop_key) = {
                let s = self.session.lock().unwrap();
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
                self.session.lock().unwrap().dpop_nonce = Some(n.clone());
            }

            if status.is_success() {
                let body = resp.text().await?;
                return serde_json::from_str(&body).map_err(AtError::from);
            }

            let body = resp.text().await.unwrap_or_default();
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
    async fn fails_when_session_expired() {
        let server = MockServer::start().await;
        let mut s = fake_session(&server.uri());
        s.expires_at = chrono::Utc::now().timestamp() - 10;
        let client = AtClient::new(s, Url::parse(&server.uri()).unwrap());
        let err = client.get_timeline(None, 1).await.unwrap_err();
        assert!(matches!(err, AtError::SessionExpired));
    }
}
