//! Constellation backlink-index client.
//!
//! [Constellation](https://constellation.microcosm.blue) is a third-party,
//! unauthenticated index of atproto **backlinks** — given a target (a DID
//! or AT-URI), it answers "which records across the whole network point at
//! this?". We use it to enumerate **followers**: the set of
//! `app.bsky.graph.follow` records whose `.subject` is our DID. The
//! first-party AppView only exposes a paginated `getFollowers` that doesn't
//! carry the follow record's rkey, so we can't TID-date it; Constellation
//! returns the `rkey` per linking record, which [`crate::tid`] turns into a
//! follow-creation instant for the growth curve.
//!
//! This is deliberately a **standalone** client, not a method surface on
//! [`crate::AtClient`]: it talks to a different host, carries no auth, and
//! has its own (non-XRPC) URL shape. Egress to
//! `https://constellation.microcosm.blue` is allow-listed per ADR-002.

use crate::error::AtError;
use serde::Deserialize;
use std::time::Duration;
use url::Url;

/// Default Constellation host.
const CONSTELLATION_BASE: &str = "https://constellation.microcosm.blue";

/// One record that links *to* the queried target — i.e. for a follower
/// query, a single `app.bsky.graph.follow` record. `rkey` is a TID; feed
/// it to [`crate::tid::tid_to_datetime`] to recover when the follow was
/// created.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LinkingRecord {
    /// DID of the actor that authored the linking record (the follower).
    pub did: String,
    /// Collection NSID of the linking record (e.g. `app.bsky.graph.follow`).
    pub collection: String,
    /// Record key of the linking record — a TID for timestamped collections.
    pub rkey: String,
}

/// Response shape for `GET /links`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LinksResponse {
    /// Total number of backlinks matching the query (may exceed the page).
    pub total: u32,
    /// This page of linking records.
    #[serde(rename = "linking_records", default)]
    pub linking_records: Vec<LinkingRecord>,
    /// Opaque pagination cursor; `None` when there are no more pages.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Response shape for `GET /links/count`.
#[derive(Debug, Clone, Deserialize)]
struct LinksCountResponse {
    total: u32,
}

/// Unauthenticated client for the Constellation backlink index.
#[derive(Clone)]
pub struct ConstellationClient {
    http: reqwest::Client,
    base: Url,
}

impl Default for ConstellationClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstellationClient {
    /// Build a client against the default Constellation host.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("smooblue/0.1 (+https://smoo.ai)")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client builds");
        Self {
            http,
            base: Url::parse(CONSTELLATION_BASE).expect("constellation base URL parses"),
        }
    }

    /// Override the base URL (used by tests to point at a mock server).
    pub fn with_base(base: Url) -> Self {
        Self {
            base,
            ..Self::new()
        }
    }

    /// Swap in a custom HTTP client (mirrors [`crate::AtClient::with_http`]).
    pub fn with_http(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// `GET /links` — page through the backlinks pointing at `target`.
    ///
    /// `collection` + `path` identify which field of which record type
    /// must reference `target`. For followers:
    /// `links(my_did, "app.bsky.graph.follow", ".subject", cursor)`.
    pub async fn links(
        &self,
        target: &str,
        collection: &str,
        path: &str,
        cursor: Option<&str>,
    ) -> Result<LinksResponse, AtError> {
        let mut url = self
            .base
            .join("/links")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("target", target)
            .append_pair("collection", collection)
            .append_pair("path", path);
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json(&url).await
    }

    /// `GET /links/count` — total number of backlinks matching the query,
    /// without paging the records.
    pub async fn links_count(
        &self,
        target: &str,
        collection: &str,
        path: &str,
    ) -> Result<u32, AtError> {
        let mut url = self
            .base
            .join("/links/count")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("target", target)
            .append_pair("collection", collection)
            .append_pair("path", path);
        let r: LinksCountResponse = self.get_json(&url).await?;
        Ok(r.total)
    }

    /// Plain unauthenticated GET + JSON decode with status-aware errors.
    /// No DPoP/auth — Constellation is a public index.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &Url) -> Result<T, AtError> {
        let resp = self.http.get(url.clone()).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AtError::Status {
                status: status.as_u16(),
                body,
            });
        }
        let body = resp.text().await?;
        serde_json::from_str(&body).map_err(AtError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn links_decodes_records_and_cursor() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/links"))
            .and(query_param("target", "did:plc:me"))
            .and(query_param("collection", "app.bsky.graph.follow"))
            .and(query_param("path", ".subject"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": 2,
                "linking_records": [
                    { "did": "did:plc:a", "collection": "app.bsky.graph.follow", "rkey": "3mp5c2tkblw2p" },
                    { "did": "did:plc:b", "collection": "app.bsky.graph.follow", "rkey": "3mp5c2tkblw2q" }
                ],
                "cursor": "next"
            })))
            .mount(&server)
            .await;

        let client = ConstellationClient::with_base(Url::parse(&server.uri()).unwrap());
        let r = client
            .links("did:plc:me", "app.bsky.graph.follow", ".subject", None)
            .await
            .unwrap();
        assert_eq!(r.total, 2);
        assert_eq!(r.linking_records.len(), 2);
        assert_eq!(r.linking_records[0].did, "did:plc:a");
        assert_eq!(r.linking_records[0].rkey, "3mp5c2tkblw2p");
        assert_eq!(r.cursor.as_deref(), Some("next"));
    }

    #[tokio::test]
    async fn links_count_returns_total() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/links/count"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "total": 4321 })),
            )
            .mount(&server)
            .await;

        let client = ConstellationClient::with_base(Url::parse(&server.uri()).unwrap());
        let n = client
            .links_count("did:plc:me", "app.bsky.graph.follow", ".subject")
            .await
            .unwrap();
        assert_eq!(n, 4321);
    }

    #[tokio::test]
    async fn missing_cursor_and_records_default_cleanly() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/links"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "total": 0 })),
            )
            .mount(&server)
            .await;

        let client = ConstellationClient::with_base(Url::parse(&server.uri()).unwrap());
        let r = client
            .links("did:plc:me", "app.bsky.graph.follow", ".subject", None)
            .await
            .unwrap();
        assert_eq!(r.total, 0);
        assert!(r.linking_records.is_empty());
        assert!(r.cursor.is_none());
    }
}
