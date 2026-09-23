//! Navigation history — back / forward inside the thread and profile
//! sheets, "reopen what I just closed", and a persisted list of
//! recently viewed threads and profiles.
//!
//! Both sheets are modals over the deck, and a modal is easy to lose:
//! one stray click on the backdrop and a 200-reply thread you were
//! halfway through is gone, along with the chain of replies you had
//! clicked through to get there. The in-memory [`NavStacks`] give each
//! sheet browser-style back / forward; the [`ClosedSheet`] snapshot
//! lets ⌘⇧T (or the "Reopen" toast) put the sheet back exactly as it
//! was; and the `nav_history` table (schema v7) backs the History
//! sheet so a thread from yesterday is still one click away.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;

/// Deepest back stack we keep per sheet. Plenty for clicking through a
/// thread; bounded so an hour of browsing doesn't grow it forever.
const MAX_STACK: usize = 100;
/// Rows kept in `nav_history`; older ones are pruned on insert.
const MAX_HISTORY_ROWS: i64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavKind {
    Thread,
    Profile,
}

impl NavKind {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Profile => "profile",
        }
    }

    fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "thread" => Some(Self::Thread),
            "profile" => Some(Self::Profile),
            _ => None,
        }
    }
}

/// Back / forward stacks for one sheet. Keys are what the sheet's
/// focus signal holds (a post AT-URI or an actor DID / handle).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavStacks {
    pub back: Vec<String>,
    pub forward: Vec<String>,
    /// Set by [`go_back`](Self::go_back) / [`go_forward`](Self::go_forward)
    /// (and by a reopen) to the key being navigated to, so the focus
    /// change that follows isn't recorded as a fresh navigation.
    pending: Option<String>,
}

impl NavStacks {
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Step back from `current`. Returns the key to focus next.
    pub fn go_back(&mut self, current: &str) -> Option<String> {
        let target = self.back.pop()?;
        self.forward.push(current.to_string());
        self.pending = Some(target.clone());
        Some(target)
    }

    /// Step forward from `current`. Returns the key to focus next.
    pub fn go_forward(&mut self, current: &str) -> Option<String> {
        let target = self.forward.pop()?;
        self.back.push(current.to_string());
        self.pending = Some(target.clone());
        Some(target)
    }

    /// Mark `key` as an expected focus change (used when restoring a
    /// closed sheet, whose stacks come back with it).
    pub fn expect(&mut self, key: &str) {
        self.pending = Some(key.to_string());
    }

    /// Record a focus change from `prev` to `next`. Returns the key of
    /// the sheet that just closed (`Some → None`), so the caller can
    /// snapshot it for "reopen".
    pub fn observe(&mut self, prev: Option<&str>, next: Option<&str>) -> Option<String> {
        if let (Some(n), Some(p)) = (next, self.pending.as_deref()) {
            if n == p {
                self.pending = None;
                return None;
            }
        }
        self.pending = None;
        match (prev, next) {
            (Some(a), Some(b)) if a != b => {
                if self.back.last().map(String::as_str) != Some(a) {
                    self.back.push(a.to_string());
                }
                if self.back.len() > MAX_STACK {
                    self.back.remove(0);
                }
                self.forward.clear();
                None
            }
            (None, Some(_)) => {
                // Fresh open from the deck — an unrelated trail.
                self.back.clear();
                self.forward.clear();
                None
            }
            (Some(a), None) => Some(a.to_string()),
            _ => None,
        }
    }
}

/// A sheet the user closed, with the trail they took through it, so
/// it can be reopened exactly as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedSheet {
    pub kind: NavKind,
    pub key: String,
    pub stacks: NavStacks,
}

/// App-wide navigation state (a Dioxus context signal).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavHistory {
    pub thread: NavStacks,
    pub profile: NavStacks,
    /// Most recently closed sheet, for ⌘⇧T / the reopen toast.
    pub last_closed: Option<ClosedSheet>,
    /// Bumped on every close so the toast re-shows even when the same
    /// thread is closed twice in a row.
    pub closed_seq: u64,
    /// When each sheet last came to the front (opened or navigated).
    /// Thread and profile sheets can stack either way round — a post
    /// clicked inside a profile opens a thread *over* it — so paint
    /// order and Esc follow this, not a fixed DOM order.
    raise_seq: u64,
    thread_raised: u64,
    profile_raised: u64,
}

impl NavHistory {
    pub fn stacks_mut(&mut self, kind: NavKind) -> &mut NavStacks {
        match kind {
            NavKind::Thread => &mut self.thread,
            NavKind::Profile => &mut self.profile,
        }
    }

    pub fn stacks(&self, kind: NavKind) -> &NavStacks {
        match kind {
            NavKind::Thread => &self.thread,
            NavKind::Profile => &self.profile,
        }
    }

    /// Feed a focus change for `kind`. On close, the trail is moved
    /// into `last_closed` and the live stacks reset.
    pub fn observe(&mut self, kind: NavKind, prev: Option<&str>, next: Option<&str>) {
        if next.is_some() && next != prev {
            self.raise_seq += 1;
            match kind {
                NavKind::Thread => self.thread_raised = self.raise_seq,
                NavKind::Profile => self.profile_raised = self.raise_seq,
            }
        }
        if let Some(key) = self.stacks_mut(kind).observe(prev, next) {
            let stacks = std::mem::take(self.stacks_mut(kind));
            self.last_closed = Some(ClosedSheet { kind, key, stacks });
            self.closed_seq = self.closed_seq.wrapping_add(1);
        }
    }

    /// Whether the thread sheet was brought forward more recently than
    /// the profile sheet (so it paints — and Esc-closes — first).
    pub fn thread_above_profile(&self) -> bool {
        self.thread_raised > self.profile_raised
    }

    /// Take the last-closed snapshot and restore its stacks, primed so
    /// the upcoming focus change isn't treated as a fresh open. The
    /// caller sets the sheet's focus signal to the returned key.
    pub fn take_reopen(&mut self) -> Option<(NavKind, String)> {
        let closed = self.last_closed.take()?;
        let mut stacks = closed.stacks;
        stacks.expect(&closed.key);
        *self.stacks_mut(closed.kind) = stacks;
        Some((closed.kind, closed.key))
    }
}

// ── Signal-level actions (buttons + keyboard) ───────────────────────

use crate::state::{ProfileFocus, ThreadFocus};
use dioxus::prelude::*;

/// The sheet on top — the one the keyboard's back / forward and Esc
/// act on when both the thread and profile sheets are open.
pub fn topmost(
    nav: Signal<NavHistory>,
    thread: Signal<ThreadFocus>,
    profile: Signal<ProfileFocus>,
) -> Option<NavKind> {
    match (thread.peek().0.is_some(), profile.peek().0.is_some()) {
        (true, true) if nav.peek().thread_above_profile() => Some(NavKind::Thread),
        (_, true) => Some(NavKind::Profile),
        (true, false) => Some(NavKind::Thread),
        (false, false) => None,
    }
}

/// Step one sheet back (or forward). Returns whether anything moved.
pub fn step(
    mut nav: Signal<NavHistory>,
    mut thread: Signal<ThreadFocus>,
    mut profile: Signal<ProfileFocus>,
    kind: NavKind,
    forward: bool,
) -> bool {
    let current = match kind {
        NavKind::Thread => thread.peek().0.clone(),
        NavKind::Profile => profile.peek().0.clone(),
    };
    let Some(current) = current else {
        return false;
    };
    let target = {
        let mut n = nav.write();
        let stacks = n.stacks_mut(kind);
        if forward {
            stacks.go_forward(&current)
        } else {
            stacks.go_back(&current)
        }
    };
    let Some(target) = target else {
        return false;
    };
    match kind {
        NavKind::Thread => thread.set(ThreadFocus(Some(target))),
        NavKind::Profile => profile.set(ProfileFocus(Some(target))),
    }
    true
}

/// Reopen the most recently closed thread / profile sheet with its
/// back / forward trail intact. Returns whether anything reopened.
pub fn reopen_last(
    mut nav: Signal<NavHistory>,
    mut thread: Signal<ThreadFocus>,
    mut profile: Signal<ProfileFocus>,
) -> bool {
    let Some((kind, key)) = nav.write().take_reopen() else {
        return false;
    };
    match kind {
        NavKind::Thread => thread.set(ThreadFocus(Some(key))),
        NavKind::Profile => profile.set(ProfileFocus(Some(key))),
    }
    true
}

/// Open a History entry: its sheet as a fresh navigation.
pub fn open_entry(
    kind: NavKind,
    key: String,
    mut thread: Signal<ThreadFocus>,
    mut profile: Signal<ProfileFocus>,
) {
    match kind {
        NavKind::Thread => thread.set(ThreadFocus(Some(key))),
        NavKind::Profile => profile.set(ProfileFocus(Some(key))),
    }
}

/// Mirror a sheet's focus signal into [`NavHistory`]. Call once per
/// sheet component, with a reader for its focus key.
pub fn use_nav_tracker(kind: NavKind, read_focus: impl Fn() -> Option<String> + 'static) {
    let mut nav = use_context::<Signal<NavHistory>>();
    let mut prev = use_signal(|| None::<String>);
    use_effect(move || {
        // A blank key is a click site that forgot to fill the focus;
        // the sheet closes itself at once. Treat it as "closed" so it
        // never becomes a back entry or a "Reopen" target.
        let next = read_focus().filter(|k| !k.trim().is_empty());
        let before = prev.peek().clone();
        if before == next {
            return;
        }
        prev.set(next.clone());
        nav.write()
            .observe(kind, before.as_deref(), next.as_deref());
    });
}

/// Record a viewed thread / profile in the persisted history, off the
/// UI thread. Failures only log — history is a convenience.
pub fn record_in_background(kind: NavKind, key: String, title: String, subtitle: String) {
    if key.trim().is_empty() {
        return;
    }
    spawn(async move {
        let res = tokio::task::spawn_blocking(move || record(kind, &key, &title, &subtitle)).await;
        if let Ok(Err(e)) = res {
            tracing::warn!(error = %e, "history: record failed");
        }
    });
}

/// One-line, length-capped version of a post body for history rows.
pub fn snippet(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut out: String = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    out.push('…');
    out
}

// ── Persisted "recently viewed" list ────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub kind: NavKind,
    /// Post AT-URI (threads) or DID (profiles).
    pub key: String,
    /// Author / display name line.
    pub title: String,
    /// Post text snippet (threads) or @handle (profiles).
    pub subtitle: String,
    pub viewed_at: DateTime<Utc>,
}

/// Record (or refresh the timestamp of) a viewed thread / profile.
pub fn record(kind: NavKind, key: &str, title: &str, subtitle: &str) -> Result<()> {
    crate::inbox::with_db(|conn| {
        conn.execute(
            r#"
            INSERT INTO nav_history (kind, key, title, subtitle, viewed_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(kind, key) DO UPDATE SET
                title     = excluded.title,
                subtitle  = excluded.subtitle,
                viewed_at = excluded.viewed_at
            "#,
            params![
                kind.as_db_str(),
                key,
                title,
                subtitle,
                Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
            ],
        )?;
        conn.execute(
            r#"
            DELETE FROM nav_history WHERE rowid IN (
                SELECT rowid FROM nav_history
                ORDER BY viewed_at DESC
                LIMIT -1 OFFSET ?1
            )
            "#,
            params![MAX_HISTORY_ROWS],
        )?;
        Ok(())
    })
}

/// Recently viewed entries, newest first.
pub fn list(limit: usize) -> Result<Vec<HistoryEntry>> {
    crate::inbox::with_db(|conn| {
        let mut stmt = conn.prepare(
            r#"
            SELECT kind, key, title, subtitle, viewed_at FROM nav_history
            ORDER BY viewed_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for (kind, key, title, subtitle, ts) in rows.flatten() {
            let (Some(kind), Ok(viewed_at)) = (
                NavKind::from_db_str(&kind),
                DateTime::parse_from_rfc3339(&ts),
            ) else {
                continue;
            };
            out.push(HistoryEntry {
                kind,
                key,
                title,
                subtitle,
                viewed_at: viewed_at.with_timezone(&Utc),
            });
        }
        Ok(out)
    })
}

pub fn clear() -> Result<()> {
    crate::inbox::with_db(|conn| {
        conn.execute("DELETE FROM nav_history", [])?;
        Ok(())
    })
}

/// Case-insensitive filter over title + subtitle for the History
/// sheet's search box. Empty query keeps everything.
pub fn filter<'a>(entries: &'a [HistoryEntry], query: &str) -> Vec<&'a HistoryEntry> {
    let q = query.trim().to_lowercase();
    entries
        .iter()
        .filter(|e| {
            q.is_empty()
                || e.title.to_lowercase().contains(&q)
                || e.subtitle.to_lowercase().contains(&q)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{install_fresh_test_db, TEST_DB_GUARD};

    #[test]
    fn clicking_through_builds_a_back_stack_and_back_forward_walk_it() {
        let mut s = NavStacks::default();
        s.observe(None, Some("a"));
        s.observe(Some("a"), Some("b"));
        s.observe(Some("b"), Some("c"));
        assert_eq!(s.back, vec!["a", "b"]);

        let t = s.go_back("c").unwrap();
        assert_eq!(t, "b");
        // The focus change the back button causes must not re-push.
        s.observe(Some("c"), Some("b"));
        assert_eq!(s.back, vec!["a"]);
        assert_eq!(s.forward, vec!["c"]);

        let t = s.go_forward("b").unwrap();
        assert_eq!(t, "c");
        s.observe(Some("b"), Some("c"));
        assert_eq!(s.back, vec!["a", "b"]);
        assert!(s.forward.is_empty());
    }

    #[test]
    fn a_new_click_after_going_back_drops_the_forward_trail() {
        let mut s = NavStacks::default();
        s.observe(None, Some("a"));
        s.observe(Some("a"), Some("b"));
        s.go_back("b");
        s.observe(Some("b"), Some("a"));
        assert!(s.can_go_forward());
        s.observe(Some("a"), Some("x"));
        assert!(!s.can_go_forward());
        assert_eq!(s.back, vec!["a"]);
    }

    #[test]
    fn fresh_open_resets_and_close_reports_the_key() {
        let mut s = NavStacks::default();
        s.observe(None, Some("a"));
        s.observe(Some("a"), Some("b"));
        assert_eq!(s.observe(Some("b"), None), Some("b".to_string()));
        s.observe(None, Some("z"));
        assert!(!s.can_go_back());
    }

    #[test]
    fn close_then_reopen_restores_the_trail() {
        let mut nav = NavHistory::default();
        nav.observe(NavKind::Thread, None, Some("a"));
        nav.observe(NavKind::Thread, Some("a"), Some("b"));
        nav.observe(NavKind::Thread, Some("b"), None);
        assert_eq!(nav.closed_seq, 1);
        assert!(!nav.thread.can_go_back());

        let (kind, key) = nav.take_reopen().unwrap();
        assert_eq!((kind, key.as_str()), (NavKind::Thread, "b"));
        // The reopen's own None → Some("b") must not wipe the trail.
        nav.observe(NavKind::Thread, None, Some("b"));
        assert_eq!(nav.thread.back, vec!["a"]);
        assert!(nav.last_closed.is_none());
    }

    #[test]
    fn most_recently_raised_sheet_is_on_top() {
        let mut nav = NavHistory::default();
        nav.observe(NavKind::Profile, None, Some("did:plc:a"));
        assert!(!nav.thread_above_profile());
        // A post clicked inside the profile opens a thread over it.
        nav.observe(NavKind::Thread, None, Some("at://p"));
        assert!(nav.thread_above_profile());
        // An avatar clicked in that thread brings the profile back up.
        nav.observe(NavKind::Profile, Some("did:plc:a"), Some("did:plc:b"));
        assert!(!nav.thread_above_profile());
    }

    #[test]
    fn stacks_are_bounded() {
        let mut s = NavStacks::default();
        s.observe(None, Some("0"));
        for i in 0..(MAX_STACK + 20) {
            s.observe(Some(&i.to_string()), Some(&(i + 1).to_string()));
        }
        assert_eq!(s.back.len(), MAX_STACK);
    }

    #[test]
    fn history_rows_upsert_list_filter_and_clear() {
        let _guard = TEST_DB_GUARD.lock();
        install_fresh_test_db();

        record(NavKind::Thread, "at://t1", "Alice", "a long thread").unwrap();
        record(NavKind::Profile, "did:plc:bob", "Bob", "@bob.bsky.social").unwrap();
        // Revisit: updates in place, moves to the top.
        record(
            NavKind::Thread,
            "at://t1",
            "Alice",
            "a long thread (edited)",
        )
        .unwrap();

        let all = list(50).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].key, "at://t1");
        assert_eq!(all[0].subtitle, "a long thread (edited)");

        let hits = filter(&all, "BOB");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, NavKind::Profile);
        assert_eq!(filter(&all, "  ").len(), 2);

        clear().unwrap();
        assert!(list(50).unwrap().is_empty());
    }
}
