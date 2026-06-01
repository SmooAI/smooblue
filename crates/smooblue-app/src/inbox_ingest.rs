//! Inbox v1 Phase B — ingestion task (pearl th-e17045).
//!
//! Background tokio task that polls upstream sources and UPSERTs into
//! the local SQLite store ([`crate::inbox`]):
//!
//! - `app.bsky.notification.listNotifications` → maps reason → InboxSource
//!   (reply / mention / quote). Likes / reposts / follows are filtered
//!   out — they're not actionable triage items.
//! - `chat.bsky.convo.listConvos` → any convo where the LATEST message
//!   was sent by someone else (and we haven't already ingested THAT
//!   message id) becomes an InboxSource::Dm item.
//!
//! UPSERT semantics + a stable `item_id` (hash of source + actor +
//!   subject + timestamp) keep the loop naturally idempotent:
//!   re-polling the same page won't double-insert, and a user's local
//!   triage state (read / archived / snoozed) is preserved on
//!   re-ingestion because the UPSERT's `ON CONFLICT DO UPDATE` clause
//!   only touches display fields, not triage state.
//!
//! Run loop:
//!
//! 1. Wait [`INGEST_INTERVAL_SECS`] (30s — same cadence as bsky.app's
//!    Notifications poll, gentle on the AppView).
//! 2. Skip the cycle if no session.
//! 3. Fetch + ingest notifications page (logged via eprintln).
//! 4. Fetch + ingest convos page.
//! 5. Loop.
//!
//! Errors are logged + the loop continues — we never want a transient
//! network blip to silently kill ingestion forever. The next 30s tick
//! retries.

use crate::auth_refresh::fresh_client;
use crate::inbox::{self, InboxItem, InboxSource};
use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use smooblue_atproto::{ConvoView, Message, Notification};
use smooblue_oauth::Session;
use std::time::Duration;

/// Poll cadence — matches bsky.app's own Notifications refresh rate.
const INGEST_INTERVAL_SECS: u64 = 30;

/// Notifications per page. 100 is the lexicon max per call; we then
/// follow the cursor up to [`MAX_INGESTION_PAGES`] times so a busy
/// account with lots of likes/reposts doesn't push real replies off
/// the end of page 1. A user reported their reply-from-an-hour-ago
/// never made it into the inbox because the first 50 items were
/// dominated by likes — bumping the per-page + chasing the cursor
/// fixes that without bloating each tick's HTTP cost meaningfully
/// (3 paginated GETs vs 1).
const NOTIFICATION_PAGE_LIMIT: u32 = 100;

/// Max notification pages to follow per cycle. 3 × 100 = 300 items
/// is enough that even an account averaging 10 notifications/minute
/// catches every reply/mention/quote in a 30-min poll cycle.
const MAX_INGESTION_PAGES: usize = 3;

/// Convo pages per ingestion — same cadence as DMs column poll.
const CONVO_PAGE_LIMIT: u32 = 50;

/// Truncation cap for the cached preview text on each row. Long
/// enough to disambiguate replies, short enough to render in one line.
const PREVIEW_LEN: usize = 240;

/// Reasons we surface in the inbox. Likes / reposts / follows are
/// noise for triage — they appear in the Notifications column but
/// shouldn't clutter the inbox.
fn classify_notification(reason: &str) -> Option<InboxSource> {
    match reason {
        "reply" => Some(InboxSource::DirectReply),
        "mention" => Some(InboxSource::Mention),
        "quote" => Some(InboxSource::Quote),
        // Bluesky lexicon doesn't currently emit a distinct
        // "reply-to-your-reply" reason — bsky.app derives it from the
        // reply chain. For Phase B we treat every "reply" as
        // DirectReply; a future enhancement can fetch the parent post
        // and upgrade to ReplyToReply when our DID appears as the
        // root replier.
        _ => None,
    }
}

/// Build an InboxItem from a notification, or None if the reason
/// isn't triage-worthy (like / repost / follow / starterpack-joined).
fn notification_to_item(n: &Notification) -> Option<InboxItem> {
    let source = classify_notification(&n.reason)?;

    // Subject: the post the notification is about. For reply / quote
    // / mention, reasonSubject is the AT-URI of our post. For mention
    // in a non-reply post, reasonSubject may be None — fall back to
    // the notification's own uri so click-through opens SOMETHING.
    let subject = n.reason_subject.clone().unwrap_or_else(|| n.uri.clone());

    let ts = n
        .indexed_at
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    // Stable id: hash inputs that uniquely identify the notification
    // across polls. cid + reason makes a like vs reply on the same
    // post distinct; including ts breaks ties for the rare case of
    // multiple notifications from the same actor on the same post.
    let item_id = format!("notif:{}:{}:{}", n.reason, n.cid, ts.timestamp());

    // Preview text: we don't have the reply body in the notification
    // payload (would require a getPostThread fetch). For Phase B we
    // construct a short caption — Phase C can hydrate the actual
    // text via a side-fetch.
    let preview = match source {
        InboxSource::DirectReply => Some("Replied to your post".to_string()),
        InboxSource::ReplyToReply => Some("Replied in your thread".to_string()),
        InboxSource::Quote => Some("Quoted your post".to_string()),
        InboxSource::Mention => Some("Mentioned you".to_string()),
        InboxSource::Dm => None, // unreachable for notifications path
    };

    let payload_json = serde_json::to_string(n).ok().unwrap_or_default();

    Some(InboxItem {
        item_id,
        source,
        actor_did: n.author.did.clone(),
        actor_handle: Some(n.author.handle.clone()),
        actor_display_name: n.author.display_name.clone(),
        actor_avatar: n.author.avatar.clone(),
        subject_uri: subject,
        preview,
        ts,
        directness: inbox::score(source, ts, n.is_read),
        read: n.is_read,
        archived: false,
        snoozed_until: None,
        device_id: None,
        synced_at: None,
        payload_json,
        // Follower count starts at 0; the enrichment pass after the
        // notifications ingest fills it via batched get_profiles +
        // inbox::set_actor_followers. Items stay sorted by directness
        // within their bucket until the enrichment lands.
        actor_follower_count: 0,
        ts_bucket: inbox::ts_bucket_for(ts),
    })
}

/// Build an InboxItem from a convo where the latest message is from
/// someone other than us. Returns None if:
/// - The convo has no last message (freshly created, never used).
/// - The last message was deleted (tombstone — already gone).
/// - WE sent the last message (no triage needed).
fn convo_to_item(c: &ConvoView, my_did: &str) -> Option<InboxItem> {
    let last = c.last_message.as_ref()?;
    let (text, sender_did, sent_at_str, message_id) = match last {
        Message::Live(v) => (
            v.text.clone(),
            v.sender.did.clone(),
            v.sent_at.clone(),
            v.id.clone(),
        ),
        Message::Deleted(_) => return None,
    };

    if sender_did == my_did {
        return None;
    }

    // Find the other-party profile in the convo's members list (the
    // sender). Falls back to None for the optional avatar / display
    // name — we still want the row even with bare DID if the lookup
    // misses.
    let other = c.members.iter().find(|m| m.did == sender_did);

    let ts = DateTime::parse_from_rfc3339(&sent_at_str)
        .ok()?
        .with_timezone(&Utc);

    // Stable id: the message id is globally unique within the convo
    // and stable across polls — perfect dedupe key.
    let item_id = format!("dm:{}:{}", c.id, message_id);

    let preview = Some(truncate_preview(&text));
    let payload_json = serde_json::to_string(c).ok().unwrap_or_default();

    Some(InboxItem {
        item_id,
        source: InboxSource::Dm,
        actor_did: sender_did,
        actor_handle: other.map(|p| p.handle.clone()),
        actor_display_name: other.and_then(|p| p.display_name.clone()),
        actor_avatar: other.and_then(|p| p.avatar.clone()),
        subject_uri: c.id.clone(),
        preview,
        ts,
        // DMs always start unread from the inbox perspective until
        // the user opens MessagesSheet (Phase C will sync that via
        // chat_update_read → inbox::set_read).
        directness: inbox::score(InboxSource::Dm, ts, false),
        read: false,
        archived: false,
        snoozed_until: None,
        device_id: None,
        synced_at: None,
        payload_json,
        actor_follower_count: 0,
        ts_bucket: inbox::ts_bucket_for(ts),
    })
}

fn truncate_preview(s: &str) -> String {
    if s.chars().count() <= PREVIEW_LEN {
        return s.to_string();
    }
    s.chars().take(PREVIEW_LEN).collect()
}

/// Spawn the ingestion loop. Called once from App-component first
/// render via a use_hook. Lives for the app's lifetime.
pub fn spawn_ingestion_task(session: Signal<Option<Session>>) {
    spawn(async move {
        // Initial sleep so we don't ingest during the first-render
        // burst (the user's columns are loading their own pages).
        // A short delay also lets the OAuth session settle.
        tokio::time::sleep(Duration::from_secs(5)).await;

        loop {
            // Wait BEFORE each cycle (rather than after) so the very
            // first iteration kicks within INGEST_INTERVAL of mount,
            // not 2× the interval.
            run_cycle(session).await;
            tokio::time::sleep(Duration::from_secs(INGEST_INTERVAL_SECS)).await;
        }
    });
}

async fn run_cycle(session: Signal<Option<Session>>) {
    // No session → nothing to ingest. Demo mode also skips because
    // demo:: doesn't have real notifications / DMs to fetch.
    if session.read().is_none() || crate::demo::is_active() {
        return;
    }

    let Some(client) = fresh_client(session).await else {
        crate::diag!("[inbox_ingest] no fresh client this cycle");
        return;
    };

    let my_did = session
        .read()
        .as_ref()
        .map(|s| s.did.clone())
        .unwrap_or_default();

    // Distinct actor DIDs seen this cycle — used by the profile
    // enrichment pass below to batch-fetch follower counts.
    let mut actor_dids_to_enrich: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // ─── Notifications (paginated up to MAX_INGESTION_PAGES) ──
    let mut cursor: Option<String> = None;
    let mut total_seen = 0usize;
    let mut total_ingested = 0usize;
    let mut total_skipped = 0usize;
    let mut pages_fetched = 0usize;
    loop {
        if pages_fetched >= MAX_INGESTION_PAGES {
            break;
        }
        let resp = match client
            .list_notifications(cursor.as_deref(), NOTIFICATION_PAGE_LIMIT)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                crate::diag!("[inbox_ingest] listNotifications page {pages_fetched} failed: {e}");
                break;
            }
        };
        pages_fetched += 1;
        let page_total = resp.notifications.len();
        if page_total == 0 {
            break;
        }
        total_seen += page_total;
        for n in &resp.notifications {
            let Some(item) = notification_to_item(n) else {
                total_skipped += 1;
                continue;
            };
            actor_dids_to_enrich.insert(item.actor_did.clone());
            let upsert_result = tokio::task::spawn_blocking(move || inbox::upsert(&item)).await;
            match upsert_result {
                Ok(Ok(())) => total_ingested += 1,
                Ok(Err(e)) => crate::diag!("[inbox_ingest] notif upsert failed: {e}"),
                Err(e) => crate::diag!("[inbox_ingest] notif blocking task panicked: {e}"),
            }
        }
        // No cursor means we've reached the end of the user's
        // notification history — bail before another GET.
        cursor = match resp.cursor {
            Some(c) => Some(c),
            None => break,
        };
    }
    eprintln!(
        "[inbox_ingest] notifications: pages={pages_fetched} ingested={total_ingested} skipped={total_skipped} total={total_seen}"
    );

    // ─── Convos (DMs) ────────────────────────────────────────
    match client.chat_list_convos(None, CONVO_PAGE_LIMIT).await {
        Ok(resp) => {
            let total = resp.convos.len();
            let mut ingested = 0usize;
            let mut skipped = 0usize;
            for c in &resp.convos {
                let Some(item) = convo_to_item(c, &my_did) else {
                    skipped += 1;
                    continue;
                };
                actor_dids_to_enrich.insert(item.actor_did.clone());
                let upsert_result = tokio::task::spawn_blocking(move || inbox::upsert(&item)).await;
                match upsert_result {
                    Ok(Ok(())) => ingested += 1,
                    Ok(Err(e)) => crate::diag!("[inbox_ingest] dm upsert failed: {e}"),
                    Err(e) => crate::diag!("[inbox_ingest] dm blocking task panicked: {e}"),
                }
            }
            crate::diag!(
                "[inbox_ingest] convos: ingested={ingested} skipped={skipped} total={total}"
            );
        }
        Err(e) => crate::diag!("[inbox_ingest] listConvos failed: {e}"),
    }

    // ─── Profile enrichment ─────────────────────────────────
    // Batched get_profiles for every distinct actor we just saw,
    // then write follower counts back to all their rows so the
    // ts_bucket / follower_count / directness sort actually has
    // useful follower data. Cheap (~2 round-trips per 50 unique
    // actors) and fire-and-forget — failure leaves follower_count
    // at its previous value (MAX semantics in inbox::upsert protect
    // against a 0 from a stale path silently downgrading a real
    // cached count). Pearl th-bce4fb.
    let dids: Vec<String> = actor_dids_to_enrich.into_iter().collect();
    if !dids.is_empty() {
        match client.get_profiles(&dids).await {
            Ok(profiles) => {
                let mut updated = 0usize;
                for p in profiles {
                    let count = p.followers_count.unwrap_or(0) as i64;
                    let did = p.did.clone();
                    let res = tokio::task::spawn_blocking(move || {
                        inbox::set_actor_followers(&did, count)
                    })
                    .await;
                    match res {
                        Ok(Ok(())) => updated += 1,
                        Ok(Err(e)) => {
                            crate::diag!("[inbox_ingest] set_actor_followers failed: {e}")
                        }
                        Err(e) => crate::diag!("[inbox_ingest] enrich blocking task panicked: {e}"),
                    }
                }
                crate::diag!("[inbox_ingest] enriched followers: updated={updated}");
            }
            Err(e) => crate::diag!("[inbox_ingest] get_profiles failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smooblue_atproto::PostAuthor;

    fn fake_notif(reason: &str) -> Notification {
        Notification {
            uri: "at://did:plc:alice/app.bsky.feed.post/abc".into(),
            cid: "cid-1".into(),
            author: PostAuthor {
                did: "did:plc:alice".into(),
                handle: "alice.bsky.social".into(),
                display_name: Some("Alice".into()),
                avatar: None,
            },
            reason: reason.into(),
            reason_subject: Some("at://did:plc:me/app.bsky.feed.post/xyz".into()),
            indexed_at: Some("2026-05-31T12:00:00Z".into()),
            is_read: false,
        }
    }

    #[test]
    fn classify_drops_noise_reasons() {
        for reason in ["like", "repost", "follow", "starterpack-joined", "unknown"] {
            assert!(
                classify_notification(reason).is_none(),
                "{reason} should be filtered out of inbox"
            );
        }
    }

    #[test]
    fn classify_includes_triage_reasons() {
        assert_eq!(
            classify_notification("reply"),
            Some(InboxSource::DirectReply)
        );
        assert_eq!(classify_notification("mention"), Some(InboxSource::Mention));
        assert_eq!(classify_notification("quote"), Some(InboxSource::Quote));
    }

    #[test]
    fn notification_to_item_populates_actor_and_subject() {
        let n = fake_notif("reply");
        let item = notification_to_item(&n).expect("reply should map to item");
        assert_eq!(item.source, InboxSource::DirectReply);
        assert_eq!(item.actor_did, "did:plc:alice");
        assert_eq!(item.actor_handle.as_deref(), Some("alice.bsky.social"));
        assert_eq!(item.subject_uri, "at://did:plc:me/app.bsky.feed.post/xyz");
        assert!(!item.read);
        assert!(item.directness >= 60);
    }

    #[test]
    fn notification_id_is_stable_across_polls() {
        let n = fake_notif("reply");
        let a = notification_to_item(&n).unwrap();
        let b = notification_to_item(&n).unwrap();
        assert_eq!(a.item_id, b.item_id, "same notif must produce same item_id");
    }

    #[test]
    fn preview_truncates_long_text() {
        let long = "x".repeat(500);
        let truncated = truncate_preview(&long);
        assert_eq!(truncated.chars().count(), PREVIEW_LEN);
    }

    #[test]
    fn preview_passes_short_text_through() {
        let short = "hello";
        assert_eq!(truncate_preview(short), "hello");
    }
}
