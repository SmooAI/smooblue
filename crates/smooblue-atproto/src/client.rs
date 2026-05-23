//! XRPC client with DPoP-bound auth + nonce retry.

use crate::error::AtError;
use crate::feed::FeedResponse;
use serde::de::DeserializeOwned;
use smooblue_oauth::Session;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

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

    async fn get_json<T: DeserializeOwned>(&self, url: &Url) -> Result<T, AtError> {
        let mut nonce = self.session.lock().unwrap().dpop_nonce.clone();

        for _ in 0..2 {
            let (access, token_type, dpop_key) = {
                let s = self.session.lock().unwrap();
                if s.is_expired() {
                    return Err(AtError::SessionExpired);
                }
                (s.access_token.clone(), s.token_type.clone(), s.dpop_key()?)
            };
            let proof =
                dpop_key.sign_proof("GET", url.as_str(), nonce.as_deref(), Some(&access))?;
            let resp = self
                .http
                .get(url.clone())
                .header("Authorization", format!("{} {}", token_type, access))
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
