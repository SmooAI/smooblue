//! Bluesky DMs — `chat.bsky.*` XRPC client.
//!
//! Architecture notes:
//! - Chat requests go to the user's **PDS** (not the AppView at
//!   `bsky.app`), with an `atproto-proxy: did:web:api.bsky.chat#bsky_chat`
//!   header that tells the PDS to forward the request to the chat
//!   service. The proxy header + PDS routing are wired through
//!   [`AtClient::get_json_proxied`] / [`AtClient::post_json_proxied`].
//! - Bluesky DMs are NOT end-to-end encrypted; messages are stored
//!   on Bluesky's chat servers and accessible to their moderators.
//!   Documented in `docs/Security/Security.md` (DM addendum).
//! - The Bluesky chat lexicon distinguishes `MessageView` (a normal
//!   message) from `DeletedMessageView` (a placeholder for messages
//!   the sender deleted). Both share `id`, `rev`, `sender`, `sentAt`.
//!   We model them as a single [`Message`] enum to keep the consumer
//!   simple.

use crate::client::AtClient;
use crate::error::AtError;
use serde::{Deserialize, Serialize};

/// The `atproto-proxy` header target for all `chat.bsky.*` requests.
/// Per Bluesky's docs, this directs the PDS to forward the request
/// to their chat service.
const CHAT_PROXY: &str = "did:web:api.bsky.chat#bsky_chat";

/// Compact profile used by the chat lexicon (a subset of
/// `app.bsky.actor.defs#profileViewBasic`). Just enough to render a
/// convo-list-item or a message bubble's avatar + name.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ChatProfile {
    pub did: String,
    pub handle: String,
    #[serde(
        rename = "displayName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// Single message in a convo. Either a live message or a tombstone
/// for one the sender deleted. The chat lexicon uses an
/// `$type`-tagged union; we discriminate on that field.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "$type")]
pub enum Message {
    /// `chat.bsky.convo.defs#messageView` — a live message with
    /// renderable text + facets.
    #[serde(rename = "chat.bsky.convo.defs#messageView")]
    Live(MessageView),
    /// `chat.bsky.convo.defs#deletedMessageView` — sender deleted
    /// the message; renders as a "this message was deleted" placeholder.
    #[serde(rename = "chat.bsky.convo.defs#deletedMessageView")]
    Deleted(DeletedMessageView),
}

/// Live message — text + sender + timestamp.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageView {
    pub id: String,
    pub rev: String,
    pub text: String,
    /// `app.bsky.richtext.facet[]` — same shape as post facets;
    /// covers @mentions, #hashtags, and link spans for highlighting.
    /// Modeled as raw JSON for now to avoid coupling to the existing
    /// richtext type before we know whether chat reuses identical
    /// shapes. UI can pretty-print as plain text via `.text` and
    /// upgrade later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facets: Option<serde_json::Value>,
    /// `embed` (record / images / video) — same. Raw JSON for v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed: Option<serde_json::Value>,
    pub sender: MessageSender,
    /// ISO-8601 timestamp from the chat server.
    #[serde(rename = "sentAt")]
    pub sent_at: String,
}

/// Tombstone — server keeps the id + sender so the conversation
/// thread renders coherently even after a delete.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeletedMessageView {
    pub id: String,
    pub rev: String,
    pub sender: MessageSender,
    #[serde(rename = "sentAt")]
    pub sent_at: String,
}

/// Minimal sender ref — the chat lexicon only includes the DID here.
/// Lookup of avatar/handle goes through `ChatProfile` in the convo's
/// `members` list (or `getProfile` if you need more).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageSender {
    pub did: String,
}

/// Conversation summary — what backs a convo-list row.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ConvoView {
    pub id: String,
    pub rev: String,
    /// Always 1–N profiles. For a 1:1 DM, this is two entries (you +
    /// the other person). The UI typically renders the OTHER member
    /// — call sites filter against the current session's DID.
    pub members: Vec<ChatProfile>,
    /// Last message preview for the convo-list row. May be Deleted
    /// (tombstone) if the most recent message was deleted; may be
    /// absent for a freshly-created empty convo.
    #[serde(
        rename = "lastMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_message: Option<Message>,
    pub muted: bool,
    /// Number of messages the user hasn't read. Drives the unread
    /// badge on the convo-list row.
    #[serde(rename = "unreadCount")]
    pub unread_count: u32,
}

/// Outbound message — what you build to send. The server adds id,
/// rev, sender, sentAt on the response.
#[derive(Debug, Clone, Serialize)]
pub struct MessageInput {
    pub text: String,
    /// Optional facets — same shape as `MessageView::facets`.
    /// Use the existing rich-text resolver to compute @mention /
    /// link spans before sending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facets: Option<serde_json::Value>,
    /// Optional embed (image / record). Raw JSON to mirror MessageView.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed: Option<serde_json::Value>,
}

/// Response from `chat.bsky.convo.listConvos`.
#[derive(Debug, Clone, Deserialize)]
pub struct ListConvosResponse {
    pub convos: Vec<ConvoView>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// Response from `chat.bsky.convo.getConvo` and `updateRead`.
#[derive(Debug, Clone, Deserialize)]
pub struct GetConvoResponse {
    pub convo: ConvoView,
}

/// Response from `chat.bsky.convo.getMessages`.
#[derive(Debug, Clone, Deserialize)]
pub struct GetMessagesResponse {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub cursor: Option<String>,
}

impl AtClient {
    /// List the current user's convos, newest activity first.
    /// Paginated via `cursor`; pass `None` for the first page.
    pub async fn chat_list_convos(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<ListConvosResponse, AtError> {
        let mut url = self
            .pds_url()?
            .join("/xrpc/chat.bsky.convo.listConvos")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json_proxied(&url, CHAT_PROXY).await
    }

    /// Get a single convo by id — primarily used after `sendMessage`
    /// or `updateRead` returns a fresh convo state for the sidebar.
    pub async fn chat_get_convo(&self, convo_id: &str) -> Result<GetConvoResponse, AtError> {
        let mut url = self
            .pds_url()?
            .join("/xrpc/chat.bsky.convo.getConvo")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut().append_pair("convoId", convo_id);
        self.get_json_proxied(&url, CHAT_PROXY).await
    }

    /// Get messages for a convo, newest first. Paginated via `cursor`.
    pub async fn chat_get_messages(
        &self,
        convo_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<GetMessagesResponse, AtError> {
        let mut url = self
            .pds_url()?
            .join("/xrpc/chat.bsky.convo.getMessages")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        url.query_pairs_mut().append_pair("convoId", convo_id);
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(c) = cursor {
            url.query_pairs_mut().append_pair("cursor", c);
        }
        self.get_json_proxied(&url, CHAT_PROXY).await
    }

    /// Send a message to a convo. Returns the server's canonical
    /// MessageView so the UI can render immediately with the
    /// server-assigned id/sentAt.
    pub async fn chat_send_message(
        &self,
        convo_id: &str,
        message: &MessageInput,
    ) -> Result<MessageView, AtError> {
        let url = self
            .pds_url()?
            .join("/xrpc/chat.bsky.convo.sendMessage")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = serde_json::json!({
            "convoId": convo_id,
            "message": message,
        });
        self.post_json_proxied(&url, &body, CHAT_PROXY).await
    }

    /// Mark messages in a convo as read up to (and including) the
    /// given message id. Server returns the updated ConvoView with
    /// `unreadCount` zeroed (or reduced).
    pub async fn chat_update_read(
        &self,
        convo_id: &str,
        message_id: Option<&str>,
    ) -> Result<GetConvoResponse, AtError> {
        let url = self
            .pds_url()?
            .join("/xrpc/chat.bsky.convo.updateRead")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        let body = match message_id {
            Some(id) => serde_json::json!({ "convoId": convo_id, "messageId": id }),
            None => serde_json::json!({ "convoId": convo_id }),
        };
        self.post_json_proxied(&url, &body, CHAT_PROXY).await
    }

    /// Find (or create) the 1:1 convo with the given member DIDs.
    /// Used when the user clicks "Message" on a profile — we don't
    /// know the convo id, only the recipient.
    pub async fn chat_get_convo_for_members(
        &self,
        members: &[&str],
    ) -> Result<GetConvoResponse, AtError> {
        let mut url = self
            .pds_url()?
            .join("/xrpc/chat.bsky.convo.getConvoForMembers")
            .map_err(|e| AtError::Decode(e.to_string()))?;
        for did in members {
            url.query_pairs_mut().append_pair("members", did);
        }
        self.get_json_proxied(&url, CHAT_PROXY).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_view_deserializes_with_type_tag() {
        // Sample response shape from chat.bsky.convo.getMessages.
        let json = serde_json::json!({
            "$type": "chat.bsky.convo.defs#messageView",
            "id": "abc",
            "rev": "001",
            "text": "hello",
            "sender": { "did": "did:plc:sender" },
            "sentAt": "2026-05-31T00:00:00.000Z",
        });
        let m: Message = serde_json::from_value(json).expect("messageView deserializes");
        match m {
            Message::Live(v) => {
                assert_eq!(v.id, "abc");
                assert_eq!(v.text, "hello");
                assert_eq!(v.sender.did, "did:plc:sender");
            }
            Message::Deleted(_) => panic!("expected Live, got Deleted"),
        }
    }

    #[test]
    fn deleted_message_deserializes_via_type_tag() {
        let json = serde_json::json!({
            "$type": "chat.bsky.convo.defs#deletedMessageView",
            "id": "abc",
            "rev": "001",
            "sender": { "did": "did:plc:sender" },
            "sentAt": "2026-05-31T00:00:00.000Z",
        });
        let m: Message = serde_json::from_value(json).expect("deletedMessageView deserializes");
        match m {
            Message::Deleted(d) => assert_eq!(d.id, "abc"),
            Message::Live(_) => panic!("expected Deleted, got Live"),
        }
    }

    #[test]
    fn convo_view_round_trips() {
        let convo = ConvoView {
            id: "convo-1".into(),
            rev: "001".into(),
            members: vec![ChatProfile {
                did: "did:plc:me".into(),
                handle: "me.bsky.social".into(),
                display_name: Some("Me".into()),
                avatar: None,
            }],
            last_message: None,
            muted: false,
            unread_count: 0,
        };
        let json = serde_json::to_value(&convo).expect("serialize");
        let back: ConvoView = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.id, "convo-1");
        assert_eq!(back.members.len(), 1);
        assert_eq!(back.unread_count, 0);
    }
}
