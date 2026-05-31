//! Inbox v1 — SQLite-backed triage list (pearl th-e17045).
//!
//! Combines direct replies, @mentions, quote-posts, and DMs into one
//! triage column scored by **directness**:
//!
//! | Source                                  | Base score |
//! | --------------------------------------- | ---------- |
//! | Reply to your reply (conversation-keep) | 100        |
//! | New DM                                  |  90        |
//! | Direct reply to a top-level post        |  60        |
//! | Quote-post of your post                 |  70        |
//! | @mention in someone else's post         |  30        |
//!
//! Modifiers:
//! - **Unread** bumps the score by `UNREAD_BUMP` so unread items
//!   float to the top within a directness bucket.
//! - **Age decay** subtracts up to `MAX_AGE_PENALTY` over a multi-day
//!   span so a 3-day-old direct reply still sits below a fresh quote
//!   without ever being scored to zero.
//!
//! ## Storage
//!
//! Items live in `directories::data_dir/smooblue/inbox.db` (SQLite),
//! schema below. The DB is opened on first inbox access (lazy) and
//! kept around for the app lifetime in a `OnceLock<Mutex<Connection>>`
//! — SQLite's own serialization plus our outer mutex keep things
//! sane across concurrent writes from the ingestion task and the UI
//! triage actions.
//!
//! ## Future smoo.ai sync
//!
//! Schema includes `device_id TEXT` + `synced_at INTEGER NULL` columns
//! from day 1 so a future sync layer can:
//!
//! 1. Tag each local triage action with the device that authored it
//!    (so a read on phone propagates to desktop without echoing back).
//! 2. Push unsync'd rows (`WHERE synced_at IS NULL`) to a smoo.ai
//!    endpoint + receive the server's last-write-wins state for any
//!    rows changed elsewhere. Bumping `synced_at = NOW()` after a
//!    successful push closes the loop.
//!
//! No sync code ships in v1, but the schema is sync-ready.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Schema version embedded in `PRAGMA user_version`. Bump on
/// every breaking change + add a migration step in [`migrate`].
const SCHEMA_VERSION: u32 = 2;

/// Hour-bucket granularity, in seconds. The sort uses
/// (ts_bucket DESC, follower_count DESC, directness DESC, ts DESC)
/// so a high-follower item sorts above more-direct peers WITHIN the
/// same hour but never lifts past a fresher (newer-bucket) item as
/// it ages. Pearl th-bce4fb. Make hour-bucket smaller (e.g. 1800
/// for 30 min) if "celebrity boost lingers too long" is a complaint.
const TS_BUCKET_SECS: i64 = 3600;

/// Per-source base directness scores. Higher = more "for you, now".
const SCORE_REPLY_TO_REPLY: i32 = 100;
const SCORE_DM: i32 = 90;
const SCORE_QUOTE: i32 = 70;
const SCORE_DIRECT_REPLY: i32 = 60;
const SCORE_MENTION: i32 = 30;
/// Bonus for unread items so they bubble within their directness band.
const UNREAD_BUMP: i32 = 20;
/// Max points to deduct from age. Decay rate = 1 point per 12 hours.
const MAX_AGE_PENALTY: i32 = 40;

/// Where an item came from. Drives the source-icon column + scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InboxSource {
    /// Direct reply to one of your top-level posts.
    DirectReply,
    /// Reply to one of YOUR replies — i.e. someone continuing a
    /// thread you started a sub-branch of. Highest directness.
    ReplyToReply,
    /// Quote-post that quotes one of your posts.
    Quote,
    /// @mention in a post that isn't a direct reply to you.
    Mention,
    /// New DM in a Bluesky chat convo.
    Dm,
}

impl InboxSource {
    fn base_score(self) -> i32 {
        match self {
            Self::ReplyToReply => SCORE_REPLY_TO_REPLY,
            Self::Dm => SCORE_DM,
            Self::Quote => SCORE_QUOTE,
            Self::DirectReply => SCORE_DIRECT_REPLY,
            Self::Mention => SCORE_MENTION,
        }
    }

    /// Persistence key — the string stored in the SQLite `source`
    /// column. Stable across schema versions so a future
    /// `Mention` rename in code wouldn't break old DB rows.
    fn as_db_str(self) -> &'static str {
        match self {
            Self::DirectReply => "direct_reply",
            Self::ReplyToReply => "reply_to_reply",
            Self::Quote => "quote",
            Self::Mention => "mention",
            Self::Dm => "dm",
        }
    }

    fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "direct_reply" => Some(Self::DirectReply),
            "reply_to_reply" => Some(Self::ReplyToReply),
            "quote" => Some(Self::Quote),
            "mention" => Some(Self::Mention),
            "dm" => Some(Self::Dm),
            _ => None,
        }
    }
}

/// One inbox row.
///
/// `subject_uri` is the AT-URI (post) or convo id (DM) the row points
/// at — clicking the row opens the corresponding ThreadSheet or
/// MessagesSheet. `payload` is the full upstream record (notification
/// JSON or message view) serialized for rendering without re-fetching.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxItem {
    /// SQLite primary key — hash of (source, actor_did, subject_uri,
    /// ts) so ingestion is naturally idempotent: re-polling
    /// listNotifications won't double-insert.
    pub item_id: String,
    pub source: InboxSource,
    /// DID of the actor responsible for the item (the replier, the
    /// mentioner, the quoter, the DM sender).
    pub actor_did: String,
    /// Optional handle / display name for the actor — cached at
    /// insert time so the row can render without re-fetching the
    /// actor profile. Stale handles are OK for triage UX.
    pub actor_handle: Option<String>,
    pub actor_display_name: Option<String>,
    pub actor_avatar: Option<String>,
    /// What the item points at:
    /// - For replies/quotes/mentions: AT-URI of the post.
    /// - For DMs: convo id.
    pub subject_uri: String,
    /// One-line preview text (post body / DM text), truncated to
    /// `PREVIEW_LEN` chars at ingestion time.
    pub preview: Option<String>,
    /// When the upstream event happened (post created, DM sent).
    /// Drives age-decay scoring + the time chip in the UI.
    pub ts: DateTime<Utc>,
    /// Cached directness score at last write. Recomputed periodically
    /// by the maintenance pass so the sort stays correct as items age.
    pub directness: i32,
    pub read: bool,
    pub archived: bool,
    /// If set + in the future, the row is hidden from the active
    /// list. The maintenance pass surfaces snoozed rows once
    /// `snoozed_until <= now()`.
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Device that last wrote a triage action on this row. `None`
    /// until any triage action fires. Reserved for the future
    /// smoo.ai sync layer.
    pub device_id: Option<String>,
    /// `Some(when)` once the row has been pushed to smoo.ai; `None`
    /// for rows that have never been sync'd. Sync layer queries
    /// `WHERE synced_at IS NULL` for the push set.
    pub synced_at: Option<DateTime<Utc>>,
    /// Raw upstream JSON for re-rendering (deserialized into the
    /// notification view or the chat MessageView depending on `source`).
    /// Stored as TEXT (sqlite has no native JSON type for our purposes).
    pub payload_json: String,
    /// Cached follower count of `actor_did` at last ingestion. Drives
    /// the within-bucket sort tiebreak (pearl th-bce4fb) — items in
    /// the same hour bucket sort by follower count first, then by
    /// directness. 0 for actors we haven't fetched a profile for yet
    /// (collapses to a directness-only sort within their bucket).
    pub actor_follower_count: i64,
    /// Epoch hour of `ts` (i.e. `ts.timestamp() / 3600`). Stored at
    /// insert time rather than computed via SQLite's strftime so the
    /// index is a stable integer and there's no parser coupling at
    /// query time. Drives the primary sort dimension.
    pub ts_bucket: i64,
}

/// Compute the hour bucket for a timestamp. Wrap as a function so
/// the bucketing rule lives in one place — bumping
/// [`TS_BUCKET_SECS`] re-buckets everything on the next ingestion.
pub fn ts_bucket_for(ts: DateTime<Utc>) -> i64 {
    ts.timestamp() / TS_BUCKET_SECS
}

/// Compute the live directness score given current age + read state.
/// Use this when refreshing the cached `directness` column.
pub fn score(source: InboxSource, ts: DateTime<Utc>, read: bool) -> i32 {
    let now = Utc::now();
    let hours_old = (now - ts).num_hours().max(0);
    // 1 point off per 12 hours of age, up to MAX_AGE_PENALTY.
    let age_penalty = (hours_old / 12).min(MAX_AGE_PENALTY as i64) as i32;
    let unread_bump = if read { 0 } else { UNREAD_BUMP };
    source.base_score() + unread_bump - age_penalty
}

/// Process-wide connection slot. `Option<Connection>` (not
/// `Connection` directly) so a transient disk failure on first
/// access doesn't permanently lock out the inbox: subsequent calls
/// retry the open. SQLite serializes writes internally; the outer
/// mutex prevents two Rust callers from interleaving statements
/// inside a transaction. Adversarial-review P2 — replaced an earlier
/// `OnceLock<Mutex<Connection>>` that silently downgraded to
/// `:memory:` on disk-open failure (silent data loss).
static DB: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();

fn db_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("ai", "Smoo", "smooblue")
        .ok_or_else(|| anyhow!("ProjectDirs unavailable on this platform"))?;
    let data_dir = dirs.data_dir();
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    Ok(data_dir.join("inbox.db"))
}

fn open() -> Result<Connection> {
    let path = db_path()?;
    let conn = Connection::open(&path)
        .with_context(|| format!("opening inbox DB at {}", path.display()))?;
    // WAL = better concurrent read while writer holds a lock; perfect
    // for the "ingestion task writes, UI reads" pattern.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Lazily-initialized DB accessor. First call opens + migrates; on
/// failure the slot is left empty and the next call retries (a
/// transient permission glitch or filesystem hiccup self-heals).
/// All errors propagate to the caller — no silent in-memory fallback
/// that would let triage actions look like they landed when they
/// didn't. Adversarial-review P2 fix.
fn with_db<R>(f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
    let slot = DB.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock();
    if guard.is_none() {
        *guard = Some(open()?);
    }
    // Unwrap is safe: we just set the slot to Some above, and the
    // mutex guard prevents anyone else from setting it back to None.
    f(guard.as_ref().expect("inbox connection set above"))
}

/// Apply pending migrations. Versioned via `PRAGMA user_version`.
fn migrate(conn: &Connection) -> Result<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    if current < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE inbox_items (
                item_id          TEXT PRIMARY KEY,
                source           TEXT NOT NULL,
                actor_did        TEXT NOT NULL,
                actor_handle     TEXT,
                actor_display    TEXT,
                actor_avatar     TEXT,
                subject_uri      TEXT NOT NULL,
                preview          TEXT,
                ts               TEXT NOT NULL,
                directness       INTEGER NOT NULL,
                read             INTEGER NOT NULL DEFAULT 0,
                archived         INTEGER NOT NULL DEFAULT 0,
                snoozed_until    TEXT,
                device_id        TEXT,
                synced_at        TEXT,
                payload_json     TEXT NOT NULL
            );

            -- Active-list query goes via directness DESC, ts DESC.
            CREATE INDEX inbox_active_idx
                ON inbox_items(directness DESC, ts DESC)
                WHERE archived = 0;

            -- Sync layer queries unsync'd rows.
            CREATE INDEX inbox_unsynced_idx
                ON inbox_items(synced_at)
                WHERE synced_at IS NULL;
            "#,
        )?;
    }
    if current < 2 {
        // v2: follower-count tiebreak within hour-bucketed time.
        // Drop the old active-list index — it's replaced with a
        // composite that matches the new ORDER BY exactly. Existing
        // rows get follower_count=0 + ts_bucket=0 from the DEFAULTs;
        // next ingestion cycle UPSERTs them with real values. In the
        // meantime they all collapse into the "bucket 0" sort cell,
        // which is below every newer-bucket item — annoying for ~30s
        // but the only data-loss-free migration.
        conn.execute_batch(
            r#"
            ALTER TABLE inbox_items ADD COLUMN actor_follower_count INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE inbox_items ADD COLUMN ts_bucket INTEGER NOT NULL DEFAULT 0;
            DROP INDEX IF EXISTS inbox_active_idx;
            CREATE INDEX inbox_active_idx
                ON inbox_items(
                    ts_bucket DESC,
                    actor_follower_count DESC,
                    directness DESC,
                    ts DESC
                )
                WHERE archived = 0;
            "#,
        )?;
    }
    // Future migrations: `if current < 3 { ... }` etc.
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Insert (or update on conflict) a single inbox item. UPSERT
/// semantics so re-polling listNotifications doesn't double-insert
/// but DOES refresh `payload_json` if the upstream item changed.
/// Triage state (read/archived/snoozed_until) is preserved on
/// re-ingestion via COALESCE on the existing row.
pub fn upsert(item: &InboxItem) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO inbox_items (
                item_id, source, actor_did, actor_handle, actor_display,
                actor_avatar, subject_uri, preview, ts, directness,
                read, archived, snoozed_until, device_id, synced_at,
                payload_json, actor_follower_count, ts_bucket
            )
            VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18
            )
            ON CONFLICT(item_id) DO UPDATE SET
                actor_handle         = excluded.actor_handle,
                actor_display        = excluded.actor_display,
                actor_avatar         = excluded.actor_avatar,
                preview              = excluded.preview,
                directness           = excluded.directness,
                payload_json         = excluded.payload_json,
                -- Re-ingestion CAN raise but never SILENTLY LOWERS the
                -- cached follower count — protects against a transient
                -- profile-fetch failure (which would write 0) blowing
                -- away a real cached value.
                actor_follower_count = MAX(actor_follower_count, excluded.actor_follower_count),
                ts_bucket            = excluded.ts_bucket
            "#,
            params![
                item.item_id,
                item.source.as_db_str(),
                item.actor_did,
                item.actor_handle,
                item.actor_display_name,
                item.actor_avatar,
                item.subject_uri,
                item.preview,
                item.ts.to_rfc3339(),
                item.directness,
                item.read as i32,
                item.archived as i32,
                item.snoozed_until.map(|t| t.to_rfc3339()),
                item.device_id,
                item.synced_at.map(|t| t.to_rfc3339()),
                item.payload_json,
                item.actor_follower_count,
                item.ts_bucket,
            ],
        )?;
        Ok(())
    })
}

/// Update the cached follower count for every row authored by
/// `actor_did`. Called by the ingestion task's profile-enrichment
/// pass after `get_profiles` returns. Idempotent + cheap — single
/// UPDATE indexed on actor_did is unaffordably fast even with thousands
/// of rows.
pub fn set_actor_followers(actor_did: &str, count: i64) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            "UPDATE inbox_items SET actor_follower_count = ?1 WHERE actor_did = ?2",
            params![count, actor_did],
        )?;
        Ok(())
    })
}

/// Top of the active (non-archived, not currently snoozed) list,
/// sorted by directness DESC, ts DESC. `limit` caps the read for the
/// UI; pass a generous value like 200 — the index covers it cheaply.
pub fn list_active(limit: i64) -> Result<Vec<InboxItem>> {
    with_db(|conn| {
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            r#"
            SELECT item_id, source, actor_did, actor_handle, actor_display,
                   actor_avatar, subject_uri, preview, ts, directness,
                   read, archived, snoozed_until, device_id, synced_at,
                   payload_json, actor_follower_count, ts_bucket
            FROM inbox_items
            WHERE archived = 0
              AND (snoozed_until IS NULL OR snoozed_until <= ?1)
            -- Pearl th-bce4fb: hour-bucket first, then follower count
            -- within bucket (so a celebrity's mention floats above a
            -- friend's deep-thread reply that arrived in the same
            -- hour), then directness, then ts as a final stable
            -- tiebreak. Idempotent + stable: as items age into older
            -- buckets, the follower bump no longer lifts them past
            -- newer arrivals.
            ORDER BY ts_bucket DESC,
                     actor_follower_count DESC,
                     directness DESC,
                     ts DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![now, limit], row_to_item)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

/// Mark an item as read or unread. Local-only — no network call.
/// Sets `synced_at = NULL` so the future sync layer ships the change.
pub fn set_read(item_id: &str, read: bool) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            "UPDATE inbox_items SET read = ?1, synced_at = NULL WHERE item_id = ?2",
            params![read as i32, item_id],
        )?;
        Ok(())
    })
}

/// Archive (or unarchive) an item — hides from the active list.
pub fn set_archived(item_id: &str, archived: bool) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            "UPDATE inbox_items SET archived = ?1, synced_at = NULL WHERE item_id = ?2",
            params![archived as i32, item_id],
        )?;
        Ok(())
    })
}

/// Snooze an item until the given timestamp (None = clear snooze).
pub fn set_snoozed(item_id: &str, until: Option<DateTime<Utc>>) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            "UPDATE inbox_items SET snoozed_until = ?1, synced_at = NULL WHERE item_id = ?2",
            params![until.map(|t| t.to_rfc3339()), item_id],
        )?;
        Ok(())
    })
}

/// Count unread+non-archived+non-snoozed items — for a future badge
/// on the rail's Inbox button.
pub fn unread_count() -> Result<u32> {
    with_db(|conn| {
        let now = Utc::now().to_rfc3339();
        let n: i64 = conn.query_row(
            r#"
            SELECT COUNT(*)
            FROM inbox_items
            WHERE read = 0
              AND archived = 0
              AND (snoozed_until IS NULL OR snoozed_until <= ?1)
            "#,
            params![now],
            |row| row.get(0),
        )?;
        Ok(n as u32)
    })
}

/// Look up an item by id — used when a triage action needs to read
/// the full row before mutating (e.g. "is this already archived?").
pub fn get(item_id: &str) -> Result<Option<InboxItem>> {
    with_db(|conn| {
        let item = conn
            .query_row(
                r#"
                SELECT item_id, source, actor_did, actor_handle, actor_display,
                       actor_avatar, subject_uri, preview, ts, directness,
                       read, archived, snoozed_until, device_id, synced_at,
                       payload_json, actor_follower_count, ts_bucket
                FROM inbox_items
                WHERE item_id = ?1
                "#,
                params![item_id],
                row_to_item,
            )
            .optional()?;
        Ok(item)
    })
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<InboxItem> {
    let ts_str: String = row.get("ts")?;
    let ts = DateTime::parse_from_rfc3339(&ts_str)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc);
    let snoozed_str: Option<String> = row.get("snoozed_until")?;
    let snoozed_until = snoozed_str
        .as_deref()
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let synced_str: Option<String> = row.get("synced_at")?;
    let synced_at = synced_str
        .as_deref()
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(14, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let source_str: String = row.get("source")?;
    let source = InboxSource::from_db_str(&source_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown inbox source: {source_str}"),
            )),
        )
    })?;
    Ok(InboxItem {
        item_id: row.get("item_id")?,
        source,
        actor_did: row.get("actor_did")?,
        actor_handle: row.get("actor_handle")?,
        actor_display_name: row.get("actor_display")?,
        actor_avatar: row.get("actor_avatar")?,
        subject_uri: row.get("subject_uri")?,
        preview: row.get("preview")?,
        ts,
        directness: row.get("directness")?,
        read: {
            let v: i32 = row.get("read")?;
            v != 0
        },
        archived: {
            let v: i32 = row.get("archived")?;
            v != 0
        },
        snoozed_until,
        device_id: row.get("device_id")?,
        synced_at,
        payload_json: row.get("payload_json")?,
        actor_follower_count: row.get("actor_follower_count")?,
        ts_bucket: row.get("ts_bucket")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: schema applies + UPSERT round-trips + list_active
    /// returns the row sorted by directness.
    #[test]
    fn inbox_roundtrip_in_memory() {
        // Force the in-memory fallback by pre-poisoning the cache
        // with a fresh in-memory conn. Avoids touching the user's
        // real DB during tests.
        let conn = Connection::open_in_memory().expect("open");
        migrate(&conn).expect("migrate");
        let _ = DB.set(Mutex::new(Some(conn)));

        let now = Utc::now();
        let item = InboxItem {
            item_id: "item-1".into(),
            source: InboxSource::DirectReply,
            actor_did: "did:plc:alice".into(),
            actor_handle: Some("alice.bsky.social".into()),
            actor_display_name: Some("Alice".into()),
            actor_avatar: None,
            subject_uri: "at://did:plc:me/app.bsky.feed.post/abc".into(),
            preview: Some("Sounds good!".into()),
            ts: now,
            directness: score(InboxSource::DirectReply, now, false),
            read: false,
            archived: false,
            snoozed_until: None,
            device_id: None,
            synced_at: None,
            payload_json: "{}".into(),
            actor_follower_count: 0,
            ts_bucket: ts_bucket_for(now),
        };
        upsert(&item).expect("upsert");

        let fetched = get("item-1").expect("get").expect("found");
        assert_eq!(fetched.actor_did, "did:plc:alice");
        assert_eq!(fetched.source, InboxSource::DirectReply);
        assert_eq!(fetched.preview.as_deref(), Some("Sounds good!"));

        let active = list_active(50).expect("list");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].item_id, "item-1");

        set_read("item-1", true).expect("set_read");
        let after_read = get("item-1").expect("get").expect("found");
        assert!(after_read.read);

        set_archived("item-1", true).expect("set_archived");
        let after_archive = list_active(50).expect("list");
        assert!(after_archive.is_empty(), "archived item must be hidden");
    }

    #[test]
    fn directness_unread_bumps_within_band() {
        let now = Utc::now();
        let unread = score(InboxSource::DirectReply, now, false);
        let read = score(InboxSource::DirectReply, now, true);
        assert!(unread > read, "unread must outrank read at same age");
        assert_eq!(unread - read, UNREAD_BUMP);
    }

    #[test]
    fn directness_age_penalty_caps() {
        let now = Utc::now();
        let very_old = now - chrono::Duration::days(365);
        let score_now = score(InboxSource::DirectReply, now, false);
        let score_old = score(InboxSource::DirectReply, very_old, false);
        assert!(score_old < score_now);
        assert!(
            score_now - score_old <= MAX_AGE_PENALTY,
            "age penalty must cap at {MAX_AGE_PENALTY}"
        );
    }

    #[test]
    fn source_db_str_roundtrips() {
        for s in [
            InboxSource::DirectReply,
            InboxSource::ReplyToReply,
            InboxSource::Quote,
            InboxSource::Mention,
            InboxSource::Dm,
        ] {
            assert_eq!(InboxSource::from_db_str(s.as_db_str()), Some(s));
        }
    }
}
