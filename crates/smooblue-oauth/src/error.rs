use thiserror::Error;

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("invalid handle: {0}")]
    InvalidHandle(String),

    #[error("identity resolution failed: {0}")]
    IdentityResolution(String),

    #[error("metadata discovery failed: {0}")]
    Metadata(String),

    #[error("PAR (pushed authorization request) failed: {0}")]
    Par(String),

    #[error("authorization callback timed out")]
    CallbackTimeout,

    #[error("authorization callback returned error: {0}")]
    CallbackError(String),

    #[error("state mismatch in callback (CSRF)")]
    StateMismatch,

    #[error("token exchange failed: {0}")]
    TokenExchange(String),

    #[error("missing DPoP nonce (server expected one but didn't provide it)")]
    MissingDpopNonce,

    #[error("could not open browser: {0}")]
    BrowserOpen(String),

    #[error("loopback bind failed: {0}")]
    LoopbackBind(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
}

impl From<reqwest::Error> for OAuthError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}
