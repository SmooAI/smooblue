//! OAuth client orchestration: identity → PAR → browser → callback → token exchange.

use crate::dpop::DpopKey;
use crate::error::OAuthError;
use crate::loopback::{await_callback, bind, CallbackParams};
use crate::metadata::{
    fetch_auth_server, fetch_protected_resource, resolve_identity, AuthorizationServerMetadata,
};
use crate::pkce::Pkce;
use crate::session::Session;
use serde::Deserialize;
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub struct OAuthClientConfig {
    /// `client_id` — must be the HTTPS URL of a public `client-metadata.json`.
    pub client_id: String,
    /// Default AppView used for handle resolution (typically `https://api.bsky.app`).
    pub appview: Url,
    /// Scopes requested (typically `["atproto", "transition:generic"]`).
    pub scopes: Vec<String>,
    /// Time to wait for the user to complete the browser flow.
    pub callback_timeout: Duration,
}

impl OAuthClientConfig {
    pub fn default_public() -> Self {
        Self {
            // Smoo-hosted client metadata. Until the static asset is hosted,
            // override at runtime via `smooai-config` (key: `bskyOauthClientId`).
            client_id: "https://smoo.ai/smooblue/client-metadata.json".to_string(),
            appview: Url::parse("https://api.bsky.app").unwrap(),
            scopes: vec!["atproto".into(), "transition:generic".into()],
            callback_timeout: Duration::from_secs(180),
        }
    }
}

/// High-level OAuth driver. Construct once and call [`Self::sign_in`].
pub struct OAuthClient {
    cfg: OAuthClientConfig,
    http: reqwest::Client,
}

impl OAuthClient {
    pub fn new(cfg: OAuthClientConfig) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("smooblue/0.1 (+https://smoo.ai)")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client builds");
        Self { cfg, http }
    }

    /// Inject a custom HTTP client (used in tests to point at wiremock).
    pub fn with_http(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// Run the full sign-in flow:
    /// 1. Bind a loopback port
    /// 2. Resolve handle → DID → PDS → authorization server
    /// 3. Generate PKCE + DPoP, push the authorization request
    /// 4. Open `open_url` (browser) and await the callback
    /// 5. Exchange the code for tokens
    ///
    /// `open_url` is a callback the caller wires to its preferred browser-
    /// opening mechanism (`open` crate, `webbrowser`, etc.). In tests, it
    /// can simulate the user-agent by hitting the loopback directly.
    pub async fn sign_in<F>(&self, handle: &str, open_url: F) -> Result<Session, OAuthError>
    where
        F: FnOnce(&str) -> Result<(), OAuthError> + Send,
    {
        // Step 1: loopback
        let (listener, redirect_uri) = bind().await?;

        // Step 2: identity + metadata
        let identity = resolve_identity(handle, &self.cfg.appview, &self.http).await?;
        let resource = fetch_protected_resource(&identity.pds, &self.http).await?;
        let issuer = resource
            .authorization_servers
            .first()
            .ok_or_else(|| OAuthError::Metadata("no authorization_servers in PRM".into()))?
            .clone();
        let auth = fetch_auth_server(&issuer, &self.http).await?;

        // Step 3: PKCE + DPoP + PAR
        let pkce = Pkce::generate();
        let dpop = DpopKey::generate();
        let state = uuid::Uuid::new_v4().to_string();

        // Pass the user-entered handle as login_hint (not the DID). Bluesky's
        // auth server pre-fills its handle input from this, so the user lands
        // on a login page with their handle already populated instead of
        // having to retype it.
        let par = self
            .push_authorization_request(&auth, &pkce, &dpop, &state, &redirect_uri, handle)
            .await?;

        // Step 4: browser + callback
        let auth_url = format!(
            "{}?client_id={}&request_uri={}",
            auth.authorization_endpoint,
            urlencode(&self.cfg.client_id),
            urlencode(&par.request_uri)
        );
        open_url(&auth_url)?;

        let callback: CallbackParams = await_callback(listener, self.cfg.callback_timeout).await?;
        if let Some(err) = callback.error {
            return Err(OAuthError::CallbackError(format!(
                "{}: {}",
                err,
                callback.error_description.unwrap_or_default()
            )));
        }
        if callback.state.as_deref() != Some(state.as_str()) {
            return Err(OAuthError::StateMismatch);
        }
        let code = callback
            .code
            .ok_or_else(|| OAuthError::CallbackError("missing code".into()))?;

        // Step 5: token exchange (with DPoP nonce loop)
        self.exchange_code(
            &auth,
            &dpop,
            &pkce,
            &code,
            &redirect_uri,
            &identity,
            &issuer,
        )
        .await
    }

    async fn push_authorization_request(
        &self,
        auth: &AuthorizationServerMetadata,
        pkce: &Pkce,
        dpop: &DpopKey,
        state: &str,
        redirect_uri: &str,
        login_hint: &str,
    ) -> Result<ParResponse, OAuthError> {
        let mut nonce: Option<String> = None;

        // PAR may demand DPoP-Nonce on the first attempt; loop at most twice.
        for _attempt in 0..2 {
            let proof = dpop.sign_proof(
                "POST",
                &auth.pushed_authorization_request_endpoint,
                nonce.as_deref(),
                None,
            )?;
            let form = [
                ("client_id", self.cfg.client_id.as_str()),
                ("response_type", "code"),
                ("redirect_uri", redirect_uri),
                ("code_challenge", pkce.challenge.as_str()),
                ("code_challenge_method", pkce.method()),
                ("scope", &self.cfg.scopes.join(" ")),
                ("state", state),
                ("login_hint", login_hint),
            ];

            let resp = self
                .http
                .post(&auth.pushed_authorization_request_endpoint)
                .header("DPoP", proof)
                .form(&form)
                .send()
                .await
                .map_err(|e| OAuthError::Par(format!("send: {e}")))?;

            let status = resp.status();
            let server_nonce = resp
                .headers()
                .get("DPoP-Nonce")
                .and_then(|h| h.to_str().ok())
                .map(String::from);
            let body = resp.text().await.unwrap_or_default();

            if status.is_success() {
                let parsed: ParResponse = serde_json::from_str(&body)
                    .map_err(|e| OAuthError::Par(format!("decode: {e}; body={body}")))?;
                return Ok(parsed);
            }

            // Server demanded a nonce — retry once with it.
            if status == 400 || status == 401 {
                if let Ok(err) = serde_json::from_str::<OAuthErrorResponse>(&body) {
                    if err.error == "use_dpop_nonce" {
                        if let Some(n) = server_nonce {
                            nonce = Some(n);
                            continue;
                        } else {
                            return Err(OAuthError::MissingDpopNonce);
                        }
                    }
                }
            }
            return Err(OAuthError::Par(format!("status={status} body={body}")));
        }
        Err(OAuthError::Par("exceeded PAR retries".into()))
    }

    #[allow(clippy::too_many_arguments)]
    async fn exchange_code(
        &self,
        auth: &AuthorizationServerMetadata,
        dpop: &DpopKey,
        pkce: &Pkce,
        code: &str,
        redirect_uri: &str,
        identity: &crate::metadata::ResolvedIdentity,
        issuer: &str,
    ) -> Result<Session, OAuthError> {
        let mut nonce: Option<String> = None;
        for _attempt in 0..2 {
            let proof = dpop.sign_proof("POST", &auth.token_endpoint, nonce.as_deref(), None)?;
            let form = [
                ("grant_type", "authorization_code"),
                ("client_id", self.cfg.client_id.as_str()),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", pkce.verifier.as_str()),
            ];

            let resp = self
                .http
                .post(&auth.token_endpoint)
                .header("DPoP", proof)
                .form(&form)
                .send()
                .await
                .map_err(|e| OAuthError::TokenExchange(format!("send: {e}")))?;

            let status = resp.status();
            let server_nonce = resp
                .headers()
                .get("DPoP-Nonce")
                .and_then(|h| h.to_str().ok())
                .map(String::from);
            let body = resp.text().await.unwrap_or_default();

            if status.is_success() {
                let parsed: TokenResponse = serde_json::from_str(&body)
                    .map_err(|e| OAuthError::TokenExchange(format!("decode: {e}; body={body}")))?;
                let now = chrono::Utc::now().timestamp();
                let pem = dpop.to_pkcs8_pem()?;
                return Ok(Session {
                    did: identity.did.clone(),
                    handle: String::new(),
                    pds: identity.pds.to_string(),
                    issuer: issuer.to_string(),
                    access_token: parsed.access_token,
                    refresh_token: parsed.refresh_token,
                    token_type: parsed.token_type,
                    expires_at: now + parsed.expires_in,
                    dpop_pem: pem,
                    dpop_nonce: server_nonce,
                    token_endpoint: Some(auth.token_endpoint.clone()),
                });
            }

            if status == 400 || status == 401 {
                if let Ok(err) = serde_json::from_str::<OAuthErrorResponse>(&body) {
                    if err.error == "use_dpop_nonce" {
                        if let Some(n) = server_nonce {
                            nonce = Some(n);
                            continue;
                        } else {
                            return Err(OAuthError::MissingDpopNonce);
                        }
                    }
                }
            }
            return Err(OAuthError::TokenExchange(format!(
                "status={status} body={body}"
            )));
        }
        Err(OAuthError::TokenExchange(
            "exceeded token-exchange retries".into(),
        ))
    }
}

#[derive(Deserialize, Debug)]
struct ParResponse {
    request_uri: String,
    #[allow(dead_code)]
    #[serde(default)]
    expires_in: i64,
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
    #[allow(dead_code)]
    #[serde(default)]
    error_description: Option<String>,
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
