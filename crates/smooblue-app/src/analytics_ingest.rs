//! Account analytics — background ingestion tasks (pearl: account-analytics).
//!
//! Mirrors [`crate::inbox_ingest`]: a single long-lived tokio task,
//! spawned once at App-mount, that re-derives its auth via
//! [`fresh_client`] each cycle (so OAuth-token rotation is transparent)
//! and writes everything through the shared analytics store
//! ([`crate::analytics`]) on `spawn_blocking` threads so the UI thread
//! never blocks on SQLite.
//!
//! Two jobs run from the one loop:
//!
//! 1. **Daily snapshot** — every cycle, `getProfile` on our own DID →
//!    [`crate::analytics::upsert_metric_snapshot_for_today`]. Date-keyed,
//!    so it's a cheap no-op refresh after the first write of the day and
//!    builds a true forward record of net displayed counts.
//!
//! 2. **One-time resumable backfill** — a phase machine
//!    (`pending → posts → following → followers → engagement → complete`)
//!    that reconstructs history. Each phase drains its pages and
//!    checkpoints its cursor to `backfill_state` per page, so a crash
//!    resumes from the last committed page rather than restarting. All
//!    writes are idempotent UPSERTs, so a resumed page that re-processes
//!    a few already-seen records is harmless.
//!
//!    - **posts** — `listRecords(app.bsky.feed.post)` on our own repo →
//!      `post_metrics` (TID-dated). First-party engagement counts
//!      (`getPosts`) are then fetched for the most-recent
//!      [`ENGAGEMENT_POST_WINDOW`] posts only (v1 scope).
//!    - **following** — `listRecords(app.bsky.graph.follow)` → outgoing
//!      `follow_events` (we follow them).
//!    - **followers** — Constellation `links(.subject)` → incoming
//!      `follow_events` (they follow us), and — in the same per-page pass
//!      — profile enrichment (`getProfiles`) + clout scoring
//!      ([`crate::analytics_score::rescore`]) into `follower_stats`.
//!      `mutual` comes from the enriched profile's viewer state
//!      (`viewer.following` present ⇒ we follow them back); engagement
//!      counts come from the inbox store.
//!    - **engagement** — no-op passthrough in v1: per-follower engagement
//!      is seeded during the `followers` pass and refreshed continuously
//!      by the inbox ingest cycle, so this phase just advances to
//!      `complete`. Kept in the machine for forward-compat.
//!
//! Guards: a cycle is a no-op without a session or in demo mode (demo
//! has no real repo / Constellation backlinks to read).

use crate::analytics::{
    self, BackfillPhase, BackfillState, FollowDir, FollowEvent, FollowerStat, PostMetric,
};
use crate::auth_refresh::fresh_client;
use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use smooblue_atproto::{tid, AtClient, ConstellationClient};
use smooblue_oauth::Session;
use std::collections::HashMap;
use std::time::Duration;

/// Seconds between backfill/snapshot cycles. Gentle — the backfill is a
/// one-time job and the daily snapshot only needs to land once per day.
const BACKFILL_CHECK_INTERVAL_SECS: u64 = 60;

/// Records per `listRecords` / Constellation `links` page (lexicon max).
const PAGE_LIMIT: u32 = 100;

/// v1: first-party engagement counts are fetched only for the most
/// recent N own posts (full-history engagement is deferred).
const ENGAGEMENT_POST_WINDOW: usize = 50;

/// Cap on the cached `text_preview` per post — one line's worth.
const PREVIEW_LEN: usize = 240;

/// Collection NSIDs we list from our own repo.
const COLLECTION_POST: &str = "app.bsky.feed.post";
const COLLECTION_FOLLOW: &str = "app.bsky.graph.follow";

/// Spawn the analytics background loop. Called once from App-component
/// first render via a `use_hook`; lives for the app's lifetime. Re-reads
/// session freshness each cycle so OAuth rotation is transparent.
pub fn spawn_analytics_tasks(session: Signal<Option<Session>>) {
    spawn(async move {
        // Let the first-render column burst + the inbox ingester settle
        // before we add backfill traffic.
        tokio::time::sleep(Duration::from_secs(10)).await;

        // One-shot on launch: recompute cached follower scores so a
        // scoring-formula change (e.g. the reach ratio penalty) applies
        // to existing data immediately, not only after a re-backfill.
        if let Some(n) = db(analytics::rescore_all_follower_stats).await {
            if n > 0 {
                crate::diag!("[analytics] re-scored {n} cached follower_stats on launch");
            }
        }

        loop {
            run_analytics_cycle(session).await;
            tokio::time::sleep(Duration::from_secs(BACKFILL_CHECK_INTERVAL_SECS)).await;
        }
    });
}

async fn run_analytics_cycle(session: Signal<Option<Session>>) {
    // No session → nothing to read. Demo mode skips for the same reason
    // inbox_ingest does: there are no real records / backlinks to fetch.
    if session.read().is_none() || crate::demo::is_active() {
        return;
    }

    let Some(client) = fresh_client(session).await else {
        crate::diag!("[analytics] no fresh client this cycle");
        return;
    };

    let my_did = session
        .read()
        .as_ref()
        .map(|s| s.did.clone())
        .unwrap_or_default();
    if my_did.is_empty() {
        return;
    }

    // ── Daily snapshot (every cycle; idempotent per local day) ──
    write_daily_snapshot(&client, &my_did).await;

    // ── Backfill phase machine ──
    let mut state = match load_or_init_state().await {
        Some(s) => s,
        None => return, // DB unavailable this cycle — retry next tick.
    };

    // Kick the machine off the initial Pending state without burning a
    // whole cycle on a bare phase flip.
    if state.phase == BackfillPhase::Pending {
        state.phase = BackfillPhase::Posts;
        state.last_checkpoint_at = Utc::now();
        checkpoint(&state).await;
    }

    // Advance exactly one *work* phase per cycle. Each backfill_* drains
    // its own pages (checkpointing per page) and returns whether the
    // phase finished; on a transient error it returns false and we leave
    // the phase in place so the next cycle resumes from the checkpoint.
    match state.phase {
        BackfillPhase::Pending => {} // handled above
        BackfillPhase::Posts => {
            if backfill_posts(&client, &my_did, &mut state).await {
                advance(&mut state, BackfillPhase::Following).await;
            }
        }
        BackfillPhase::Following => {
            if backfill_following(&client, &my_did, &mut state).await {
                advance(&mut state, BackfillPhase::Followers).await;
            }
        }
        BackfillPhase::Followers => {
            if backfill_followers(&client, &my_did, &mut state).await {
                advance(&mut state, BackfillPhase::Engagement).await;
            }
        }
        BackfillPhase::Engagement => {
            // v1 no-op: scoring already seeded during the followers pass
            // and kept fresh by the inbox cycle. Mark the backfill done.
            state.completed_at = Some(Utc::now());
            advance(&mut state, BackfillPhase::Complete).await;
            crate::diag!("[analytics] backfill complete");
        }
        BackfillPhase::Complete => {}
    }
}

/// Advance the phase machine and checkpoint.
async fn advance(state: &mut BackfillState, next: BackfillPhase) {
    state.phase = next;
    state.last_checkpoint_at = Utc::now();
    checkpoint(state).await;
}

/// Read the singleton backfill state, creating a fresh `Pending` row on
/// first run. Returns `None` if the DB is unreachable this cycle.
async fn load_or_init_state() -> Option<BackfillState> {
    match db(analytics::get_backfill_state).await {
        Some(Some(s)) => Some(s),
        Some(None) => {
            let now = Utc::now();
            let fresh = BackfillState {
                phase: BackfillPhase::Pending,
                repo_posts_cursor: None,
                repo_follows_cursor: None,
                constellation_followers_cursor: None,
                started_at: now,
                last_checkpoint_at: now,
                completed_at: None,
            };
            checkpoint(&fresh).await;
            Some(fresh)
        }
        None => None,
    }
}

async fn checkpoint(state: &BackfillState) {
    let s = state.clone();
    db(move || analytics::checkpoint_backfill(&s)).await;
}

// ─────────────────────────── daily snapshot ────────────────────────

async fn write_daily_snapshot(client: &AtClient, my_did: &str) {
    let profile = match client.get_profile(my_did).await {
        Ok(p) => p,
        Err(e) => {
            crate::diag!("[analytics] getProfile failed: {e}");
            return;
        }
    };
    let followers = profile.followers_count.unwrap_or(0) as i64;
    let following = profile.follows_count.unwrap_or(0) as i64;
    let posts = profile.posts_count.unwrap_or(0) as i64;
    db(move || analytics::upsert_metric_snapshot_for_today(followers, following, posts)).await;
}

// ───────────────────────────── posts phase ─────────────────────────

/// Drain own `app.bsky.feed.post` records into `post_metrics`, then fetch
/// first-party engagement for the most-recent [`ENGAGEMENT_POST_WINDOW`]
/// posts. Returns `true` when the full history has been paged.
async fn backfill_posts(client: &AtClient, my_did: &str, state: &mut BackfillState) -> bool {
    let mut cursor = state.repo_posts_cursor.clone();
    // (ts, uri) of every post seen — sorted at the end to pick the most
    // recent N for engagement, independent of listRecords page order.
    let mut seen: Vec<(DateTime<Utc>, String)> = Vec::new();

    loop {
        let resp = match client
            .list_records(my_did, COLLECTION_POST, cursor.as_deref(), PAGE_LIMIT)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                crate::diag!("[analytics] listRecords(posts) failed: {e}");
                return false;
            }
        };
        if resp.records.is_empty() {
            break;
        }
        for rec in &resp.records {
            let rkey = rkey_from_uri(&rec.uri);
            if rkey.is_empty() {
                continue;
            }
            let ts = record_ts(rkey, &rec.value);
            let text = rec
                .value
                .get("text")
                .and_then(|v| v.as_str())
                .map(truncate_preview);
            let pm = PostMetric {
                rkey: rkey.to_string(),
                uri: rec.uri.clone(),
                ts,
                like_count: 0,
                repost_count: 0,
                reply_count: 0,
                quote_count: 0,
                text_preview: text,
            };
            db(move || analytics::upsert_post_metric(&pm)).await;
            seen.push((ts, rec.uri.clone()));
        }
        match resp.cursor {
            Some(c) => {
                state.repo_posts_cursor = Some(c.clone());
                state.last_checkpoint_at = Utc::now();
                checkpoint(state).await;
                cursor = Some(c);
            }
            None => break,
        }
    }

    // Engagement on the most-recent window only (v1 scope).
    seen.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    let recent_uris: Vec<String> = seen
        .into_iter()
        .take(ENGAGEMENT_POST_WINDOW)
        .map(|(_, uri)| uri)
        .collect();
    if !recent_uris.is_empty() {
        match client.get_posts(&recent_uris).await {
            Ok(posts) => {
                for pv in posts {
                    let rkey = rkey_from_uri(&pv.uri);
                    if rkey.is_empty() {
                        continue;
                    }
                    let ts = tid::tid_to_datetime(rkey).unwrap_or_else(Utc::now);
                    let pm = PostMetric {
                        rkey: rkey.to_string(),
                        uri: pv.uri.clone(),
                        ts,
                        like_count: pv.like_count,
                        repost_count: pv.repost_count,
                        reply_count: pv.reply_count,
                        quote_count: pv.quote_count,
                        text_preview: Some(truncate_preview(&pv.record.text)),
                    };
                    db(move || analytics::upsert_post_metric(&pm)).await;
                }
            }
            Err(e) => crate::diag!("[analytics] getPosts(engagement) failed: {e}"),
        }
    }

    state.repo_posts_cursor = None;
    true
}

// ──────────────────────────── following phase ──────────────────────

/// Drain own `app.bsky.graph.follow` records into outgoing `follow_events`.
async fn backfill_following(client: &AtClient, my_did: &str, state: &mut BackfillState) -> bool {
    let mut cursor = state.repo_follows_cursor.clone();
    loop {
        let resp = match client
            .list_records(my_did, COLLECTION_FOLLOW, cursor.as_deref(), PAGE_LIMIT)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                crate::diag!("[analytics] listRecords(follows) failed: {e}");
                return false;
            }
        };
        if resp.records.is_empty() {
            break;
        }
        for rec in &resp.records {
            let rkey = rkey_from_uri(&rec.uri);
            let Some(subject) = rec.value.get("subject").and_then(|v| v.as_str()) else {
                continue;
            };
            if rkey.is_empty() {
                continue;
            }
            let ts = record_ts(rkey, &rec.value);
            let ev = FollowEvent {
                id: follow_event_id(FollowDir::Outgoing, subject, rkey),
                direction: FollowDir::Outgoing,
                actor_did: subject.to_string(),
                actor_handle: None,
                ts,
                source: "own_repo".into(),
                rkey: Some(rkey.to_string()),
            };
            db(move || analytics::upsert_follow_event(&ev)).await;
        }
        match resp.cursor {
            Some(c) => {
                state.repo_follows_cursor = Some(c.clone());
                state.last_checkpoint_at = Utc::now();
                checkpoint(state).await;
                cursor = Some(c);
            }
            None => break,
        }
    }
    state.repo_follows_cursor = None;
    true
}

// ──────────────────────────── followers phase ──────────────────────

/// Page Constellation backlinks (`app.bsky.graph.follow` records whose
/// `.subject` is us) into incoming `follow_events`, and — per page —
/// enrich + score the followers into `follower_stats`.
async fn backfill_followers(client: &AtClient, my_did: &str, state: &mut BackfillState) -> bool {
    let constellation = ConstellationClient::new();
    let mut cursor = state.constellation_followers_cursor.clone();

    loop {
        let resp = match constellation
            .links(my_did, COLLECTION_FOLLOW, ".subject", cursor.as_deref())
            .await
        {
            Ok(r) => r,
            Err(e) => {
                crate::diag!("[analytics] constellation links failed: {e}");
                return false;
            }
        };
        if resp.linking_records.is_empty() && resp.cursor.is_none() {
            break;
        }

        // Write incoming follow_events + remember each follower's
        // follow-creation instant (their tenure start) for scoring.
        let mut dids: Vec<String> = Vec::new();
        let mut first_followed: HashMap<String, DateTime<Utc>> = HashMap::new();
        for rec in &resp.linking_records {
            let ts = tid::tid_to_datetime(&rec.rkey).unwrap_or_else(Utc::now);
            let ev = FollowEvent {
                id: follow_event_id(FollowDir::Incoming, &rec.did, &rec.rkey),
                direction: FollowDir::Incoming,
                actor_did: rec.did.clone(),
                actor_handle: None,
                ts,
                source: "constellation".into(),
                rkey: Some(rec.rkey.clone()),
            };
            db(move || analytics::upsert_follow_event(&ev)).await;
            first_followed.entry(rec.did.clone()).or_insert(ts);
            dids.push(rec.did.clone());
        }

        enrich_and_score(client, &dids, &first_followed).await;

        match resp.cursor {
            Some(c) => {
                state.constellation_followers_cursor = Some(c.clone());
                state.last_checkpoint_at = Utc::now();
                checkpoint(state).await;
                cursor = Some(c);
            }
            None => break,
        }
    }
    state.constellation_followers_cursor = None;
    true
}

/// Batched profile enrichment + clout scoring for a page of followers.
async fn enrich_and_score(
    client: &AtClient,
    dids: &[String],
    first_followed: &HashMap<String, DateTime<Utc>>,
) {
    if dids.is_empty() {
        return;
    }
    let profiles = match client.get_profiles(dids).await {
        Ok(p) => p,
        Err(e) => {
            crate::diag!("[analytics] getProfiles(followers) failed: {e}");
            return;
        }
    };

    let since = Utc::now() - chrono::Duration::days(analytics::ENGAGEMENT_WINDOW_DAYS);
    for p in profiles {
        let did = p.did.clone();
        let first_ts = first_followed.get(&did).copied().unwrap_or_else(Utc::now);

        // Engagement signals from the shared inbox store.
        let did_for_count = did.clone();
        let engagement_count =
            db(move || analytics::count_engagements_from_inbox(&did_for_count, since))
                .await
                .unwrap_or(0);
        let did_for_last = did.clone();
        let last_engagement_ts = db(move || analytics::last_engagement_from_inbox(&did_for_last))
            .await
            .flatten();

        // Mutual = we follow them back. The enriched profile's viewer
        // state carries our follow-record URI when present.
        let mutual = p
            .viewer
            .as_ref()
            .and_then(|v| v.following.as_ref())
            .is_some();

        let mut stat = FollowerStat {
            follower_did: did,
            follower_handle: p.handle.clone(),
            follower_display_name: p.display_name.clone(),
            follower_avatar: p.avatar.clone(),
            followers_count: p.followers_count.unwrap_or(0) as i64,
            following_count: p.follows_count.unwrap_or(0) as i64,
            engagement_count,
            last_engagement_ts,
            mutual,
            first_followed_ts: first_ts,
            reach_score: 0.0,
            engagement_score: 0.0,
            mutual_bonus: 0.0,
            tenure_bonus: 0.0,
            composite_score: 0.0,
        };
        crate::analytics_score::rescore(&mut stat);
        db(move || analytics::upsert_follower_stat(&stat)).await;
    }
}

// ───────────────────────────── helpers ─────────────────────────────

/// Run a blocking analytics-store closure on the blocking pool, logging
/// (and swallowing) failures so a transient DB error never kills the
/// loop. Returns `None` on panic or store error.
async fn db<T, F>(f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            crate::diag!("[analytics] db write failed: {e}");
            None
        }
        Err(e) => {
            crate::diag!("[analytics] db blocking task panicked: {e}");
            None
        }
    }
}

/// The record key — the tail path segment of an AT-URI.
fn rkey_from_uri(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap_or("")
}

/// TID-decode `rkey` to a creation instant, falling back to the record's
/// own `createdAt`, then to "now" so a malformed record still lands.
fn record_ts(rkey: &str, value: &serde_json::Value) -> DateTime<Utc> {
    if let Some(ts) = tid::tid_to_datetime(rkey) {
        return ts;
    }
    value
        .get("createdAt")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

/// Stable, deterministic `follow_events.id` = a hash of
/// `direction | actor_did | rkey`. `DefaultHasher` has a fixed (un-seeded)
/// key, so the same inputs hash identically across runs — re-ingesting a
/// follow record is an idempotent UPSERT.
fn follow_event_id(direction: FollowDir, actor_did: &str, rkey: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    direction.as_db_str().hash(&mut h);
    actor_did.hash(&mut h);
    rkey.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn truncate_preview(s: &str) -> String {
    if s.chars().count() <= PREVIEW_LEN {
        return s.to_string();
    }
    s.chars().take(PREVIEW_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rkey_extraction() {
        assert_eq!(
            rkey_from_uri("at://did:plc:me/app.bsky.feed.post/3mp5c2tkblw2p"),
            "3mp5c2tkblw2p"
        );
        assert_eq!(rkey_from_uri(""), "");
    }

    #[test]
    fn follow_event_id_is_stable_and_direction_sensitive() {
        let a = follow_event_id(FollowDir::Outgoing, "did:plc:x", "rk1");
        let b = follow_event_id(FollowDir::Outgoing, "did:plc:x", "rk1");
        assert_eq!(a, b, "same inputs → same id");
        let c = follow_event_id(FollowDir::Incoming, "did:plc:x", "rk1");
        assert_ne!(a, c, "direction participates in the id");
    }

    #[test]
    fn record_ts_prefers_tid_then_created_at() {
        // Valid TID wins regardless of createdAt.
        let v = serde_json::json!({ "createdAt": "2000-01-01T00:00:00Z" });
        let ts = record_ts("3mp5c2tkblw2p", &v);
        assert_eq!(ts.format("%Y-%m-%d").to_string(), "2026-06-25");
        // Bad rkey falls back to createdAt.
        let ts2 = record_ts("not-a-tid", &v);
        assert_eq!(ts2.format("%Y-%m-%d").to_string(), "2000-01-01");
    }

    #[test]
    fn truncate_caps_length() {
        assert_eq!(
            truncate_preview(&"x".repeat(500)).chars().count(),
            PREVIEW_LEN
        );
        assert_eq!(truncate_preview("hi"), "hi");
    }
}
