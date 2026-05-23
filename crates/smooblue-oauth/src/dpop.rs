//! DPoP (Demonstrating Proof-of-Possession) per RFC 9449.
//!
//! ATproto OAuth *requires* DPoP-bound tokens. Each request carries a
//! `DPoP` header whose value is a JWS signed by an ES256 key we hold
//! locally, asserting the HTTP method + URL + (optional) nonce + (optional)
//! access-token hash.

use crate::error::OAuthError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use p256::ecdsa::{signature::Signer, Signature, SigningKey, VerifyingKey};
use p256::pkcs8::EncodePrivateKey;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// ES256 keypair bound to a single OAuth session.
///
/// Persist via [`Self::to_pkcs8_pem`] / [`Self::from_pkcs8_pem`] so the
/// same key signs proofs across app restarts (otherwise the access token
/// becomes useless on first use).
#[derive(Clone)]
pub struct DpopKey {
    signing: SigningKey,
}

impl std::fmt::Debug for DpopKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DpopKey")
            .field("kid", &self.thumbprint())
            .finish()
    }
}

impl DpopKey {
    /// Generate a fresh ES256 keypair.
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::random(&mut OsRng),
        }
    }

    /// Export as PKCS8 PEM (for keyring persistence).
    pub fn to_pkcs8_pem(&self) -> Result<String, OAuthError> {
        self.signing
            .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .map(|s| s.to_string())
            .map_err(|e| OAuthError::Crypto(format!("encode pkcs8: {e}")))
    }

    /// Import from PKCS8 PEM (paired with [`Self::to_pkcs8_pem`]).
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self, OAuthError> {
        use p256::pkcs8::DecodePrivateKey;
        let signing = SigningKey::from_pkcs8_pem(pem)
            .map_err(|e| OAuthError::Crypto(format!("decode pkcs8: {e}")))?;
        Ok(Self { signing })
    }

    /// JWK representation of the *public* half — embedded in every DPoP header.
    pub fn public_jwk(&self) -> PublicJwk {
        let verifying: VerifyingKey = *self.signing.verifying_key();
        let point = verifying.to_encoded_point(false);
        let x = point.x().expect("ES256 public point must have x");
        let y = point.y().expect("ES256 public point must have y");
        PublicJwk {
            kty: "EC".into(),
            crv: "P-256".into(),
            x: URL_SAFE_NO_PAD.encode(x),
            y: URL_SAFE_NO_PAD.encode(y),
        }
    }

    /// RFC 7638 JWK thumbprint — used as the `jkt` claim in token requests
    /// and as a stable identifier for the bound key.
    pub fn thumbprint(&self) -> String {
        let jwk = self.public_jwk();
        // RFC 7638 canonical form: members in lexicographic order, no whitespace.
        let canonical = format!(
            r#"{{"crv":"{}","kty":"{}","x":"{}","y":"{}"}}"#,
            jwk.crv, jwk.kty, jwk.x, jwk.y
        );
        let digest = Sha256::digest(canonical.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }

    /// Build a DPoP proof JWT for a single HTTP request.
    ///
    /// Set `access_token` to bind the proof to an existing access token
    /// (required for resource requests; omit for the token-exchange call).
    /// Set `nonce` to the most recent `DPoP-Nonce` header value the server
    /// returned (servers may demand a nonce and respond with `use_dpop_nonce`).
    pub fn sign_proof(
        &self,
        htm: &str,
        htu: &str,
        nonce: Option<&str>,
        access_token: Option<&str>,
    ) -> Result<String, OAuthError> {
        let header = DpopHeader {
            typ: "dpop+jwt".into(),
            alg: "ES256".into(),
            jwk: self.public_jwk(),
        };
        let now = Utc::now().timestamp();
        let claims = DpopClaims {
            jti: uuid::Uuid::new_v4().to_string(),
            htm: htm.to_string(),
            htu: htu.to_string(),
            iat: now,
            nonce: nonce.map(String::from),
            ath: access_token.map(|t| {
                let digest = Sha256::digest(t.as_bytes());
                URL_SAFE_NO_PAD.encode(digest)
            }),
        };

        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
        let claims_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
        let signing_input = format!("{header_b64}.{claims_b64}");
        let signature: Signature = self.signing.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        Ok(format!("{signing_input}.{sig_b64}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
}

#[derive(Serialize)]
struct DpopHeader {
    typ: String,
    alg: String,
    jwk: PublicJwk,
}

#[derive(Serialize)]
struct DpopClaims {
    jti: String,
    htm: String,
    htu: String,
    iat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ath: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, decode_header, jwk::Jwk, DecodingKey, Validation};
    use serde_json::Value;

    #[test]
    fn keypair_round_trips_through_pem() {
        let k1 = DpopKey::generate();
        let pem = k1.to_pkcs8_pem().unwrap();
        let k2 = DpopKey::from_pkcs8_pem(&pem).unwrap();
        assert_eq!(k1.thumbprint(), k2.thumbprint());
    }

    #[test]
    fn proof_is_a_valid_jws_with_correct_claims() {
        let key = DpopKey::generate();
        let proof = key
            .sign_proof("POST", "https://bsky.social/oauth/token", None, None)
            .unwrap();

        // Header must announce dpop+jwt + ES256 + the embedded jwk.
        let header = decode_header(&proof).expect("dpop proof must be a valid JWS");
        assert_eq!(header.typ.as_deref(), Some("dpop+jwt"));
        assert_eq!(header.alg, jsonwebtoken::Algorithm::ES256);
        assert!(header.jwk.is_some(), "DPoP proof must embed JWK in header");

        // Verify the signature using the embedded key.
        let jwk: Jwk = header.jwk.unwrap();
        let decoding = DecodingKey::from_jwk(&jwk).unwrap();
        let mut validation = Validation::new(jsonwebtoken::Algorithm::ES256);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        let token = decode::<Value>(&proof, &decoding, &validation)
            .expect("signature must verify against embedded JWK");

        let claims = token.claims;
        assert_eq!(claims["htm"], "POST");
        assert_eq!(claims["htu"], "https://bsky.social/oauth/token");
        assert!(claims["jti"].is_string());
        assert!(claims["iat"].is_i64());
        assert!(
            claims.get("nonce").is_none(),
            "no nonce given => no nonce claim"
        );
        assert!(
            claims.get("ath").is_none(),
            "no access_token given => no ath claim"
        );
    }

    #[test]
    fn proof_with_nonce_and_token_includes_both_claims() {
        let key = DpopKey::generate();
        let proof = key
            .sign_proof(
                "GET",
                "https://example.test/xrpc/foo",
                Some("nonce-abc"),
                Some("access-token-xyz"),
            )
            .unwrap();
        let parts: Vec<&str> = proof.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims_json = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let claims: Value = serde_json::from_slice(&claims_json).unwrap();
        assert_eq!(claims["nonce"], "nonce-abc");
        // ath = base64url(sha256(access_token))
        let expected_ath = URL_SAFE_NO_PAD.encode(Sha256::digest(b"access-token-xyz"));
        assert_eq!(claims["ath"], expected_ath);
    }

    /// Thumbprint must be stable across calls and deterministic per key.
    #[test]
    fn thumbprint_is_stable() {
        let key = DpopKey::generate();
        let t1 = key.thumbprint();
        let t2 = key.thumbprint();
        assert_eq!(t1, t2);
        let other = DpopKey::generate();
        assert_ne!(t1, other.thumbprint());
    }
}
