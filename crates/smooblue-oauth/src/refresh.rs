//! Refresh-token flow for DPoP-bound bsky OAuth sessions.
//!
//! Sessions silently die after ~2 hours without this — the access
//! token expires, every XRPC call starts returning 401, and the user
//! gets stuck staring at an empty deck. With refresh in place, we
//! swap in a new access token in the background as it approaches
//! expiry (or reactively on a 401) and the user never notices.
//!
//! Mechanics (RFC 9449 + bsky OAuth):
//! - POST to the token endpoint with `grant_type=refresh_token` +
//!   `refresh_token=...` + `client_id=...` as form data.
//! - DPoP-sign the request with the same key bound to the session.
//! - Handle the `use_dpop_nonce` retry exactly like every other
//!   request — bsky's auth server uses the same nonce dance.
//! - On success: the response body has new access + refresh tokens
//!   (rotation), token type, and `expires_in`. The DPoP-Nonce header
//!   carries forward.
//!
//! The function takes a `&Session` and returns a fresh `Session`
//! that the caller is responsible for persisting (we don't reach
//! into the keyring here — the app-layer knows how it wants to
//! mutate its `Signal<Option<Session>>` and the OS keyring).

use crate::error::OAuthError;
use crate::metadata::fetch_auth_server;
use crate::session::Session;
use serde::Deserialize;

/// Refresh the access token. Returns a new [`Session`] with rotated
/// tokens; the input session is unchanged. Caller persists the
/// result.
///
/// Resolves the token endpoint from `session.token_endpoint` (cached
/// at sign-in time) or falls back to fetching the auth server's
/// metadata when the field is missing (older persisted sessions).
pub async fn refresh_session(
    http: &reqwest::Client,
    session: &Session,
    client_id: &str,
) -> Result<Session, OAuthError> {
    let token_endpoint = match session.token_endpoint.clone() {
        Some(t) => t,
        None => {
            let meta = fetch_auth_server(&session.issuer, http).await?;
            meta.token_endpoint
        }
    };
    let dpop_key = session.dpop_key()?;

    // Start the nonce dance with whatever was stashed on the session —
    // some servers require the nonce on the very first refresh call,
    // others issue one in the 401 response.
    let mut nonce = session.dpop_nonce.clone();
    for _attempt in 0..3 {
        // ath=hash(access_token) is omitted for the *token* endpoint —
        // RFC 9449 §4.3 reserves `ath` for resource-server requests
        // only. Pass None.
        let proof = dpop_key.sign_proof("POST", &token_endpoint, nonce.as_deref(), None)?;
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", session.refresh_token.as_str()),
            ("client_id", client_id),
        ];

        let resp = http
            .post(&token_endpoint)
            .header("DPoP", proof)
            .form(&form)
            .send()
            .await
            .map_err(|e| OAuthError::TokenExchange(format!("refresh send: {e}")))?;

        let status = resp.status();
        let server_nonce = resp
            .headers()
            .get("DPoP-Nonce")
            .and_then(|h| h.to_str().ok())
            .map(String::from);
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            let parsed: TokenResponse = serde_json::from_str(&body).map_err(|e| {
                OAuthError::TokenExchange(format!("refresh decode: {e}; body={body}"))
            })?;
            let now = chrono::Utc::now().timestamp();
            return Ok(Session {
                // Identity + key + endpoint carry forward unchanged.
                did: session.did.clone(),
                handle: session.handle.clone(),
                pds: session.pds.clone(),
                issuer: session.issuer.clone(),
                dpop_pem: session.dpop_pem.clone(),
                token_endpoint: Some(token_endpoint.clone()),
                // Rotated bits from the response.
                access_token: parsed.access_token,
                refresh_token: parsed.refresh_token,
                token_type: parsed.token_type,
                expires_at: now + parsed.expires_in,
                dpop_nonce: server_nonce.or(nonce),
            });
        }

        if status == 400 || status == 401 {
            if let Ok(err) = serde_json::from_str::<OAuthErrorResponse>(&body) {
                if err.error == "use_dpop_nonce" {
                    if let Some(n) = server_nonce {
                        nonce = Some(n);
                        continue;
                    }
                    return Err(OAuthError::MissingDpopNonce);
                }
                // invalid_grant means the refresh token is dead — the
                // user has to re-auth. Surface that distinctly so the
                // caller can drop the session and route back to login.
                if err.error == "invalid_grant" {
                    return Err(OAuthError::TokenExchange(format!(
                        "refresh rejected (invalid_grant) — re-auth required: {}",
                        err.error_description.unwrap_or_default()
                    )));
                }
            }
        }
        return Err(OAuthError::TokenExchange(format!(
            "refresh failed: status={status} body={body}"
        )));
    }
    Err(OAuthError::TokenExchange(
        "exceeded refresh-token retries".into(),
    ))
}

#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: i64,
}

#[derive(Deserialize, Debug)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dpop::DpopKey;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn fake_session(token_endpoint: &str) -> Session {
        let k = DpopKey::generate();
        Session {
            did: "did:plc:test".into(),
            handle: "alice.bsky.test".into(),
            pds: "https://pds.invalid".into(),
            issuer: "https://issuer.invalid".into(),
            access_token: "old-at".into(),
            refresh_token: "rt-original".into(),
            token_type: "DPoP".into(),
            expires_at: chrono::Utc::now().timestamp() - 10, // already expired
            dpop_pem: k.to_pkcs8_pem().unwrap(),
            dpop_nonce: Some("nonce-from-prior-resource-call".into()),
            token_endpoint: Some(token_endpoint.to_string()),
        }
    }

    #[tokio::test]
    async fn refresh_rotates_tokens_and_keeps_identity() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(header_exists("DPoP"))
            .respond_with(|req: &Request| {
                let body = std::str::from_utf8(&req.body).unwrap();
                assert!(body.contains("grant_type=refresh_token"));
                assert!(body.contains("refresh_token=rt-original"));
                assert!(body.contains("client_id=https"));
                ResponseTemplate::new(200)
                    .insert_header("DPoP-Nonce", "fresh-nonce")
                    .set_body_json(serde_json::json!({
                        "access_token": "new-at",
                        "refresh_token": "rt-rotated",
                        "token_type": "DPoP",
                        "expires_in": 3600
                    }))
            })
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let session = fake_session(&format!("{}/oauth/token", server.uri()));
        let new_session = refresh_session(
            &http,
            &session,
            "https://smoo.ai/smooblue/client-metadata.json",
        )
        .await
        .unwrap();

        // Rotated bits.
        assert_eq!(new_session.access_token, "new-at");
        assert_eq!(new_session.refresh_token, "rt-rotated");
        assert_eq!(new_session.dpop_nonce.as_deref(), Some("fresh-nonce"));
        assert!(new_session.expires_at > chrono::Utc::now().timestamp());
        // Identity + key + endpoint carry forward.
        assert_eq!(new_session.did, session.did);
        assert_eq!(new_session.handle, session.handle);
        assert_eq!(new_session.pds, session.pds);
        assert_eq!(new_session.dpop_pem, session.dpop_pem);
        assert_eq!(new_session.token_endpoint, session.token_endpoint);
    }

    #[tokio::test]
    async fn refresh_retries_on_use_dpop_nonce() {
        let server = MockServer::start().await;
        let calls: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let calls_c = calls.clone();
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(move |_req: &Request| {
                let n = calls_c.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    ResponseTemplate::new(401)
                        .insert_header("DPoP-Nonce", "server-nonce")
                        .set_body_json(serde_json::json!({ "error": "use_dpop_nonce" }))
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("DPoP-Nonce", "post-success-nonce")
                        .set_body_json(serde_json::json!({
                            "access_token": "at2", "refresh_token": "rt2",
                            "token_type": "DPoP", "expires_in": 3600
                        }))
                }
            })
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let session = fake_session(&format!("{}/oauth/token", server.uri()));
        let new_session = refresh_session(&http, &session, "cid").await.unwrap();
        assert_eq!(new_session.access_token, "at2");
        assert_eq!(calls.load(Ordering::SeqCst), 2, "expected nonce retry");
    }

    #[tokio::test]
    async fn refresh_surfaces_invalid_grant_distinctly() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "refresh token expired"
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let session = fake_session(&format!("{}/oauth/token", server.uri()));
        let err = refresh_session(&http, &session, "cid").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid_grant"), "error should mention invalid_grant: {msg}");
        assert!(msg.contains("re-auth"), "should signal re-auth required: {msg}");
    }
}
