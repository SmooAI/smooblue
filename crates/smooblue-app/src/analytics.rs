//! Account analytics — storage layer (pearl: account-analytics).
//!
//! This module owns the SQLite **schema and CRUD** for the analytics
//! feature. It deliberately does *not* open its own database: per the
//! integration contract there is **one SQLite file, one connection
//! slot, one `PRAGMA user_version`**. `PRAGMA user_version` is a
//! per-*file* value, so two independent version schemes against the
//! same file would corrupt each other's migration gating. Instead:
//!
//! - The tables are created by [`create_tables_v5`], which is invoked
//!   from `inbox.rs::migrate()` under the unified `user_version` bump
//!   4 → 5. `create_tables_v5` runs one `execute_batch` and **does not**
//!   touch `user_version` (inbox.rs writes it once at the end).
//! - Every read/write here reuses [`crate::inbox::with_db`], the same
//!   process-wide `OnceLock<Mutex<Option<Connection>>>` slot the inbox
//!   uses. SQLite's internal serialization plus the outer mutex keep
//!   concurrent writes (ingestion task) and reads (UI) consistent.
//!
//! All timestamps are stored as RFC3339 TEXT to match `inbox.rs`. Window
//! queries compute their cutoff in Rust (`Utc::now() - Duration`) and
//! bind it as a parameter — never `datetime('now', …)`, which yields
//! `'YYYY-MM-DD HH:MM:SS'` and mis-compares against `'…T…Z'` RFC3339.
//!
//! ## What lives here
//!
//! - [`create_tables_v5`] — the v5 schema (5 tables + indexes).
//! - Store structs: [`MetricSnapshot`], [`FollowEvent`], [`PostMetric`],
//!   [`FollowerStat`], [`BackfillState`] (+ [`FollowDir`] / [`BackfillPhase`]).
//! - Idempotent UPSERT writers + the backfill phase-machine checkpoint.
//! - Read helpers for the growth curves, post timeline, and the four
//!   ranked follower cuts the scoring view consumes.
//! - Pure bucketing helpers ([`cadence_cells`], [`posts_per_day`]) used
//!   by the (later) view-aggregation layer — unit-tested here.

use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::inbox::with_db;

/// Engagement recency window. Inbox interactions older than this don't
/// count toward a follower's engagement score. Bound is computed in
/// Rust and bound as a parameter (never `datetime('now', …)`).
pub const ENGAGEMENT_WINDOW_DAYS: i64 = 90;

// ───────────────────────────── structs ─────────────────────────────

/// Direction of a follow edge relative to the signed-in account.
///
/// `Outgoing` = we follow them (sourced from our own repo's
/// `app.bsky.graph.follow` records). `Incoming` = they follow us
/// (sourced from Constellation backlinks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowDir {
    Outgoing,
    Incoming,
}

impl FollowDir {
    /// Persistence key written to the `follow_events.direction` column.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Outgoing => "outgoing",
            Self::Incoming => "incoming",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "outgoing" => Some(Self::Outgoing),
            "incoming" => Some(Self::Incoming),
            _ => None,
        }
    }
}

/// Backfill phase machine state. Phases advance
/// `pending → posts → following → followers → engagement → complete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillPhase {
    Pending,
    Posts,
    Following,
    Followers,
    Engagement,
    Complete,
}

impl BackfillPhase {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Posts => "posts",
            Self::Following => "following",
            Self::Followers => "followers",
            Self::Engagement => "engagement",
            Self::Complete => "complete",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "posts" => Some(Self::Posts),
            "following" => Some(Self::Following),
            "followers" => Some(Self::Followers),
            "engagement" => Some(Self::Engagement),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }

    /// Monotonic rank for "has the backfill reached past phase X yet?"
    /// comparisons that drive the per-card loading states in the view.
    pub fn rank(self) -> u8 {
        match self {
            Self::Pending => 0,
            Self::Posts => 1,
            Self::Following => 2,
            Self::Followers => 3,
            Self::Engagement => 4,
            Self::Complete => 5,
        }
    }
}

/// One day's snapshot of the *displayed* counts from `getProfile`.
/// Keyed by local-day date string so re-running on the same day is a
/// no-op (idempotent forward record of net displayed counts).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricSnapshot {
    pub snapshot_date: String,
    pub ts: DateTime<Utc>,
    pub followers_count: i64,
    pub following_count: i64,
    pub posts_count: i64,
}

/// A single still-existing follow record, TID-dated. Reconstructs the
/// growth curve when aggregated cumulatively over `ts`.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowEvent {
    /// `hash(direction|actor_did|rkey)` — stable, so re-ingesting the
    /// same follow record is an idempotent UPSERT.
    pub id: String,
    pub direction: FollowDir,
    pub actor_did: String,
    pub actor_handle: Option<String>,
    pub ts: DateTime<Utc>,
    pub source: String,
    pub rkey: Option<String>,
}

/// One of our own posts plus its first-party engagement snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct PostMetric {
    pub rkey: String,
    pub uri: String,
    pub ts: DateTime<Utc>,
    pub like_count: i64,
    pub repost_count: i64,
    pub reply_count: i64,
    pub quote_count: i64,
    pub text_preview: Option<String>,
}

/// Follower clout cache — the scoring system of record. `tenure_bonus`
/// is **not** persisted (no column); it is recomputed at read time and
/// folded into `composite_score`, so the row mapper sets it to 0.0.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowerStat {
    pub follower_did: String,
    pub follower_handle: String,
    pub follower_display_name: Option<String>,
    pub follower_avatar: Option<String>,
    pub followers_count: i64,
    pub following_count: i64,
    pub engagement_count: i64,
    pub last_engagement_ts: Option<DateTime<Utc>>,
    pub mutual: bool,
    pub first_followed_ts: DateTime<Utc>,
    pub reach_score: f64,
    pub engagement_score: f64,
    pub mutual_bonus: f64,
    /// Recomputed at read time, **not** stored.
    pub tenure_bonus: f64,
    pub composite_score: f64,
}

/// Singleton backfill phase machine (key `'global'` in v1).
#[derive(Debug, Clone, PartialEq)]
pub struct BackfillState {
    pub phase: BackfillPhase,
    pub repo_posts_cursor: Option<String>,
    pub repo_follows_cursor: Option<String>,
    pub constellation_followers_cursor: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_checkpoint_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

// ──────────────────────────── schema ───────────────────────────────

/// Create the v5 analytics tables. Invoked from `inbox.rs::migrate()`
/// inside the unified `user_version` bump — runs one `execute_batch`
/// and does **not** touch `user_version` (inbox owns the version).
pub(crate) fn create_tables_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Daily forward snapshot of the *displayed* counts from getProfile.
        CREATE TABLE metric_snapshots (
            snapshot_date    TEXT PRIMARY KEY,   -- 'YYYY-MM-DD' (local day), idempotent per day
            ts               TEXT NOT NULL,      -- RFC3339 capture instant
            followers_count  INTEGER NOT NULL,
            following_count  INTEGER NOT NULL,
            posts_count      INTEGER NOT NULL DEFAULT 0,
            created_at       TEXT NOT NULL
        );

        -- Reconstructed growth curve. One row per still-existing follow record.
        -- outgoing = we follow them (own repo); incoming = they follow us (constellation).
        CREATE TABLE follow_events (
            id           TEXT PRIMARY KEY,       -- hash(direction|actor_did|rkey)
            direction    TEXT NOT NULL,          -- 'outgoing' | 'incoming'
            actor_did    TEXT NOT NULL,
            actor_handle TEXT,
            ts           TEXT NOT NULL,          -- RFC3339, TID-decoded follow creation time
            source       TEXT NOT NULL,          -- 'own_repo' | 'constellation'
            rkey         TEXT,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX follow_events_direction_ts_idx ON follow_events(direction, ts);
        CREATE UNIQUE INDEX follow_events_source_rkey_idx ON follow_events(source, rkey) WHERE rkey IS NOT NULL;
        CREATE INDEX follow_events_actor_idx ON follow_events(actor_did);

        -- Own posts over time + first-party engagement snapshot (getPosts).
        CREATE TABLE post_metrics (
            rkey         TEXT PRIMARY KEY,
            uri          TEXT NOT NULL,
            ts           TEXT NOT NULL,          -- RFC3339, TID-decoded post creation time
            like_count   INTEGER NOT NULL DEFAULT 0,
            repost_count INTEGER NOT NULL DEFAULT 0,
            reply_count  INTEGER NOT NULL DEFAULT 0,
            quote_count  INTEGER NOT NULL DEFAULT 0,
            text_preview TEXT,
            indexed_at   TEXT NOT NULL
        );
        CREATE INDEX post_metrics_ts_idx ON post_metrics(ts);
        CREATE INDEX post_metrics_like_idx ON post_metrics(like_count DESC);

        -- Follower clout cache (scoring system of record).
        CREATE TABLE follower_stats (
            follower_did          TEXT PRIMARY KEY,
            follower_handle       TEXT NOT NULL,
            follower_display_name TEXT,
            follower_avatar       TEXT,
            followers_count       INTEGER NOT NULL DEFAULT 0,  -- the follower's reach
            following_count       INTEGER NOT NULL DEFAULT 0,
            engagement_count      INTEGER NOT NULL DEFAULT 0,
            last_engagement_ts    TEXT,
            mutual                INTEGER NOT NULL DEFAULT 0,   -- 0/1
            first_followed_ts     TEXT NOT NULL,                -- RFC3339, TID-decoded
            reach_score           REAL NOT NULL DEFAULT 0.0,
            engagement_score      REAL NOT NULL DEFAULT 0.0,
            mutual_bonus          REAL NOT NULL DEFAULT 0.0,
            composite_score       REAL NOT NULL DEFAULT 0.0,    -- tenure folded in at compute time
            profile_fetched_at    TEXT,
            stats_computed_at     TEXT NOT NULL
        );
        CREATE INDEX follower_stats_composite_idx ON follower_stats(composite_score DESC, followers_count DESC);
        CREATE INDEX follower_stats_engagement_idx ON follower_stats(engagement_count DESC);
        CREATE INDEX follower_stats_mutual_reach_idx ON follower_stats(mutual DESC, followers_count DESC) WHERE mutual = 1;

        -- Singleton backfill phase machine.
        CREATE TABLE backfill_state (
            key                            TEXT PRIMARY KEY,    -- always 'global' in v1
            phase                          TEXT NOT NULL DEFAULT 'pending',
            repo_posts_cursor              TEXT,
            repo_follows_cursor            TEXT,
            constellation_followers_cursor TEXT,
            started_at                     TEXT NOT NULL,
            last_checkpoint_at             TEXT NOT NULL,
            completed_at                   TEXT
        );
        "#,
    )?;
    Ok(())
}

// ──────────────────── timestamp parse helper ───────────────────────

fn parse_rfc3339(s: &str, col: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
        })
}

fn parse_rfc3339_opt(s: Option<String>, col: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    s.as_deref().map(|v| parse_rfc3339(v, col)).transpose()
}

// ───────────────────────── writes (UPSERT) ─────────────────────────

/// Upsert today's metric snapshot, keyed by the local-day date string.
/// Idempotent per day — re-running the same day refreshes counts but
/// keeps one row.
pub fn upsert_metric_snapshot_for_today(followers: i64, following: i64, posts: i64) -> Result<()> {
    let now = Utc::now();
    let snapshot_date = now.format("%Y-%m-%d").to_string();
    with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO metric_snapshots
                (snapshot_date, ts, followers_count, following_count, posts_count, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(snapshot_date) DO UPDATE SET
                ts              = excluded.ts,
                followers_count = excluded.followers_count,
                following_count = excluded.following_count,
                posts_count     = excluded.posts_count
            "#,
            params![
                snapshot_date,
                now.to_rfc3339(),
                followers,
                following,
                posts,
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    })
}

/// Upsert a single follow event. Keyed by `id` so re-ingesting the same
/// follow record is a no-op refresh.
pub fn upsert_follow_event(e: &FollowEvent) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO follow_events
                (id, direction, actor_did, actor_handle, ts, source, rkey, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                actor_handle = excluded.actor_handle,
                ts           = excluded.ts,
                source       = excluded.source,
                rkey         = excluded.rkey
            "#,
            params![
                e.id,
                e.direction.as_db_str(),
                e.actor_did,
                e.actor_handle,
                e.ts.to_rfc3339(),
                e.source,
                e.rkey,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    })
}

/// Upsert a single post metric, keyed by `rkey`. Refreshes engagement
/// counts on re-ingestion.
pub fn upsert_post_metric(p: &PostMetric) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO post_metrics
                (rkey, uri, ts, like_count, repost_count, reply_count, quote_count, text_preview, indexed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(rkey) DO UPDATE SET
                uri          = excluded.uri,
                ts           = excluded.ts,
                like_count   = excluded.like_count,
                repost_count = excluded.repost_count,
                reply_count  = excluded.reply_count,
                quote_count  = excluded.quote_count,
                text_preview = excluded.text_preview,
                indexed_at   = excluded.indexed_at
            "#,
            params![
                p.rkey,
                p.uri,
                p.ts.to_rfc3339(),
                p.like_count,
                p.repost_count,
                p.reply_count,
                p.quote_count,
                p.text_preview,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    })
}

/// Upsert a follower clout row, keyed by `follower_did`. `tenure_bonus`
/// is not persisted — it is recomputed at read and folded into
/// `composite_score`.
pub fn upsert_follower_stat(s: &FollowerStat) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO follower_stats (
                follower_did, follower_handle, follower_display_name, follower_avatar,
                followers_count, following_count, engagement_count, last_engagement_ts,
                mutual, first_followed_ts, reach_score, engagement_score, mutual_bonus,
                composite_score, profile_fetched_at, stats_computed_at
            )
            VALUES (
                ?1, ?2, ?3, ?4,
                ?5, ?6, ?7, ?8,
                ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16
            )
            ON CONFLICT(follower_did) DO UPDATE SET
                follower_handle       = excluded.follower_handle,
                follower_display_name = excluded.follower_display_name,
                follower_avatar       = excluded.follower_avatar,
                followers_count       = excluded.followers_count,
                following_count       = excluded.following_count,
                engagement_count      = excluded.engagement_count,
                last_engagement_ts    = excluded.last_engagement_ts,
                mutual                = excluded.mutual,
                reach_score           = excluded.reach_score,
                engagement_score      = excluded.engagement_score,
                mutual_bonus          = excluded.mutual_bonus,
                composite_score       = excluded.composite_score,
                profile_fetched_at    = excluded.profile_fetched_at,
                stats_computed_at     = excluded.stats_computed_at
            "#,
            params![
                s.follower_did,
                s.follower_handle,
                s.follower_display_name,
                s.follower_avatar,
                s.followers_count,
                s.following_count,
                s.engagement_count,
                s.last_engagement_ts.map(|t| t.to_rfc3339()),
                s.mutual as i32,
                s.first_followed_ts.to_rfc3339(),
                s.reach_score,
                s.engagement_score,
                s.mutual_bonus,
                s.composite_score,
                Utc::now().to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    })
}

// ───────────────────────── backfill state ──────────────────────────

const BACKFILL_KEY: &str = "global";

/// Read the singleton backfill state. `None` before the first checkpoint.
pub fn get_backfill_state() -> Result<Option<BackfillState>> {
    with_db(|conn| {
        let state = conn
            .query_row(
                r#"
                SELECT phase, repo_posts_cursor, repo_follows_cursor,
                       constellation_followers_cursor, started_at,
                       last_checkpoint_at, completed_at
                FROM backfill_state
                WHERE key = ?1
                "#,
                params![BACKFILL_KEY],
                row_to_backfill_state,
            )
            .optional()?;
        Ok(state)
    })
}

/// Upsert the singleton backfill state — crash-resume checkpoint.
pub fn checkpoint_backfill(s: &BackfillState) -> Result<()> {
    with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO backfill_state (
                key, phase, repo_posts_cursor, repo_follows_cursor,
                constellation_followers_cursor, started_at,
                last_checkpoint_at, completed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(key) DO UPDATE SET
                phase                          = excluded.phase,
                repo_posts_cursor              = excluded.repo_posts_cursor,
                repo_follows_cursor            = excluded.repo_follows_cursor,
                constellation_followers_cursor = excluded.constellation_followers_cursor,
                last_checkpoint_at             = excluded.last_checkpoint_at,
                completed_at                   = excluded.completed_at
            "#,
            params![
                BACKFILL_KEY,
                s.phase.as_db_str(),
                s.repo_posts_cursor,
                s.repo_follows_cursor,
                s.constellation_followers_cursor,
                s.started_at.to_rfc3339(),
                s.last_checkpoint_at.to_rfc3339(),
                s.completed_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    })
}

// ───────────────────── reads for growth / view ─────────────────────

/// All daily snapshots, ascending by date (oldest first) — the growth
/// line chart consumes them in order.
pub fn list_metric_snapshots(limit: i64) -> Result<Vec<MetricSnapshot>> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            r#"
            SELECT snapshot_date, ts, followers_count, following_count, posts_count
            FROM metric_snapshots
            ORDER BY snapshot_date ASC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit], row_to_metric_snapshot)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

/// Cumulative count of follow events in `direction` whose TID-decoded
/// `ts` is at or before `until`. Drives each point on the reconstructed
/// growth curve. Cutoff is bound as an RFC3339 parameter.
/// Earliest follow-event timestamp across both directions, or `None`
/// when no follow records have been ingested yet. Anchors the
/// full-history growth curve at the account's first follow rather than
/// an arbitrary fixed window.
pub fn earliest_follow_event_ts() -> Result<Option<DateTime<Utc>>> {
    with_db(|conn| {
        let s: Option<String> =
            conn.query_row("SELECT MIN(ts) FROM follow_events", [], |r| r.get(0))?;
        Ok(s.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }))
    })
}

/// One `(label, cutoff)` per calendar month from `from`'s month through
/// `to`'s month. `cutoff` is the first instant of the *following* month
/// so a cumulative `count(... ts <= cutoff)` includes that whole month.
/// Labels are `YYYY-MM`. Used to sample the growth curve monthly across
/// the full account history (the one-off chart did the same).
fn month_cutoffs(from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<(String, DateTime<Utc>)> {
    let mut out = Vec::new();
    let (mut y, mut m) = (from.year(), from.month());
    loop {
        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        if let Some(cutoff) = Utc.with_ymd_and_hms(ny, nm, 1, 0, 0, 0).single() {
            out.push((format!("{y:04}-{m:02}"), cutoff));
        }
        if (y, m) >= (to.year(), to.month()) {
            break;
        }
        (y, m) = (ny, nm);
        // Safety stop: never emit more than ~50 years of months.
        if out.len() > 600 {
            break;
        }
    }
    out
}

/// Full-history monthly cumulative growth curves. Returns
/// `(month_labels, followers_cumulative, following_cumulative)`. Empty
/// vecs when no follow events exist yet (view shows a loading state).
fn growth_series_monthly(now: DateTime<Utc>) -> Result<(Vec<String>, Vec<f64>, Vec<f64>)> {
    let Some(earliest) = earliest_follow_event_ts()? else {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    };
    let months = month_cutoffs(earliest, now);
    let mut labels = Vec::with_capacity(months.len());
    let mut followers = Vec::with_capacity(months.len());
    let mut following = Vec::with_capacity(months.len());
    for (label, cutoff) in months {
        labels.push(label);
        followers.push(count_follow_events_until(FollowDir::Incoming, cutoff)? as f64);
        following.push(count_follow_events_until(FollowDir::Outgoing, cutoff)? as f64);
    }
    Ok((labels, followers, following))
}

pub fn count_follow_events_until(direction: FollowDir, until: DateTime<Utc>) -> Result<i64> {
    with_db(|conn| {
        let n: i64 = conn.query_row(
            r#"
            SELECT COUNT(*)
            FROM follow_events
            WHERE direction = ?1 AND ts <= ?2
            "#,
            params![direction.as_db_str(), until.to_rfc3339()],
            |row| row.get(0),
        )?;
        Ok(n)
    })
}

/// Posts whose `ts` falls in `[from, to]`, ascending. Bounds are
/// Rust-computed RFC3339 parameters (never `datetime('now', …)`).
pub fn list_posts_in_range(from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<PostMetric>> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            r#"
            SELECT rkey, uri, ts, like_count, repost_count, reply_count, quote_count, text_preview
            FROM post_metrics
            WHERE ts >= ?1 AND ts <= ?2
            ORDER BY ts ASC
            "#,
        )?;
        let rows = stmt.query_map(
            params![from.to_rfc3339(), to.to_rfc3339()],
            row_to_post_metric,
        )?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

/// Top posts by like count, descending.
pub fn list_top_posts(limit: i64) -> Result<Vec<PostMetric>> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            r#"
            SELECT rkey, uri, ts, like_count, repost_count, reply_count, quote_count, text_preview
            FROM post_metrics
            ORDER BY like_count DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit], row_to_post_metric)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

// ───────────────────── ranked follower cuts ────────────────────────

/// Helper: run a `follower_stats` SELECT with a single `LIMIT` bind.
fn follower_stats_query(sql: &str, limit: i64) -> Result<Vec<FollowerStat>> {
    with_db(|conn| {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit], row_to_follower_stat)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

/// Your biggest fans — most inbox engagements, descending.
pub fn list_top_fans(limit: i64) -> Result<Vec<FollowerStat>> {
    follower_stats_query(
        r#"
        SELECT follower_did, follower_handle, follower_display_name, follower_avatar,
               followers_count, following_count, engagement_count, last_engagement_ts,
               mutual, first_followed_ts, reach_score, engagement_score, mutual_bonus,
               composite_score
        FROM follower_stats
        ORDER BY engagement_count DESC, composite_score DESC
        LIMIT ?1
        "#,
        limit,
    )
}

/// High-reach accounts that follow you but you don't follow back —
/// mutual = 0, by reach descending.
pub fn list_high_clout_not_mutual(limit: i64) -> Result<Vec<FollowerStat>> {
    follower_stats_query(
        r#"
        SELECT follower_did, follower_handle, follower_display_name, follower_avatar,
               followers_count, following_count, engagement_count, last_engagement_ts,
               mutual, first_followed_ts, reach_score, engagement_score, mutual_bonus,
               composite_score
        FROM follower_stats
        WHERE mutual = 0
        ORDER BY followers_count DESC
        LIMIT ?1
        "#,
        limit,
    )
}

/// Mutuals ranked by their reach, descending.
pub fn list_mutuals_by_reach(limit: i64) -> Result<Vec<FollowerStat>> {
    follower_stats_query(
        r#"
        SELECT follower_did, follower_handle, follower_display_name, follower_avatar,
               followers_count, following_count, engagement_count, last_engagement_ts,
               mutual, first_followed_ts, reach_score, engagement_score, mutual_bonus,
               composite_score
        FROM follower_stats
        WHERE mutual = 1
        ORDER BY followers_count DESC
        LIMIT ?1
        "#,
        limit,
    )
}

/// High-reach followers who never engage — `followers_count >=
/// min_followers` and zero engagement, by reach descending.
pub fn list_lurkers_by_clout(limit: i64, min_followers: i64) -> Result<Vec<FollowerStat>> {
    with_db(|conn| {
        let mut stmt = conn.prepare(
            r#"
            SELECT follower_did, follower_handle, follower_display_name, follower_avatar,
                   followers_count, following_count, engagement_count, last_engagement_ts,
                   mutual, first_followed_ts, reach_score, engagement_score, mutual_bonus,
                   composite_score
            FROM follower_stats
            WHERE engagement_count = 0 AND followers_count >= ?2
            ORDER BY followers_count DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit, min_followers], row_to_follower_stat)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

// ────────────── engagement reads from inbox_items ──────────────────

/// Count inbox interactions authored by `actor_did` within the
/// engagement window. The window bound is computed in Rust and bound as
/// an RFC3339 parameter — never `datetime('now', …)`, which would
/// mis-compare against RFC3339 `ts` values.
pub fn count_engagements_from_inbox(actor_did: &str, since: DateTime<Utc>) -> Result<i64> {
    with_db(|conn| {
        let n: i64 = conn.query_row(
            r#"
            SELECT COUNT(*)
            FROM inbox_items
            WHERE actor_did = ?1 AND ts >= ?2
            "#,
            params![actor_did, since.to_rfc3339()],
            |row| row.get(0),
        )?;
        Ok(n)
    })
}

/// Most recent inbox interaction authored by `actor_did`, if any.
pub fn last_engagement_from_inbox(actor_did: &str) -> Result<Option<DateTime<Utc>>> {
    with_db(|conn| {
        let ts: Option<String> = conn
            .query_row(
                r#"
                SELECT ts FROM inbox_items
                WHERE actor_did = ?1
                ORDER BY ts DESC
                LIMIT 1
                "#,
                params![actor_did],
                |row| row.get(0),
            )
            .optional()?;
        Ok(parse_rfc3339_opt(ts, 0)?)
    })
}

// ─────────────────────── pure bucketing helpers ────────────────────

/// Posting-cadence heatmap cells: a `[7][24]` grid (`[weekday][hour]`,
/// weekday 0 = Monday, UTC) of post counts normalized to `0.0..=1.0`
/// against the busiest cell. An empty input yields an all-zero grid.
///
/// Pure — no IO — so it's directly unit-testable.
pub fn cadence_cells(timestamps: &[DateTime<Utc>]) -> Vec<Vec<f64>> {
    let mut counts = vec![vec![0.0f64; 24]; 7];
    let mut max = 0.0f64;
    for ts in timestamps {
        let weekday = ts.weekday().num_days_from_monday() as usize; // 0..=6
        let hour = ts.hour() as usize; // 0..=23
        counts[weekday][hour] += 1.0;
        if counts[weekday][hour] > max {
            max = counts[weekday][hour];
        }
    }
    if max > 0.0 {
        for row in counts.iter_mut() {
            for cell in row.iter_mut() {
                *cell /= max;
            }
        }
    }
    counts
}

/// Per-day post counts over the inclusive `[from, to]` day range as
/// `(YYYY-MM-DD, count)` pairs, one bar per calendar day ascending.
/// Days with no posts emit a zero bar so the bar chart has no gaps. An
/// inverted range (`from > to`) yields an empty vec.
///
/// Pure — no IO — so it's directly unit-testable.
pub fn posts_per_day(
    posts: &[PostMetric],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<(String, f64)> {
    if from.date_naive() > to.date_naive() {
        return Vec::new();
    }
    let mut day = from.date_naive();
    let last = to.date_naive();
    let mut out: Vec<(String, f64)> = Vec::new();
    while day <= last {
        let label = day.format("%Y-%m-%d").to_string();
        let count = posts.iter().filter(|p| p.ts.date_naive() == day).count() as f64;
        out.push((label, count));
        day += Duration::days(1);
    }
    out
}

// ────────────────────── view aggregation (DTO) ─────────────────────

/// Number of trailing days the growth curves + posts-per-day bar chart
/// cover. A month reads cleanly on a column-width chart and keeps the
/// per-day cumulative-count sampling cheap (`GROWTH_WINDOW_DAYS × 2`
/// `COUNT(*)` queries).
const GROWTH_WINDOW_DAYS: i64 = 30;
/// Wider window the posting-cadence heatmap aggregates over — a single
/// month is too sparse to reveal a weekly rhythm, so the heatmap reaches
/// back a quarter.
const CADENCE_WINDOW_DAYS: i64 = 90;
/// How many rows the ranked follower / post lists show.
const TOP_FOLLOWERS_LIMIT: i64 = 25;
const TOP_POSTS_LIMIT: i64 = 10;

/// Roll the analytics tables into the view DTO the Analytics column
/// renders. Pure store reads — no network — so it runs on a blocking
/// task off the render thread (see `column.rs`'s Analytics fetch arm).
///
/// - **Growth curves** are reconstructed cumulatively from `follow_events`:
///   for each of the last [`GROWTH_WINDOW_DAYS`] days we count the follow
///   records (per direction) whose TID-decoded `ts` is at or before that
///   day's end. Outgoing → following-over-time; incoming → followers.
/// - **Posts-per-day** + **cadence** come from `post_metrics`; cadence
///   reaches back [`CADENCE_WINDOW_DAYS`] to expose a weekly rhythm.
/// - **Ranked lists** reuse the scoring system of record
///   ([`list_top_fans`]) and the like-ordered [`list_top_posts`].
pub fn build_analytics_data() -> Result<crate::components::AnalyticsData> {
    use crate::components::{AnalyticsData, BarDatum};

    let now = Utc::now();

    // Full-history cumulative growth curves, one sample per month
    // (oldest → newest). A fixed short window made years-old follows
    // read as a constant — a flat, useless line; spanning the whole
    // history shows the real curve. `count_*_until` binds the cutoff as
    // an RFC3339 parameter (never `datetime('now', …)`).
    let (growth_labels, followers_over_time, following_over_time) = growth_series_monthly(now)?;

    // Backfill phase drives the view's per-card loading states (a
    // degenerate/empty card mid-backfill must read as "loading", not
    // "done + empty").
    let backfill = get_backfill_state()?;
    let phase = backfill
        .as_ref()
        .map(|b| b.phase)
        .unwrap_or(BackfillPhase::Pending);

    // Posts: pull the wider cadence window once, derive both the per-day
    // bars (last GROWTH_WINDOW_DAYS) and the [7][24] cadence grid from it.
    let cadence_from = now - Duration::days(CADENCE_WINDOW_DAYS);
    let posts = list_posts_in_range(cadence_from, now)?;
    let cadence = cadence_cells(&posts.iter().map(|p| p.ts).collect::<Vec<_>>());
    let bars_from = now - Duration::days(GROWTH_WINDOW_DAYS);
    let posts_per_day = posts_per_day(&posts, bars_from, now)
        .into_iter()
        .map(|(label, value)| BarDatum { label, value })
        .collect();

    let top_followers = list_top_fans(TOP_FOLLOWERS_LIMIT)?;
    let top_posts = list_top_posts(TOP_POSTS_LIMIT)?;

    Ok(AnalyticsData {
        growth_labels,
        followers_over_time,
        following_over_time,
        posts_per_day,
        cadence,
        top_followers,
        top_posts,
        backfill_phase_rank: phase.rank(),
        backfill_complete: phase == BackfillPhase::Complete,
    })
}

// ─────────────────────────── row mappers ───────────────────────────

fn row_to_metric_snapshot(row: &rusqlite::Row) -> rusqlite::Result<MetricSnapshot> {
    let ts: String = row.get("ts")?;
    Ok(MetricSnapshot {
        snapshot_date: row.get("snapshot_date")?,
        ts: parse_rfc3339(&ts, 1)?,
        followers_count: row.get("followers_count")?,
        following_count: row.get("following_count")?,
        posts_count: row.get("posts_count")?,
    })
}

fn row_to_post_metric(row: &rusqlite::Row) -> rusqlite::Result<PostMetric> {
    let ts: String = row.get("ts")?;
    Ok(PostMetric {
        rkey: row.get("rkey")?,
        uri: row.get("uri")?,
        ts: parse_rfc3339(&ts, 2)?,
        like_count: row.get("like_count")?,
        repost_count: row.get("repost_count")?,
        reply_count: row.get("reply_count")?,
        quote_count: row.get("quote_count")?,
        text_preview: row.get("text_preview")?,
    })
}

fn row_to_follower_stat(row: &rusqlite::Row) -> rusqlite::Result<FollowerStat> {
    let last_eng: Option<String> = row.get("last_engagement_ts")?;
    let first_followed: String = row.get("first_followed_ts")?;
    Ok(FollowerStat {
        follower_did: row.get("follower_did")?,
        follower_handle: row.get("follower_handle")?,
        follower_display_name: row.get("follower_display_name")?,
        follower_avatar: row.get("follower_avatar")?,
        followers_count: row.get("followers_count")?,
        following_count: row.get("following_count")?,
        engagement_count: row.get("engagement_count")?,
        last_engagement_ts: parse_rfc3339_opt(last_eng, 7)?,
        mutual: {
            let v: i32 = row.get("mutual")?;
            v != 0
        },
        first_followed_ts: parse_rfc3339(&first_followed, 9)?,
        reach_score: row.get("reach_score")?,
        engagement_score: row.get("engagement_score")?,
        mutual_bonus: row.get("mutual_bonus")?,
        // Not persisted — recomputed at read by the scoring layer.
        tenure_bonus: 0.0,
        composite_score: row.get("composite_score")?,
    })
}

fn row_to_backfill_state(row: &rusqlite::Row) -> rusqlite::Result<BackfillState> {
    let phase_str: String = row.get("phase")?;
    let phase = BackfillPhase::from_db_str(&phase_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown backfill phase: {phase_str}"),
            )),
        )
    })?;
    let started_at: String = row.get("started_at")?;
    let last_checkpoint_at: String = row.get("last_checkpoint_at")?;
    let completed_at: Option<String> = row.get("completed_at")?;
    Ok(BackfillState {
        phase,
        repo_posts_cursor: row.get("repo_posts_cursor")?,
        repo_follows_cursor: row.get("repo_follows_cursor")?,
        constellation_followers_cursor: row.get("constellation_followers_cursor")?,
        started_at: parse_rfc3339(&started_at, 5)?,
        last_checkpoint_at: parse_rfc3339(&last_checkpoint_at, 6)?,
        completed_at: parse_rfc3339_opt(completed_at, 7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox;
    use parking_lot::MutexGuard;

    /// Install a fresh in-memory DB (with the full inbox+analytics
    /// schema) into the shared connection slot and return the held
    /// serialization guard. The caller binds the guard for the whole
    /// test (`let _g = install_in_memory();`) so the single
    /// process-wide connection isn't stomped by a concurrent DB test.
    fn install_in_memory() -> MutexGuard<'static, ()> {
        let guard = inbox::TEST_DB_GUARD.lock();
        // Drives the full inbox migration path (user_version 0 → 5),
        // which calls create_tables_v5 for us.
        inbox::install_fresh_test_db();
        guard
    }

    fn sample_follow_event(id: &str, dir: FollowDir, ts: DateTime<Utc>) -> FollowEvent {
        FollowEvent {
            id: id.into(),
            direction: dir,
            actor_did: format!("did:plc:{id}"),
            actor_handle: Some(format!("{id}.bsky.social")),
            ts,
            source: match dir {
                FollowDir::Outgoing => "own_repo".into(),
                FollowDir::Incoming => "constellation".into(),
            },
            rkey: Some(format!("rkey-{id}")),
        }
    }

    #[test]
    fn analytics_schema_applies() {
        let _g = install_in_memory();
        with_db(|conn| {
            for table in [
                "metric_snapshots",
                "follow_events",
                "post_metrics",
                "follower_stats",
                "backfill_state",
            ] {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |row| row.get(0),
                )?;
                assert_eq!(n, 1, "table {table} must exist");
            }
            // Key indexes from the v5 schema.
            for idx in [
                "follow_events_direction_ts_idx",
                "post_metrics_like_idx",
                "follower_stats_composite_idx",
            ] {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx],
                    |row| row.get(0),
                )?;
                assert_eq!(n, 1, "index {idx} must exist");
            }
            // user_version must be the unified 5.
            let ver: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
            assert_eq!(ver, 5);
            Ok(())
        })
        .expect("schema checks");
    }

    #[test]
    fn metric_snapshot_roundtrip_and_idempotent() {
        let _g = install_in_memory();
        upsert_metric_snapshot_for_today(100, 50, 10).expect("upsert 1");
        // Same day again with new counts → still one row, updated.
        upsert_metric_snapshot_for_today(110, 55, 12).expect("upsert 2");
        let rows = list_metric_snapshots(100).expect("list");
        assert_eq!(rows.len(), 1, "same-day upsert must be idempotent");
        assert_eq!(rows[0].followers_count, 110);
        assert_eq!(rows[0].following_count, 55);
        assert_eq!(rows[0].posts_count, 12);
    }

    #[test]
    fn follow_event_roundtrip_and_cumulative_count() {
        let _g = install_in_memory();
        let base = Utc::now() - Duration::days(10);
        // Three outgoing follows on successive days, one incoming.
        let e1 = sample_follow_event("a", FollowDir::Outgoing, base);
        let e2 = sample_follow_event("b", FollowDir::Outgoing, base + Duration::days(2));
        let e3 = sample_follow_event("c", FollowDir::Outgoing, base + Duration::days(4));
        let inc = sample_follow_event("z", FollowDir::Incoming, base + Duration::days(1));
        upsert_follow_event(&e1).unwrap();
        upsert_follow_event(&e2).unwrap();
        upsert_follow_event(&e3).unwrap();
        upsert_follow_event(&inc).unwrap();
        // Idempotency: re-upsert one → still no duplicate.
        upsert_follow_event(&e1).unwrap();

        // Cumulative outgoing count grows with the `until` bound.
        let c0 = count_follow_events_until(FollowDir::Outgoing, base - Duration::days(1)).unwrap();
        assert_eq!(c0, 0);
        let c1 = count_follow_events_until(FollowDir::Outgoing, base + Duration::days(2)).unwrap();
        assert_eq!(c1, 2, "two outgoing follows by day 2");
        let c2 =
            count_follow_events_until(FollowDir::Outgoing, base + Duration::days(100)).unwrap();
        assert_eq!(c2, 3, "all three outgoing");
        // Incoming counted separately.
        let inc_count =
            count_follow_events_until(FollowDir::Incoming, base + Duration::days(100)).unwrap();
        assert_eq!(inc_count, 1);
    }

    #[test]
    fn post_metric_roundtrip_and_top() {
        let _g = install_in_memory();
        let now = Utc::now();
        let p1 = PostMetric {
            rkey: "p1".into(),
            uri: "at://did:plc:me/app.bsky.feed.post/p1".into(),
            ts: now - Duration::days(2),
            like_count: 5,
            repost_count: 1,
            reply_count: 2,
            quote_count: 0,
            text_preview: Some("hello".into()),
        };
        let p2 = PostMetric {
            rkey: "p2".into(),
            uri: "at://did:plc:me/app.bsky.feed.post/p2".into(),
            ts: now - Duration::days(1),
            like_count: 50,
            repost_count: 10,
            reply_count: 3,
            quote_count: 1,
            text_preview: Some("viral".into()),
        };
        upsert_post_metric(&p1).unwrap();
        upsert_post_metric(&p2).unwrap();
        // Idempotent refresh.
        upsert_post_metric(&p2).unwrap();

        let top = list_top_posts(10).unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].rkey, "p2", "highest like_count first");

        let in_range = list_posts_in_range(now - Duration::days(3), now).unwrap();
        assert_eq!(in_range.len(), 2);
        assert_eq!(in_range[0].rkey, "p1", "ascending by ts");

        let narrow =
            list_posts_in_range(now - Duration::days(1) - Duration::hours(1), now).unwrap();
        assert_eq!(narrow.len(), 1, "range excludes the older post");
        assert_eq!(narrow[0].rkey, "p2");
    }

    #[test]
    fn follower_stat_roundtrip_and_ranked_cuts() {
        let _g = install_in_memory();
        let now = Utc::now();
        let mk = |did: &str, followers: i64, eng: i64, mutual: bool| FollowerStat {
            follower_did: did.into(),
            follower_handle: format!("{did}.bsky.social"),
            follower_display_name: Some(did.to_uppercase()),
            follower_avatar: None,
            followers_count: followers,
            following_count: 100,
            engagement_count: eng,
            last_engagement_ts: if eng > 0 { Some(now) } else { None },
            mutual,
            first_followed_ts: now - Duration::days(30),
            reach_score: 0.5,
            engagement_score: 0.4,
            mutual_bonus: if mutual { 0.15 } else { 0.0 },
            tenure_bonus: 0.0,
            composite_score: 0.6,
        };
        let fan = mk("fan", 200, 25, true);
        let whale = mk("whale", 100_000, 0, false);
        let mutual_big = mk("mutualbig", 5_000, 3, true);
        upsert_follower_stat(&fan).unwrap();
        upsert_follower_stat(&whale).unwrap();
        upsert_follower_stat(&mutual_big).unwrap();

        // round-trip identity (tenure_bonus is not persisted → 0.0).
        let fans = list_top_fans(10).unwrap();
        assert_eq!(fans[0].follower_did, "fan", "most engagements first");
        assert_eq!(fans[0].tenure_bonus, 0.0);

        let high = list_high_clout_not_mutual(10).unwrap();
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].follower_did, "whale");

        let mutuals = list_mutuals_by_reach(10).unwrap();
        assert_eq!(mutuals[0].follower_did, "mutualbig", "highest reach mutual");

        let lurkers = list_lurkers_by_clout(10, 10_000).unwrap();
        assert_eq!(lurkers.len(), 1);
        assert_eq!(
            lurkers[0].follower_did, "whale",
            "high reach, zero engagement"
        );
    }

    #[test]
    fn backfill_state_roundtrip() {
        let _g = install_in_memory();
        assert!(get_backfill_state().unwrap().is_none());
        let now = Utc::now();
        let state = BackfillState {
            phase: BackfillPhase::Posts,
            repo_posts_cursor: Some("cur-1".into()),
            repo_follows_cursor: None,
            constellation_followers_cursor: None,
            started_at: now,
            last_checkpoint_at: now,
            completed_at: None,
        };
        checkpoint_backfill(&state).unwrap();
        let got = get_backfill_state().unwrap().expect("present");
        assert_eq!(got.phase, BackfillPhase::Posts);
        assert_eq!(got.repo_posts_cursor.as_deref(), Some("cur-1"));

        // Advance to complete; still one row.
        let done = BackfillState {
            phase: BackfillPhase::Complete,
            completed_at: Some(now),
            ..state
        };
        checkpoint_backfill(&done).unwrap();
        let got2 = get_backfill_state().unwrap().expect("present");
        assert_eq!(got2.phase, BackfillPhase::Complete);
        assert!(got2.completed_at.is_some());
    }

    #[test]
    fn engagement_window_uses_rust_rfc3339_bound() {
        let _g = install_in_memory();
        // Insert two inbox items for one actor: one recent, one ancient.
        let now = Utc::now();
        let mk_item = |id: &str, ts: DateTime<Utc>| inbox::InboxItem {
            item_id: id.into(),
            source: inbox::InboxSource::DirectReply,
            actor_did: "did:plc:engager".into(),
            actor_handle: None,
            actor_display_name: None,
            actor_avatar: None,
            subject_uri: "at://x".into(),
            preview: None,
            ts,
            directness: 60,
            read: false,
            archived: false,
            snoozed_until: None,
            device_id: None,
            synced_at: None,
            payload_json: "{}".into(),
            actor_follower_count: 0,
            actor_follows_count: 0,
            ts_bucket: inbox::ts_bucket_for(ts),
        };
        inbox::upsert(&mk_item("recent", now - Duration::days(1))).unwrap();
        inbox::upsert(&mk_item("ancient", now - Duration::days(400))).unwrap();

        let since = now - Duration::days(ENGAGEMENT_WINDOW_DAYS);
        let windowed = count_engagements_from_inbox("did:plc:engager", since).unwrap();
        assert_eq!(windowed, 1, "only the in-window item counts");

        let last = last_engagement_from_inbox("did:plc:engager")
            .unwrap()
            .expect("has engagement");
        // Most-recent is the 1-day-old item, not the 400-day-old one.
        assert!(last > now - Duration::days(2));
    }

    #[test]
    fn cadence_cells_buckets_by_weekday_hour() {
        // Empty input → all-zero 7×24 grid.
        let empty = cadence_cells(&[]);
        assert_eq!(empty.len(), 7);
        assert_eq!(empty[0].len(), 24);
        assert!(empty.iter().all(|r| r.iter().all(|c| *c == 0.0)));

        // 2026-06-22 is a Monday (weekday 0). Build two posts Monday 09:00
        // and one Wednesday 14:00 (weekday 2).
        let mon_9a = DateTime::parse_from_rfc3339("2026-06-22T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mon_9b = DateTime::parse_from_rfc3339("2026-06-22T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let wed_2 = DateTime::parse_from_rfc3339("2026-06-24T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let cells = cadence_cells(&[mon_9a, mon_9b, wed_2]);
        // Monday 09 is the busiest cell (2) → normalized to 1.0.
        assert_eq!(cells[0][9], 1.0);
        // Wednesday 14 has half the volume → 0.5.
        assert_eq!(cells[2][14], 0.5);
        // An untouched cell stays 0.
        assert_eq!(cells[1][0], 0.0);
    }

    #[test]
    fn month_cutoffs_spans_full_history_inclusive() {
        let from = DateTime::parse_from_rfc3339("2024-09-25T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let to = DateTime::parse_from_rfc3339("2026-06-26T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let months = month_cutoffs(from, to);
        // Sep 2024 .. Jun 2026 inclusive = 22 monthly samples.
        assert_eq!(months.len(), 22);
        assert_eq!(months.first().unwrap().0, "2024-09");
        assert_eq!(months.last().unwrap().0, "2026-06");
        // Each cutoff is the first instant of the month AFTER its label,
        // so a `ts <= cutoff` count includes that whole labelled month.
        let (label, cutoff) = &months[0];
        assert_eq!(label, "2024-09");
        assert_eq!(cutoff.to_rfc3339(), "2024-10-01T00:00:00+00:00");
        // Cutoffs are strictly increasing.
        assert!(months.windows(2).all(|w| w[0].1 < w[1].1));
    }

    #[test]
    fn month_cutoffs_single_month() {
        let t = DateTime::parse_from_rfc3339("2026-06-10T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let months = month_cutoffs(t, t);
        assert_eq!(months.len(), 1);
        assert_eq!(months[0].0, "2026-06");
    }

    #[test]
    fn posts_per_day_buckets_and_fills_gaps() {
        let mk = |rkey: &str, ts: DateTime<Utc>| PostMetric {
            rkey: rkey.into(),
            uri: "at://x".into(),
            ts,
            like_count: 0,
            repost_count: 0,
            reply_count: 0,
            quote_count: 0,
            text_preview: None,
        };
        let d22 = DateTime::parse_from_rfc3339("2026-06-22T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let d22b = DateTime::parse_from_rfc3339("2026-06-22T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let d24 = DateTime::parse_from_rfc3339("2026-06-24T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let from = DateTime::parse_from_rfc3339("2026-06-22T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let to = DateTime::parse_from_rfc3339("2026-06-24T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);
        let bars = posts_per_day(&[mk("a", d22), mk("b", d22b), mk("c", d24)], from, to);
        // 3 calendar days, gap day (23rd) filled with a zero bar.
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0], ("2026-06-22".to_string(), 2.0));
        assert_eq!(bars[1], ("2026-06-23".to_string(), 0.0));
        assert_eq!(bars[2], ("2026-06-24".to_string(), 1.0));

        // Inverted range → empty.
        assert!(posts_per_day(&[], to, from).is_empty());
    }
}
