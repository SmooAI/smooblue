//! PKCE (Proof Key for Code Exchange) per RFC 7636.
//!
//! Generates a high-entropy `code_verifier` and its SHA-256 `code_challenge`
//! using S256 (the only method accepted by Bluesky / ATproto OAuth).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// A freshly-generated PKCE pair. Hold onto [`Self::verifier`] until the
/// token exchange; send [`Self::challenge`] in the authorization request.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    /// Generate a new PKCE pair. The verifier is 64 bytes of entropy
    /// base64url-encoded (~86 chars, within the 43–128 char RFC range).
    pub fn generate() -> Self {
        let mut bytes = [0u8; 64];
        rand::thread_rng().fill_bytes(&mut bytes);
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = derive_challenge(&verifier);
        Self {
            verifier,
            challenge,
        }
    }

    /// Derived challenge method — always `S256` for ATproto OAuth.
    pub fn method(&self) -> &'static str {
        "S256"
    }
}

fn derive_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_is_well_formed() {
        let pkce = Pkce::generate();
        assert!(
            (43..=128).contains(&pkce.verifier.len()),
            "verifier length {} out of RFC 7636 range",
            pkce.verifier.len()
        );
        assert_eq!(
            pkce.challenge,
            derive_challenge(&pkce.verifier),
            "challenge must be SHA256(verifier) base64url-encoded"
        );
        assert_eq!(pkce.method(), "S256");
    }

    #[test]
    fn pkce_pairs_are_unique() {
        let a = Pkce::generate();
        let b = Pkce::generate();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.challenge, b.challenge);
    }

    /// RFC 7636 Appendix B test vector.
    #[test]
    fn matches_rfc_7636_appendix_b_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected_challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(derive_challenge(verifier), expected_challenge);
    }
}
