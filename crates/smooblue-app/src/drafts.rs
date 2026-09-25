//! Compose drafts — every post, reply, quote, and multi-post thread the
//! user starts is autosaved here until it's posted or discarded.
//!
//! Before this module the composer kept a single `draft.txt` holding
//! only the first post's text: closing the sheet dropped the thread
//! posts, and a half-written reply leaked into the next "New post".
//! Now a draft is the whole composer state — reply / quote target,
//! every post in the thread, and each post's attached media (by path,
//! with alt text) — and there can be many of them.
//!
//! Storage is the shared app SQLite file (`drafts` table, schema v7,
//! owned by [`crate::inbox::migrate`]). The draft body is one JSON
//! blob so the shape can grow without a migration per field.

use crate::state::{QuoteTarget, ReplyTarget};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Oldest drafts are pruned past this many per account, so a years-old
/// pile of abandoned replies can't grow the DB without bound.
const MAX_DRAFTS: usize = 200;

/// One attached file (image or video), stored by path. The bytes stay
/// on disk; restoring a draft re-runs the normal attach pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftMedia {
    pub path: PathBuf,
    #[serde(default)]
    pub alt: String,
}

/// One post in a (possibly single-post) draft thread.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftPost {
    pub text: String,
    #[serde(default)]
    pub images: Vec<DraftMedia>,
    #[serde(default)]
    pub video: Option<DraftMedia>,
    /// A GIF picked from the composer's GIF search (first post only).
    #[serde(default)]
    pub gif: Option<crate::gifs::Gif>,
    /// The user's alt text for that GIF (empty = Tenor's description).
    #[serde(default)]
    pub gif_alt: String,
}

impl DraftPost {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.images.is_empty()
            && self.video.is_none()
            && self.gif.is_none()
    }
}

/// What a draft is attached to. Used to find the right draft to resume
/// when the composer opens for a given target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftTarget {
    /// A new top-level post (or self-thread).
    New,
    /// A reply to the post at this URI.
    Reply(String),
    /// A quote of the post at this URI.
    Quote(String),
}

impl DraftTarget {
    pub fn of(reply_to: Option<&ReplyTarget>, quote_to: Option<&QuoteTarget>) -> Self {
        match (reply_to, quote_to) {
            (Some(r), _) => Self::Reply(r.uri.clone()),
            (None, Some(q)) => Self::Quote(q.uri.clone()),
            (None, None) => Self::New,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    /// DID of the account the draft belongs to. `None` for drafts
    /// imported from the pre-multi-draft `draft.txt`, which show for
    /// every account.
    #[serde(default)]
    pub account_did: Option<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub reply_to: Option<ReplyTarget>,
    #[serde(default)]
    pub quote_to: Option<QuoteTarget>,
    /// Posts in thread order. Always at least one once built through
    /// [`Draft::new`]; an older / hand-edited row with zero posts is
    /// treated as empty.
    pub posts: Vec<DraftPost>,
}

impl Draft {
    pub fn new(account_did: Option<String>) -> Self {
        Self {
            id: new_id(),
            account_did,
            updated_at: Utc::now(),
            reply_to: None,
            quote_to: None,
            posts: vec![DraftPost::default()],
        }
    }

    pub fn target(&self) -> DraftTarget {
        DraftTarget::of(self.reply_to.as_ref(), self.quote_to.as_ref())
    }

    /// A draft with no text and no media in any post. Empty drafts
    /// are never stored — saving one deletes the row instead.
    pub fn is_empty(&self) -> bool {
        self.posts.iter().all(DraftPost::is_empty)
    }

    /// Number of non-empty posts — what would actually be published.
    pub fn post_count(&self) -> usize {
        self.posts.iter().filter(|p| !p.is_empty()).count()
    }

    /// Short human label for draft lists: "Reply to @alice",
    /// "Quote of @bob", "Thread · 4 posts", or "Post".
    pub fn label(&self) -> String {
        let n = self.post_count();
        let thread = if n > 1 {
            format!(" · {n} posts")
        } else {
            String::new()
        };
        match (&self.reply_to, &self.quote_to) {
            (Some(r), _) => format!("Reply to @{}{thread}", r.handle),
            (None, Some(q)) => format!("Quote of @{}{thread}", q.handle),
            (None, None) if n > 1 => format!("Thread{thread}"),
            (None, None) => "Post".to_string(),
        }
    }

    /// First non-empty post text, whitespace-collapsed and cut to
    /// `max_chars`, for list rows and the resume chip. Media-only
    /// drafts preview as "(image)" / "(video)".
    pub fn preview(&self, max_chars: usize) -> String {
        for p in &self.posts {
            let collapsed = p.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !collapsed.is_empty() {
                return truncate_chars(&collapsed, max_chars);
            }
        }
        for p in &self.posts {
            if p.video.is_some() {
                return "(video)".into();
            }
            if p.gif.is_some() {
                return "(GIF)".into();
            }
            if !p.images.is_empty() {
                return "(image)".into();
            }
        }
        String::new()
    }
}

/// Truncate to `max` chars, adding an ellipsis when anything was cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The draft to resume when the composer opens for `target`: the most
/// recently edited one aimed at the same place. `drafts` must be
/// sorted newest-first (as [`list`] returns them).
pub fn pick_resume<'a>(drafts: &'a [Draft], target: &DraftTarget) -> Option<&'a Draft> {
    drafts
        .iter()
        .find(|d| &d.target() == target && !d.is_empty())
}

/// Split `text` into chunks of at most `max` chars for a thread,
/// preferring (in order) paragraph breaks, sentence ends, then word
/// boundaries — and hard-cutting only a single "word" longer than
/// `max` (e.g. a giant URL). Chunks are trimmed; empties dropped.
pub fn split_for_thread(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        if rest.chars().count() <= max {
            out.push(rest.to_string());
            break;
        }
        // Byte index just past the `max`-th char — the hard ceiling.
        let ceiling = rest
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        let window = &rest[..ceiling];
        // Don't accept a break so early that the chunk is tiny; a
        // paragraph break at char 10 of a 300 budget would make a
        // throwaway post. Half the budget is the floor.
        let floor = window
            .char_indices()
            .nth(max / 2)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let cut = find_break(window, floor, "\n\n")
            .or_else(|| find_sentence_break(window, floor))
            .or_else(|| find_break(window, floor, "\n"))
            .or_else(|| find_break(window, 0, " "))
            .unwrap_or(ceiling);
        let chunk = rest[..cut].trim();
        if !chunk.is_empty() {
            out.push(chunk.to_string());
        }
        rest = rest[cut..].trim_start();
    }
    out
}

/// Last occurrence of `sep` in `window` at or after byte `floor`;
/// returns the byte index just after it.
fn find_break(window: &str, floor: usize, sep: &str) -> Option<usize> {
    window
        .rfind(sep)
        .filter(|&i| i >= floor && i > 0)
        .map(|i| i + sep.len())
}

/// Last ". " / "! " / "? " (sentence end followed by whitespace) at or
/// after `floor`; returns the index just after the punctuation.
fn find_sentence_break(window: &str, floor: usize) -> Option<usize> {
    let bytes = window.as_bytes();
    (floor.max(1)..window.len())
        .rev()
        .find(|&i| matches!(bytes[i - 1], b'.' | b'!' | b'?') && bytes[i].is_ascii_whitespace())
}

// ── Storage ─────────────────────────────────────────────────────────

/// Ids of drafts that were posted or discarded this session. A
/// debounced autosave snapshotted just before the post can land on its
/// worker thread *after* the delete; without this it would resurrect a
/// draft of something already published.
static RETIRED: std::sync::OnceLock<parking_lot::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

fn retired() -> &'static parking_lot::Mutex<std::collections::HashSet<String>> {
    RETIRED.get_or_init(Default::default)
}

/// Upsert `draft`, or remove its row if it's empty. A stale snapshot
/// (older `updated_at` than the stored row) never overwrites a newer
/// one, and a retired id is never written again. Prunes the oldest
/// drafts past [`MAX_DRAFTS`] for the same account.
pub fn save(draft: &Draft) -> Result<()> {
    if retired().lock().contains(&draft.id) {
        return Ok(());
    }
    if draft.is_empty() {
        return delete_row(&draft.id);
    }
    let body = serde_json::to_string(draft)?;
    crate::inbox::with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO drafts (id, account_did, updated_at, body_json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                account_did = excluded.account_did,
                updated_at  = excluded.updated_at,
                body_json   = excluded.body_json
            WHERE excluded.updated_at >= drafts.updated_at
            "#,
            params![
                draft.id,
                draft.account_did,
                draft
                    .updated_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                body
            ],
        )?;
        conn.execute(
            r#"
            DELETE FROM drafts WHERE id IN (
                SELECT id FROM drafts
                WHERE account_did IS ?1
                ORDER BY updated_at DESC
                LIMIT -1 OFFSET ?2
            )
            "#,
            params![draft.account_did, MAX_DRAFTS as i64],
        )?;
        Ok(())
    })
}

/// Permanently remove a draft that was posted or discarded. The id is
/// retired so an in-flight autosave can't bring it back.
pub fn delete(id: &str) -> Result<()> {
    retired().lock().insert(id.to_string());
    delete_row(id)
}

fn delete_row(id: &str) -> Result<()> {
    crate::inbox::with_db(|conn| {
        conn.execute("DELETE FROM drafts WHERE id = ?1", params![id])?;
        Ok(())
    })
}

pub fn get(id: &str) -> Result<Option<Draft>> {
    crate::inbox::with_db(|conn| {
        let body: Option<String> = conn
            .query_row(
                "SELECT body_json FROM drafts WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(body.and_then(|b| serde_json::from_str(&b).ok()))
    })
}

/// Every draft visible to `account_did` (its own plus legacy
/// account-less ones), newest first. Rows that fail to decode are
/// skipped rather than failing the whole list.
pub fn list(account_did: Option<&str>) -> Result<Vec<Draft>> {
    crate::inbox::with_db(|conn| {
        let mut stmt = conn.prepare(
            r#"
            SELECT body_json FROM drafts
            WHERE account_did IS NULL OR account_did = ?1
            ORDER BY updated_at DESC
            "#,
        )?;
        let rows = stmt.query_map(params![account_did], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for body in rows.flatten() {
            match serde_json::from_str::<Draft>(&body) {
                Ok(d) if !d.is_empty() => out.push(d),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "drafts: skipping undecodable row"),
            }
        }
        Ok(out)
    })
}

/// One-time import of the pre-multi-draft `draft.txt` (text only) as a
/// regular draft, then remove the file so it isn't imported twice.
pub fn import_legacy_draft() -> Result<()> {
    let Some(text) = crate::persistence::load_draft() else {
        return Ok(());
    };
    let mut d = Draft::new(None);
    d.posts = vec![DraftPost::text(text)];
    save(&d)?;
    crate::persistence::clear_legacy_draft();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{install_fresh_test_db, TEST_DB_GUARD};

    fn reply(uri: &str, handle: &str) -> ReplyTarget {
        ReplyTarget {
            uri: uri.into(),
            cid: "cid".into(),
            root_uri: uri.into(),
            root_cid: "cid".into(),
            handle: handle.into(),
            text: "parent".into(),
        }
    }

    fn draft_with(texts: &[&str]) -> Draft {
        let mut d = Draft::new(Some("did:plc:me".into()));
        d.posts = texts.iter().map(|t| DraftPost::text(*t)).collect();
        d
    }

    #[test]
    fn empty_draft_detection_ignores_whitespace_but_counts_media() {
        assert!(draft_with(&["", "  \n"]).is_empty());
        let mut d = draft_with(&[""]);
        d.posts[0].images.push(DraftMedia {
            path: "/tmp/x.png".into(),
            alt: String::new(),
        });
        assert!(!d.is_empty());
        assert_eq!(d.preview(40), "(image)");
    }

    #[test]
    fn labels_describe_target_and_thread_length() {
        assert_eq!(draft_with(&["hi"]).label(), "Post");
        assert_eq!(draft_with(&["a", "b", ""]).label(), "Thread · 2 posts");
        let mut r = draft_with(&["a"]);
        r.reply_to = Some(reply("at://x", "alice.bsky.social"));
        assert_eq!(r.label(), "Reply to @alice.bsky.social");
    }

    #[test]
    fn preview_collapses_whitespace_and_truncates() {
        let d = draft_with(&["", "hello\n\n   world   and more"]);
        assert_eq!(d.preview(100), "hello world and more");
        assert_eq!(d.preview(8), "hello w…");
    }

    #[test]
    fn pick_resume_matches_target_newest_first() {
        let mut older_new = draft_with(&["old top-level"]);
        older_new.updated_at = Utc::now() - chrono::Duration::hours(2);
        let mut reply_a = draft_with(&["reply to a"]);
        reply_a.reply_to = Some(reply("at://a", "a"));
        let newer_new = draft_with(&["new top-level"]);
        // Newest-first, as list() returns.
        let drafts = vec![newer_new.clone(), reply_a.clone(), older_new];
        assert_eq!(
            pick_resume(&drafts, &DraftTarget::New).map(|d| &d.id),
            Some(&newer_new.id)
        );
        assert_eq!(
            pick_resume(&drafts, &DraftTarget::Reply("at://a".into())).map(|d| &d.id),
            Some(&reply_a.id)
        );
        assert!(pick_resume(&drafts, &DraftTarget::Reply("at://b".into())).is_none());
    }

    #[test]
    fn split_keeps_short_text_whole() {
        assert_eq!(split_for_thread("  hello  ", 300), vec!["hello"]);
        assert!(split_for_thread("   ", 300).is_empty());
    }

    #[test]
    fn split_prefers_paragraphs_then_sentences_then_words() {
        let para = format!("{}\n\n{}", "a".repeat(20), "b".repeat(20));
        assert_eq!(
            split_for_thread(&para, 30),
            vec!["a".repeat(20), "b".repeat(20)]
        );

        let sentences = "One two three four. Five six seven eight. Nine ten.";
        let chunks = split_for_thread(sentences, 30);
        assert_eq!(chunks[0], "One two three four.");
        assert!(chunks.iter().all(|c| c.chars().count() <= 30));

        let words = "alpha beta gamma delta epsilon zeta eta theta";
        let chunks = split_for_thread(words, 12);
        assert!(chunks.iter().all(|c| c.chars().count() <= 12));
        assert_eq!(chunks.join(" "), words);
    }

    #[test]
    fn split_hard_cuts_a_single_overlong_word_on_char_boundaries() {
        let emoji = "🦋".repeat(25);
        let chunks = split_for_thread(&emoji, 10);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.chars().count() <= 10));
        assert_eq!(chunks.concat(), emoji);
    }

    #[test]
    fn split_every_chunk_fits_for_realistic_prose() {
        let prose = "This is a long post about Rust. ".repeat(40);
        let chunks = split_for_thread(&prose, 300);
        assert!(chunks.len() >= 4);
        assert!(chunks.iter().all(|c| c.chars().count() <= 300));
        // Nothing lost: word stream survives the split.
        let rejoined: Vec<&str> = chunks.iter().flat_map(|c| c.split_whitespace()).collect();
        let original: Vec<&str> = prose.split_whitespace().collect();
        assert_eq!(rejoined, original);
    }

    #[test]
    fn storage_roundtrip_scoping_and_empty_delete() {
        let _guard = TEST_DB_GUARD.lock();
        install_fresh_test_db();

        let mut mine = draft_with(&["mine", "second"]);
        mine.reply_to = Some(reply("at://p", "p"));
        mine.posts[0].images.push(DraftMedia {
            path: "/tmp/a.png".into(),
            alt: "a cat".into(),
        });
        save(&mine).unwrap();
        let mut theirs = draft_with(&["theirs"]);
        theirs.account_did = Some("did:plc:other".into());
        save(&theirs).unwrap();
        let mut legacy = draft_with(&["legacy"]);
        legacy.account_did = None;
        legacy.updated_at = Utc::now() - chrono::Duration::days(1);
        save(&legacy).unwrap();

        let listed = list(Some("did:plc:me")).unwrap();
        let ids: Vec<&str> = listed.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec![mine.id.as_str(), legacy.id.as_str()]);
        assert_eq!(get(&mine.id).unwrap().as_ref(), Some(&mine));

        // Saving an emptied draft removes it.
        let mut emptied = mine.clone();
        emptied.posts = vec![DraftPost::default()];
        save(&emptied).unwrap();
        assert!(get(&mine.id).unwrap().is_none());

        // Emptying is not retiring: typing again saves under the same id.
        save(&mine).unwrap();
        assert!(get(&mine.id).unwrap().is_some());

        delete(&theirs.id).unwrap();
        assert!(list(Some("did:plc:other"))
            .unwrap()
            .iter()
            .all(|d| d.id != theirs.id));
        // A late autosave of a retired (posted / discarded) draft is dropped.
        save(&theirs).unwrap();
        assert!(get(&theirs.id).unwrap().is_none());
    }

    #[test]
    fn stale_snapshot_does_not_overwrite_newer_row() {
        let _guard = TEST_DB_GUARD.lock();
        install_fresh_test_db();

        let mut newer = draft_with(&["newer text"]);
        let mut stale = newer.clone();
        stale.posts = vec![DraftPost::text("stale text")];
        stale.updated_at = newer.updated_at - chrono::Duration::seconds(1);
        newer.updated_at += chrono::Duration::milliseconds(1);
        save(&newer).unwrap();
        save(&stale).unwrap();
        assert_eq!(get(&newer.id).unwrap().unwrap().posts[0].text, "newer text");
    }
}
