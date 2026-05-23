use thiserror::Error;

#[derive(Debug, Error)]
pub enum AtError {
    #[error("http error: {0}")]
    Http(String),

    #[error("server returned status {status}: {body}")]
    Status { status: u16, body: String },

    #[error("decode error: {0}")]
    Decode(String),

    #[error("oauth error: {0}")]
    OAuth(#[from] smooblue_oauth::OAuthError),

    #[error("missing DPoP nonce after retry")]
    MissingDpopNonce,

    #[error("session expired")]
    SessionExpired,
}

impl From<reqwest::Error> for AtError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}

impl From<serde_json::Error> for AtError {
    fn from(e: serde_json::Error) -> Self {
        Self::Decode(e.to_string())
    }
}
