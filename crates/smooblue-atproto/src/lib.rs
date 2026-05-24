//! ATproto / Bluesky AppView client.
//!
//! Every resource request:
//! - Carries `Authorization: DPoP <access_token>` (token_type from session)
//! - Carries `DPoP: <proof>` with `ath` = SHA256(access_token), `nonce` = last
//!   server-issued `DPoP-Nonce`
//! - Retries once on `use_dpop_nonce` errors, using the freshly-issued nonce
//!
//! All HTTP goes through reqwest with logging via tracing. The intent is to
//! swap in `smooai_fetch` for retry/circuit-break once it surfaces a generic
//! request-builder API; for now the DPoP nonce loop is hand-rolled.

mod client;
mod error;
pub mod feed;
pub mod notifications;

pub use client::{
    AspectRatio, AtClient, BlobLink, BlobRef, CreatedRecord, PostImage, ReplyRef, StrongRef,
};
pub use error::AtError;
pub use feed::{
    ActorProfile, Embed, EmbedImage, EmbedKind, FeedItem, FeedResponse, PostAuthor, PostRecord,
};
pub use notifications::{Notification, NotificationsResponse};
