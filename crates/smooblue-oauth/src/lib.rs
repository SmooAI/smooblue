//! ATproto / Bluesky OAuth client.
//!
//! Implements the public-client desktop flow:
//! 1. Resolve handle → DID (via PDS or PLC directory)
//! 2. Fetch protected resource metadata, then authorization server metadata
//! 3. Generate PKCE pair + DPoP keypair
//! 4. Push authorization request (PAR)
//! 5. Open browser to `authorization_endpoint?request_uri=...`
//! 6. Listen on a loopback `127.0.0.1` port for the callback
//! 7. Exchange the authorization code for tokens (with DPoP proof)
//! 8. Surface the bound DPoP key + access/refresh tokens via [`Session`]
//!
//! All HTTP is performed through [`smooai_fetch`] so we get retries,
//! exponential backoff, and circuit-breaking for free. All steps emit
//! structured logs through [`smooai_logger`].

pub mod dpop;
pub mod loopback;
pub mod metadata;
pub mod pkce;
pub mod session;

mod client;
mod error;

pub use client::{OAuthClient, OAuthClientConfig};
pub use error::OAuthError;
pub use session::Session;
