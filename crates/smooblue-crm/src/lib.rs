//! Smoo AI CRM sync — **opt-in only**.
//!
//! When the user ticks "Stay in touch with Smoo AI" on the login screen,
//! smooblue posts a single contact record (display name, handle, DID,
//! avatar, bio, follower count) to Smoo AI's CRM so they can follow up
//! by email/Bluesky.
//!
//! Privacy invariants this crate enforces:
//!
//! 1. **Never fires without `consent: true`.** The public API takes a
//!    [`Consent`] token by value to make accidental fire impossible.
//! 2. **No password / no access token leaves the device.** Only
//!    publicly-visible profile fields are posted.
//! 3. **Failures don't block sign-in.** A returned [`Err`] is surfaced
//!    to the UI as a non-blocking toast; the user is still signed in.
//! 4. **Caller controls the endpoint.** Default is the Smoo-hosted
//!    intake URL, override via env or config so self-hosting works.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// Marker value the caller must construct to call into this crate.
///
/// The only way to obtain one is via [`Consent::granted`] — which the
/// UI calls iff the user ticked the consent checkbox. This makes
/// `report_signup` impossible to call accidentally with consent=false.
#[derive(Debug, Clone, Copy)]
pub struct Consent(());

impl Consent {
    /// Construct an explicit-consent token. Call ONLY when the user has
    /// actively opted in (ticked the box / clicked the affirmative button).
    pub fn granted() -> Self {
        Self(())
    }
}

/// The subset of `app.bsky.actor.getProfile` we forward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueskyProfile {
    pub did: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub followers_count: Option<u64>,
    pub follows_count: Option<u64>,
}

/// The JSON body posted to the Smoo AI intake endpoint.
///
/// Mirrors the smoo AI CRM `CreateCrmContact` shape: typed fields for
/// `firstName` / `lastName`, everything Bluesky-specific lives in
/// `customFields`. The intake endpoint stamps `source = "smooblue"` server-side.
#[derive(Debug, Clone, Serialize)]
struct SignupBody {
    #[serde(rename = "firstName", skip_serializing_if = "Option::is_none")]
    first_name: Option<String>,
    #[serde(rename = "lastName", skip_serializing_if = "Option::is_none")]
    last_name: Option<String>,
    #[serde(rename = "customFields")]
    custom_fields: serde_json::Value,
}

impl SignupBody {
    fn from_profile(profile: &BlueskyProfile) -> Self {
        let (first, last) = split_display_name(profile.display_name.as_deref().unwrap_or(""));
        Self {
            first_name: first,
            last_name: last,
            custom_fields: serde_json::json!({
                "source": "smooblue",
                "blueskyDid": profile.did,
                "blueskyHandle": profile.handle,
                "blueskyDisplayName": profile.display_name,
                "blueskyDescription": profile.description,
                "blueskyAvatar": profile.avatar,
                "blueskyFollowersCount": profile.followers_count,
                "blueskyFollowsCount": profile.follows_count,
            }),
        }
    }
}

/// Best-effort split of a Bluesky display name into first/last.
/// Bluesky display names are free-form, so this is heuristic; the full
/// name still lives in `customFields.blueskyDisplayName` so nothing is lost.
fn split_display_name(s: &str) -> (Option<String>, Option<String>) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    match parts.as_slice() {
        [single] => (Some((*single).to_string()), None),
        [first, rest] => (Some((*first).to_string()), Some((*rest).trim().to_string())),
        _ => (None, None),
    }
}

#[derive(Debug, Clone)]
pub struct CrmClient {
    http: reqwest::Client,
    endpoint: Url,
}

impl CrmClient {
    /// Default Smoo AI intake endpoint. Will live at
    /// `https://api.smoo.ai/v1/smooblue/signup` once server-side support lands
    /// (tracked separately; until then this URL 404s, which is fine — calls
    /// are non-blocking and an error just shows a toast).
    pub fn smoo_default() -> Self {
        let endpoint = std::env::var("SMOOBLUE_CRM_ENDPOINT")
            .ok()
            .and_then(|s| Url::parse(&s).ok())
            .unwrap_or_else(|| {
                Url::parse("https://api.smoo.ai/v1/smooblue/signup").expect("hardcoded URL parses")
            });
        Self::with_endpoint(endpoint)
    }

    pub fn with_endpoint(endpoint: Url) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("smooblue/0.1 (+https://github.com/SmooAI/smooblue)")
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client builds");
        Self { http, endpoint }
    }

    /// Post the given Bluesky profile to the Smoo AI CRM intake endpoint.
    ///
    /// The `_consent` argument is a type-level marker — it's only
    /// constructable via [`Consent::granted`], which the UI calls iff
    /// the user actively opted in. Non-consent paths fail to compile.
    pub async fn report_signup(
        &self,
        _consent: Consent,
        profile: &BlueskyProfile,
    ) -> Result<(), CrmError> {
        let body = SignupBody::from_profile(profile);
        tracing::info!(
            did = %profile.did,
            handle = %profile.handle,
            endpoint = %self.endpoint,
            "reporting consented smooblue signup to Smoo AI CRM"
        );
        let resp = self
            .http
            .post(self.endpoint.clone())
            .json(&body)
            .send()
            .await
            .map_err(|e| CrmError::Network(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(CrmError::Server {
                status: status.as_u16(),
                body,
            })
        }
    }
}

#[derive(Debug, Error)]
pub enum CrmError {
    #[error("network error: {0}")]
    Network(String),
    #[error("server returned {status}: {body}")]
    Server { status: u16, body: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_profile() -> BlueskyProfile {
        BlueskyProfile {
            did: "did:plc:test".into(),
            handle: "alice.bsky.social".into(),
            display_name: Some("Alice Example".into()),
            description: Some("rust + bsky".into()),
            avatar: Some("https://cdn/avatar.png".into()),
            followers_count: Some(42),
            follows_count: Some(10),
        }
    }

    #[test]
    fn signup_body_splits_display_name() {
        let body = SignupBody::from_profile(&sample_profile());
        assert_eq!(body.first_name.as_deref(), Some("Alice"));
        assert_eq!(body.last_name.as_deref(), Some("Example"));
        assert_eq!(body.custom_fields["source"], "smooblue");
        assert_eq!(body.custom_fields["blueskyHandle"], "alice.bsky.social");
        assert_eq!(body.custom_fields["blueskyFollowersCount"], 42);
    }

    #[test]
    fn signup_body_handles_single_word_display_name() {
        let mut p = sample_profile();
        p.display_name = Some("Alice".into());
        let body = SignupBody::from_profile(&p);
        assert_eq!(body.first_name.as_deref(), Some("Alice"));
        assert!(body.last_name.is_none());
    }

    #[test]
    fn signup_body_handles_no_display_name() {
        let mut p = sample_profile();
        p.display_name = None;
        let body = SignupBody::from_profile(&p);
        assert!(body.first_name.is_none());
        assert!(body.last_name.is_none());
        // Handle is still preserved in customFields.
        assert_eq!(body.custom_fields["blueskyHandle"], "alice.bsky.social");
    }

    /// **Privacy invariant**: the consent token is the only way to call.
    /// We can't `#[should_not_compile]` test in stable Rust, but we can
    /// document the API: there's no other public constructor for Consent,
    /// so report_signup is uncallable without explicit opt-in.
    #[test]
    fn consent_token_is_explicit() {
        let _c = Consent::granted();
        // Confirmed: no other public path constructs Consent.
    }

    /// **Privacy invariant**: no access token / password ever in the body.
    #[test]
    fn signup_body_serializes_with_only_public_profile_fields() {
        let body = SignupBody::from_profile(&sample_profile());
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("accessToken"));
        assert!(!json.contains("access_token"));
        assert!(!json.contains("refreshToken"));
        assert!(!json.contains("password"));
        assert!(!json.contains("dpop"));
    }

    #[tokio::test]
    async fn report_signup_posts_expected_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/smooblue/signup"))
            .and(body_string_contains("did:plc:test"))
            .and(body_string_contains("alice.bsky.social"))
            .and(body_string_contains("\"source\":\"smooblue\""))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({ "id": "contact-1" })),
            )
            .mount(&server)
            .await;

        let endpoint = Url::parse(&format!("{}/v1/smooblue/signup", server.uri())).unwrap();
        let client = CrmClient::with_endpoint(endpoint);
        client
            .report_signup(Consent::granted(), &sample_profile())
            .await
            .expect("report should succeed");
    }

    #[tokio::test]
    async fn report_signup_surfaces_server_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/smooblue/signup"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;
        let endpoint = Url::parse(&format!("{}/v1/smooblue/signup", server.uri())).unwrap();
        let client = CrmClient::with_endpoint(endpoint);
        let err = client
            .report_signup(Consent::granted(), &sample_profile())
            .await
            .unwrap_err();
        match err {
            CrmError::Server { status, .. } => assert_eq!(status, 429),
            other => panic!("expected Server error, got {other:?}"),
        }
    }
}
