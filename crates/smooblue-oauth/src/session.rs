//! Persistent OAuth session.
//!
//! Holds the DPoP-bound access + refresh tokens plus the PKCS8 PEM of the
//! signing key so we can keep signing proofs across app restarts.

use crate::dpop::DpopKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// User DID (e.g., `did:plc:abc123`).
    pub did: String,
    /// User handle at sign-in time (display only — handles can change).
    pub handle: String,
    /// PDS URL.
    pub pds: String,
    /// Authorization server URL.
    pub issuer: String,
    /// DPoP-bound access token.
    pub access_token: String,
    /// Refresh token (also DPoP-bound).
    pub refresh_token: String,
    /// Access token type (always `DPoP`).
    pub token_type: String,
    /// Access token expiry (seconds since unix epoch).
    pub expires_at: i64,
    /// PKCS8 PEM of the bound ES256 signing key.
    pub dpop_pem: String,
    /// Most recent server-issued DPoP nonce (mutated on each response).
    pub dpop_nonce: Option<String>,
}

impl Session {
    /// Reconstruct the bound DPoP key.
    pub fn dpop_key(&self) -> Result<DpopKey, crate::OAuthError> {
        DpopKey::from_pkcs8_pem(&self.dpop_pem)
    }

    /// True if the access token expires within the next 30 seconds.
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.expires_at <= now + 30
    }
}
