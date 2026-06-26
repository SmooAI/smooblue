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

pub mod chat;
mod client;
pub mod constellation;
mod error;
pub mod feed;
pub mod notifications;
pub mod richtext;
pub mod tid;

pub use chat::{
    ChatProfile, ConvoView, DeletedMessageView, GetConvoResponse, GetMessagesResponse,
    ListConvosResponse, Message, MessageInput, MessageSender, MessageView,
};
pub use client::{
    AspectRatio, AtClient, BlobLink, BlobRef, CreatedRecord, LinkCard, PostExternal, PostImage,
    PostVideo, ReplyRef, StrongRef,
};
pub use constellation::{ConstellationClient, LinkingRecord, LinksResponse};
pub use error::AtError;
pub use feed::{
    ActorProfile, ActorViewerState, Embed, EmbedAspectRatio, EmbedExternal, EmbedImage, EmbedKind,
    EmbedMedia, EmbedRecordView, EmbedRecordWrapper, FacetSegment, FeedGeneratorView,
    FeedGeneratorsResponse, FeedItem, FeedResponse, GetPostThreadResponse, KnownFollowersResponse,
    Label, LikeView, LikesResponse, ListRecordsResponse, ListView, ListedRecord, ListsResponse,
    PostAuthor, PostRecord, PostView, PostViewerState, PreferencesResponse, QuotesResponse,
    RepostedByResponse, SavedFeedItem, SuggestionsResponse, ThreadView,
};
pub use notifications::{
    group_notifications, Notification, NotificationGroup, NotificationsResponse,
};
pub use richtext::{
    detect_facet_candidates, Facet, FacetCandidate, FacetFeature, FacetIndex, FacetKind,
};
