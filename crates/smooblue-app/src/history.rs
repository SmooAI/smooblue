//! Navigation history — browser-style back / forward across the deck
//! and the thread / profile sheets, "reopen what I just closed", and a
//! persisted list of recently viewed threads and profiles.
//!
//! Both sheets are modals over the deck, and a modal is easy to lose:
//! one stray click on the backdrop and a 200-reply thread you were
//! halfway through is gone. [`NavHistory`] keeps one timeline of what
//! was on screen (the deck included) for ← / →; its `last_closed` backs
//! the Reopen button, ⌘⇧T and the reopen toast; and the `nav_history`
//! table (schema v7) backs the History sheet so a thread from
//! yesterday is still one click away.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;

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

/// Oldest timeline entries are dropped past this many. Plenty for a
/// session of reading; bounded so it can't grow forever.
const MAX_TIMELINE: usize = 200;

/// What's on screen: the deck (no sheet), or the thread / profile
/// sheet on top, by key (post AT-URI / actor DID-or-handle).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Deck,
    Thread(String),
    Profile(String),
}

impl View {
    pub fn kind(&self) -> Option<NavKind> {
        match self {
            View::Deck => None,
            View::Thread(_) => Some(NavKind::Thread),
            View::Profile(_) => Some(NavKind::Profile),
        }
    }
}

/// App-wide, browser-style navigation (a Dioxus context signal).
///
/// One **timeline** of everything shown, with the deck itself as a
/// stop on it — not a per-sheet stack. That's what makes back/forward
/// useful in real use: you open a thread from a column, close it, open
/// a profile, open a post from it… and ← walks back through exactly
/// that, including "back to the deck" and "back into the thread I just
/// closed". (The first cut kept a back stack per sheet that reset on
/// every open from the deck, so ← only ever lit up after clicking
/// between replies *inside* one thread.)
///
/// [`observe`](Self::observe) is fed the two sheets' focus after every
/// change; anything not caused by [`go_back`](Self::go_back) /
/// [`go_forward`](Self::go_forward) is a new visit (truncating the
/// forward half, as a browser does).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavHistory {
    timeline: Vec<View>,
    cursor: usize,
    /// The view a back / forward is taking us to, so the focus change
    /// it causes isn't recorded as a fresh visit.
    pending: Option<View>,
    /// Most recently *closed* sheet (backdrop, ×, Esc — not a back /
    /// forward), for the Reopen button, ⌘⇧T and the reopen toast.
    pub last_closed: Option<View>,
    /// Bumped on every such close so the toast re-shows even when the
    /// same thread is closed twice in a row.
    pub closed_seq: u64,
    prev_thread: Option<String>,
    prev_profile: Option<String>,
    /// When each sheet last came to the front. Thread and profile
    /// sheets stack either way round (a post clicked inside a profile
    /// opens a thread *over* it), so paint order + Esc follow this.
    raise_seq: u64,
    thread_raised: u64,
    profile_raised: u64,
}

impl Default for NavHistory {
    fn default() -> Self {
        Self {
            timeline: vec![View::Deck],
            cursor: 0,
            pending: None,
            last_closed: None,
            closed_seq: 0,
            prev_thread: None,
            prev_profile: None,
            raise_seq: 0,
            thread_raised: 0,
            profile_raised: 0,
        }
    }
}

impl NavHistory {
    pub fn current(&self) -> &View {
        &self.timeline[self.cursor]
    }

    pub fn can_go_back(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.cursor + 1 < self.timeline.len()
    }

    /// The view ← would show (for tooltips).
    pub fn back_target(&self) -> Option<&View> {
        self.cursor.checked_sub(1).map(|i| &self.timeline[i])
    }

    /// The view → would show (for tooltips).
    pub fn forward_target(&self) -> Option<&View> {
        self.timeline.get(self.cursor + 1)
    }

    /// Whether the thread sheet was brought forward more recently than
    /// the profile sheet (so it paints — and Esc-closes — first).
    pub fn thread_above_profile(&self) -> bool {
        self.thread_raised > self.profile_raised
    }

    /// The view on top for these sheet focuses.
    fn top(&self, thread: Option<&str>, profile: Option<&str>) -> View {
        match (thread, profile) {
            (Some(t), Some(_)) if self.thread_above_profile() => View::Thread(t.to_string()),
            (_, Some(p)) => View::Profile(p.to_string()),
            (Some(t), None) => View::Thread(t.to_string()),
            (None, None) => View::Deck,
        }
    }

    /// Feed the current focus of both sheets (blank keys count as
    /// closed). Updates stacking order, the timeline and last-closed.
    pub fn observe(&mut self, thread: Option<&str>, profile: Option<&str>) {
        let thread = thread.filter(|k| !k.trim().is_empty());
        let profile = profile.filter(|k| !k.trim().is_empty());
        if thread.is_some() && thread != self.prev_thread.as_deref() {
            self.raise_seq += 1;
            self.thread_raised = self.raise_seq;
        }
        if profile.is_some() && profile != self.prev_profile.as_deref() {
            self.raise_seq += 1;
            self.profile_raised = self.raise_seq;
        }
        let top = self.top(thread, profile);
        let via_nav = self.pending.take().is_some_and(|p| p == top);
        if !via_nav {
            // A sheet that went away on its own is a "close" worth
            // offering back. Profile is checked last so, if both close
            // at once, the one that was on top wins.
            if let (Some(t), None) = (self.prev_thread.clone(), thread) {
                self.last_closed = Some(View::Thread(t));
                self.closed_seq = self.closed_seq.wrapping_add(1);
            }
            if let (Some(p), None) = (self.prev_profile.clone(), profile) {
                self.last_closed = Some(View::Profile(p));
                self.closed_seq = self.closed_seq.wrapping_add(1);
            }
            self.visit(top);
        }
        self.prev_thread = thread.map(str::to_string);
        self.prev_profile = profile.map(str::to_string);
    }

    /// Record a fresh visit: drop the forward half, append, bound.
    fn visit(&mut self, view: View) {
        if self.timeline[self.cursor] == view {
            return;
        }
        self.timeline.truncate(self.cursor + 1);
        self.timeline.push(view);
        if self.timeline.len() > MAX_TIMELINE {
            self.timeline.remove(0);
        }
        self.cursor = self.timeline.len() - 1;
    }

    /// Step back. Returns the view to show; the caller applies it.
    pub fn go_back(&mut self) -> Option<View> {
        let target = self.back_target()?.clone();
        self.cursor -= 1;
        self.pending = Some(target.clone());
        Some(target)
    }

    /// Step forward. Returns the view to show; the caller applies it.
    pub fn go_forward(&mut self) -> Option<View> {
        let target = self.forward_target()?.clone();
        self.cursor += 1;
        self.pending = Some(target.clone());
        Some(target)
    }

    /// The closed sheet to reopen, if it isn't already open again. The
    /// caller shows it as a fresh visit.
    pub fn take_reopen(&mut self, thread_open: bool, profile_open: bool) -> Option<View> {
        let still_closed = match self.last_closed.as_ref()? {
            View::Thread(_) => !thread_open,
            View::Profile(_) => !profile_open,
            View::Deck => false,
        };
        if still_closed {
            self.last_closed.take()
        } else {
            None
        }
    }
}

// ── Signal-level actions (buttons + keyboard) ───────────────────────

use crate::state::{ProfileFocus, ThreadFocus};
use dioxus::prelude::*;

/// The sheet on top — the one Esc closes first when both the thread
/// and profile sheets are open.
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

/// Make `view` what's on screen: exactly that sheet open (or none, for
/// the deck).
fn show(view: View, mut thread: Signal<ThreadFocus>, mut profile: Signal<ProfileFocus>) {
    let (t, p) = match view {
        View::Deck => (None, None),
        View::Thread(k) => (Some(k), None),
        View::Profile(k) => (None, Some(k)),
    };
    if thread.peek().0 != t {
        thread.set(ThreadFocus(t));
    }
    if profile.peek().0 != p {
        profile.set(ProfileFocus(p));
    }
}

/// ← / → (buttons, ⌘[ / ⌘]). Returns whether anything moved.
pub fn step(
    mut nav: Signal<NavHistory>,
    thread: Signal<ThreadFocus>,
    profile: Signal<ProfileFocus>,
    forward: bool,
) -> bool {
    let target = {
        let mut n = nav.write();
        if forward {
            n.go_forward()
        } else {
            n.go_back()
        }
    };
    match target {
        Some(view) => {
            show(view, thread, profile);
            true
        }
        None => false,
    }
}

/// Reopen the most recently closed thread / profile (button, ⌘⇧T,
/// toast). Returns whether anything reopened.
pub fn reopen_last(
    mut nav: Signal<NavHistory>,
    mut thread: Signal<ThreadFocus>,
    mut profile: Signal<ProfileFocus>,
) -> bool {
    let thread_open = thread.peek().0.is_some();
    let profile_open = profile.peek().0.is_some();
    let Some(view) = nav.write().take_reopen(thread_open, profile_open) else {
        return false;
    };
    // Reopen stacks the sheet back on top of whatever is open, rather
    // than replacing it — a fresh visit, not a jump.
    match view {
        View::Thread(k) => thread.set(ThreadFocus(Some(k))),
        View::Profile(k) => profile.set(ProfileFocus(Some(k))),
        View::Deck => {}
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

/// Mirror both sheets' focus into [`NavHistory`]. Mount once (the
/// deck shell): one observer sees both signals together, so a back /
/// forward that changes both lands as a single step.
pub fn use_nav_observer() {
    let mut nav = use_context::<Signal<NavHistory>>();
    let thread = use_context::<Signal<ThreadFocus>>();
    let profile = use_context::<Signal<ProfileFocus>>();
    use_effect(move || {
        let t = thread.read().0.clone();
        let p = profile.read().0.clone();
        nav.write().observe(t.as_deref(), p.as_deref());
    });
}

/// Short tooltip label for a view ("the deck", "a thread", …).
pub fn describe(view: &View) -> &'static str {
    match view {
        View::Deck => "the deck",
        View::Thread(_) => "a thread",
        View::Profile(_) => "a profile",
    }
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

    fn t(k: &str) -> View {
        View::Thread(k.into())
    }
    fn p(k: &str) -> View {
        View::Profile(k.into())
    }

    /// Replay a back/forward the way the signal layer does: step, then
    /// observe the focus it produces.
    fn apply(nav: &mut NavHistory, view: View) {
        let (th, pr) = match &view {
            View::Deck => (None, None),
            View::Thread(k) => (Some(k.clone()), None),
            View::Profile(k) => (None, Some(k.clone())),
        };
        nav.observe(th.as_deref(), pr.as_deref());
    }

    #[test]
    fn opening_one_thread_from_the_deck_already_enables_back() {
        // The real-use complaint: open a thread from a column → ← was
        // disabled. Now ← goes back to the deck.
        let mut nav = NavHistory::default();
        assert!(!nav.can_go_back());
        nav.observe(Some("a"), None);
        assert!(nav.can_go_back());
        assert_eq!(nav.back_target(), Some(&View::Deck));
    }

    #[test]
    fn back_walks_across_separate_opens_and_closes() {
        let mut nav = NavHistory::default();
        nav.observe(Some("a"), None); // open thread a from a column
        nav.observe(None, None); // close it
        nav.observe(None, Some("bob")); // open a profile
        nav.observe(Some("b"), Some("bob")); // a post inside the profile
        assert_eq!(nav.current(), &t("b"));

        let v = nav.go_back().unwrap();
        assert_eq!(v, p("bob"));
        apply(&mut nav, v);
        let v = nav.go_back().unwrap();
        assert_eq!(v, View::Deck);
        apply(&mut nav, v);
        // …and back into the thread that was closed before all that.
        let v = nav.go_back().unwrap();
        assert_eq!(v, t("a"));
        apply(&mut nav, v);
        assert!(nav.can_go_back()); // to the initial deck
        assert!(nav.can_go_forward());

        // Forward retraces without rewriting history.
        let v = nav.go_forward().unwrap();
        assert_eq!(v, View::Deck);
        apply(&mut nav, v);
        assert_eq!(nav.forward_target(), Some(&p("bob")));
    }

    #[test]
    fn a_new_visit_after_going_back_drops_the_forward_half() {
        let mut nav = NavHistory::default();
        nav.observe(Some("a"), None);
        nav.observe(Some("b"), None);
        let v = nav.go_back().unwrap();
        apply(&mut nav, v);
        assert!(nav.can_go_forward());
        nav.observe(Some("x"), None);
        assert!(!nav.can_go_forward());
        assert_eq!(nav.back_target(), Some(&t("a")));
    }

    #[test]
    fn closing_offers_reopen_but_navigating_away_does_not() {
        let mut nav = NavHistory::default();
        nav.observe(Some("a"), None);
        nav.observe(None, None); // backdrop click
        assert_eq!(nav.last_closed, Some(t("a")));
        assert_eq!(nav.closed_seq, 1);
        assert_eq!(nav.take_reopen(false, false), Some(t("a")));

        // ← from a thread back to the deck is navigation, not a "close".
        let mut nav = NavHistory::default();
        nav.observe(Some("a"), None);
        let v = nav.go_back().unwrap();
        apply(&mut nav, v);
        assert_eq!(nav.last_closed, None);
        assert_eq!(nav.closed_seq, 0);
    }

    #[test]
    fn reopen_is_offered_only_while_the_sheet_is_still_closed() {
        let mut nav = NavHistory::default();
        nav.observe(Some("a"), None);
        nav.observe(None, None);
        assert_eq!(nav.take_reopen(true, false), None);
        assert_eq!(nav.take_reopen(false, false), Some(t("a")));
        assert_eq!(nav.take_reopen(false, false), None);
    }

    #[test]
    fn most_recently_raised_sheet_is_on_top() {
        let mut nav = NavHistory::default();
        nav.observe(None, Some("did:plc:a"));
        assert!(!nav.thread_above_profile());
        assert_eq!(nav.current(), &p("did:plc:a"));
        // A post clicked inside the profile opens a thread over it.
        nav.observe(Some("at://p"), Some("did:plc:a"));
        assert!(nav.thread_above_profile());
        assert_eq!(nav.current(), &t("at://p"));
        // An avatar clicked in that thread brings a profile back up.
        nav.observe(Some("at://p"), Some("did:plc:b"));
        assert!(!nav.thread_above_profile());
        assert_eq!(nav.current(), &p("did:plc:b"));
    }

    #[test]
    fn blank_keys_count_as_closed_and_never_become_entries() {
        let mut nav = NavHistory::default();
        nav.observe(None, Some("  "));
        assert!(!nav.can_go_back());
        assert_eq!(nav.last_closed, None);
    }

    #[test]
    fn timeline_is_bounded() {
        let mut nav = NavHistory::default();
        for i in 0..(MAX_TIMELINE + 50) {
            nav.observe(Some(&i.to_string()), None);
        }
        assert_eq!(nav.timeline.len(), MAX_TIMELINE);
        assert_eq!(nav.cursor, MAX_TIMELINE - 1);
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
