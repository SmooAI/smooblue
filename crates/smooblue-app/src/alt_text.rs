//! AI-suggested alt-text for compose attachments.
//!
//! Two providers are planned, defined behind a small trait so the
//! compose sheet can ask either (or both, and merge):
//!
//! - **Smoo LLM vision** — describes the *scene* ("a black cat sitting
//!   on a laptop keyboard"). Cross-platform; needs network. Default
//!   endpoint is `api.smoo.ai`'s vision-describe route, overridable
//!   via `SMOOBLUE_VISION_ENDPOINT` for local server testing.
//!
//! - **Apple Vision OCR** (macOS only, in a follow-up module) —
//!   extracts the *literal text* visible in the image. Complementary
//!   to the LLM: screenshots and memes get accurate transcription
//!   without burning an LLM call, while photos get the LLM treatment.
//!
//! When both succeed the compose sheet stitches them as
//! `"{description}. Text reads: \"{ocr_text}\""`.
//!
//! Failures are silent — the alt field just stays empty and the user
//! types their own. Network down, endpoint 5xx, model timeout: none of
//! that should ever block a post going out.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use url::Url;

/// Default LLM vision endpoint. Overridable via the
/// `SMOOBLUE_VISION_ENDPOINT` env var (useful for local dev against a
/// staging api). smoo.ai doesn't version under `/v1/` — features
/// namespace by product + kebab-cased verb, matching existing routes
/// like `/booking/google/...` and `/agents/{id}/regenerate-prompts`.
pub const DEFAULT_ENDPOINT: &str = "https://api.smoo.ai/smooblue/describe-image";

/// A single AI-suggested alt text plus a confidence hint, so the UI
/// can dim very-low-confidence suggestions or hide them entirely.
#[derive(Clone, Debug, PartialEq)]
pub struct AltSuggestion {
    pub text: String,
    /// 0.0..=1.0 — provider's confidence. We don't currently filter on
    /// this in the compose UI but it'd let us hide hallucinated
    /// blank-image descriptions later.
    pub confidence: f32,
    /// Which provider produced this — purely for telemetry / icons.
    pub source: AltSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltSource {
    SmooLlm,
    AppleVision,
}

#[async_trait]
pub trait AltTextProvider: Send + Sync {
    /// Generate a screen-reader alt-text from the raw image bytes +
    /// mime. Implementations should keep total latency under a few
    /// seconds; the compose sheet times out anything longer.
    async fn describe(&self, image: &[u8], mime: &str) -> Result<AltSuggestion>;
}

/// Smoo's hosted vision endpoint. Posts the raw image bytes as
/// `multipart/form-data` and expects:
///
/// ```json
/// { "description": "…", "confidence": 0.92 }
/// ```
pub struct SmooLlmAltText {
    endpoint: Url,
    http: reqwest::Client,
    auth_bearer: Option<String>,
}

impl SmooLlmAltText {
    pub fn new(endpoint: Url) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("smooblue/0.1 (+https://smoo.ai)")
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client builds");
        Self { endpoint, http, auth_bearer: None }
    }

    pub fn with_bearer(mut self, token: impl Into<String>) -> Self {
        self.auth_bearer = Some(token.into());
        self
    }

    /// Resolve endpoint from `SMOOBLUE_VISION_ENDPOINT`, falling back
    /// to [`DEFAULT_ENDPOINT`]. Returns `None` if the env value is
    /// present but doesn't parse — caller decides what to do.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("SMOOBLUE_VISION_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
        let url = Url::parse(&raw).ok()?;
        Some(Self::new(url))
    }
}

#[derive(Deserialize)]
struct DescribeResponse {
    description: String,
    #[serde(default)]
    confidence: Option<f32>,
}

#[async_trait]
impl AltTextProvider for SmooLlmAltText {
    async fn describe(&self, image: &[u8], mime: &str) -> Result<AltSuggestion> {
        let part = reqwest::multipart::Part::bytes(image.to_vec())
            .mime_str(mime)
            .context("bad image mime")?
            .file_name("image");
        let form = reqwest::multipart::Form::new().part("image", part);

        let mut req = self.http.post(self.endpoint.clone()).multipart(form);
        if let Some(tok) = &self.auth_bearer {
            req = req.bearer_auth(tok);
        }
        let resp = req.send().await.context("vision endpoint request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("vision endpoint {status}: {body}"));
        }
        let parsed: DescribeResponse = resp.json().await.context("decoding describe response")?;
        let trimmed = parsed.description.trim().to_string();
        if trimmed.is_empty() {
            return Err(anyhow!("vision endpoint returned empty description"));
        }
        Ok(AltSuggestion {
            text: trimmed,
            confidence: parsed.confidence.unwrap_or(0.5),
            source: AltSource::SmooLlm,
        })
    }
}

/// Combine an LLM scene description with optional OCR text into a
/// single alt string. Either input may be empty/None — output is the
/// best of what's available.
pub fn merge_descriptions(llm: Option<&str>, ocr: Option<&str>) -> String {
    let llm = llm.map(str::trim).filter(|s| !s.is_empty());
    let ocr = ocr.map(str::trim).filter(|s| !s.is_empty());
    match (llm, ocr) {
        (Some(l), Some(o)) => format!("{l} Text reads: \"{o}\""),
        (Some(l), None) => l.to_string(),
        (None, Some(o)) => format!("Text reads: \"{o}\""),
        (None, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn merge_descriptions_handles_all_combinations() {
        assert_eq!(merge_descriptions(None, None), "");
        assert_eq!(
            merge_descriptions(Some("a cat on a keyboard"), None),
            "a cat on a keyboard"
        );
        assert_eq!(
            merge_descriptions(None, Some("404 not found")),
            "Text reads: \"404 not found\""
        );
        assert_eq!(
            merge_descriptions(Some("a meme template"), Some("WHEN YOU SEE IT")),
            "a meme template Text reads: \"WHEN YOU SEE IT\""
        );
        assert_eq!(merge_descriptions(Some("  "), Some("x")), "Text reads: \"x\"");
    }

    #[tokio::test]
    async fn smoo_llm_calls_endpoint_and_parses_description() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/smooblue/describe-image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "description": "A black cat sitting on a laptop keyboard.",
                "confidence": 0.87
            })))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/smooblue/describe-image", server.uri())).unwrap();
        let provider = SmooLlmAltText::new(url);
        let suggestion = provider.describe(&[0xFF, 0xD8, 0xFF, 0xE0], "image/jpeg").await.unwrap();
        assert_eq!(suggestion.text, "A black cat sitting on a laptop keyboard.");
        assert!((suggestion.confidence - 0.87).abs() < 0.001);
        assert_eq!(suggestion.source, AltSource::SmooLlm);
    }

    #[tokio::test]
    async fn smoo_llm_forwards_bearer_when_set() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/smooblue/describe-image"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "description": "x", "confidence": 1.0
            })))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/smooblue/describe-image", server.uri())).unwrap();
        let provider = SmooLlmAltText::new(url).with_bearer("secret-token");
        provider.describe(&[1], "image/jpeg").await.unwrap();
    }

    #[tokio::test]
    async fn smoo_llm_returns_error_on_5xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/smooblue/describe-image"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/smooblue/describe-image", server.uri())).unwrap();
        let provider = SmooLlmAltText::new(url);
        let err = provider.describe(&[1], "image/jpeg").await.unwrap_err();
        assert!(err.to_string().contains("503"));
    }

    #[tokio::test]
    async fn smoo_llm_rejects_empty_description() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/smooblue/describe-image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "description": "   "
            })))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/smooblue/describe-image", server.uri())).unwrap();
        let provider = SmooLlmAltText::new(url);
        let err = provider.describe(&[1], "image/jpeg").await.unwrap_err();
        assert!(err.to_string().contains("empty"));
    }
}
