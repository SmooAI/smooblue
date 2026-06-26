//! Global app state.
//!
//! Held in Dioxus contexts so any component can read/write without prop drilling:
//! - `Signal<Option<Session>>` — current OAuth session (None ⇒ logged out)
//! - `Signal<Vec<ColumnSpec>>` — column deck specification (which columns,
//!   in what order). Persisted to disk via [`crate::persistence`].

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use smooblue_oauth::Session;

/// Per-column user settings. Persisted with the column. All fields
/// default to "off"/"default" so a column deserialized from an older
/// save (without this block) behaves exactly as before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ColumnSettings {
    /// Hide reposts (only first-hand posts).
    #[serde(default)]
    pub hide_reposts: bool,
    /// Hide replies (only top-level posts).
    #[serde(default)]
    pub hide_replies: bool,
    /// Only show posts that carry media (image / video / external card).
    #[serde(default)]
    pub media_only: bool,
    /// Only show text-only posts (no media embed).
    #[serde(default)]
    pub text_only: bool,
    /// Which notifications to show (Notifications columns only).
    #[serde(default)]
    pub notif_filter: NotifFilter,
    /// Auto-refresh cadence override.
    #[serde(default)]
    pub refresh: RefreshInterval,
}

/// Notification-type filter for a Notifications column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NotifFilter {
    /// Everything.
    #[default]
    All,
    /// Conversational: replies, mentions, quotes.
    Mentions,
    /// Reactions + graph: likes, reposts, follows.
    Reactions,
}

/// Per-column auto-refresh cadence. `Default` uses the per-kind
/// built-in interval; `Off` pauses live polling entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RefreshInterval {
    #[default]
    Default,
    Off,
    S15,
    S30,
    S60,
}

impl RefreshInterval {
    /// Resolve to a concrete poll duration. `None` means "paused".
    /// `Default` falls back to `fallback` (the per-kind interval).
    pub fn duration(self, fallback: std::time::Duration) -> Option<std::time::Duration> {
        use std::time::Duration;
        match self {
            RefreshInterval::Default => Some(fallback),
            RefreshInterval::Off => None,
            RefreshInterval::S15 => Some(Duration::from_secs(15)),
            RefreshInterval::S30 => Some(Duration::from_secs(30)),
            RefreshInterval::S60 => Some(Duration::from_secs(60)),
        }
    }
}

/// What a single deck column shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub id: String,
    pub kind: ColumnKind,
    pub title: String,
    /// Per-column user settings (filters, refresh cadence). Defaulted
    /// when absent so older saves load unchanged.
    #[serde(default)]
    pub settings: ColumnSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ColumnKind {
    /// `app.bsky.feed.getTimeline` — the Home feed.
    Home,
    // ── stubs for future columns; rendered as empty bodies for now ──
    Notifications,
    AuthorFeed {
        actor: String,
    },
    Search {
        query: String,
    },
    Feed {
        uri: String,
    },
    /// `app.bsky.feed.getListFeed` — posts from members of a curated
    /// list. Same shape as Feed/Home; the rendering side reuses the
    /// standard PostCard path.
    List {
        uri: String,
    },
    /// `app.bsky.actor.getSuggestions` — personalized list of actors
    /// the AppView thinks the viewer might want to follow. Renders as
    /// follow-row cards rather than posts.
    Suggestions,
    /// `chat.bsky.convo.listConvos` — Bluesky DMs (proxied through
    /// the user's PDS to `did:web:api.bsky.chat#bsky_chat`). Renders
    /// as a convo list; tapping a row opens MessagesSheet with the
    /// message history + compose input.
    Messages,
    /// Stripe-Inbox-style triage column combining direct replies,
    /// @mentions, quote-posts, and DMs into a single list sorted
    /// by directness score. Backed by the [`crate::inbox`] SQLite
    /// store — ingestion populates the DB on a poll, this column
    /// just reads it. Pearl th-e17045.
    Inbox,
    /// Account analytics dashboard — growth curves, posting cadence,
    /// and ranked follower/post lists. Backed by the [`crate::analytics`]
    /// SQLite store; this column reads the aggregated view DTO, the
    /// background ingest task populates the DB. Pearl account-analytics.
    Analytics,
}

impl ColumnSpec {
    pub fn home() -> Self {
        Self {
            id: "home".into(),
            kind: ColumnKind::Home,
            title: "Home".into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn notifications() -> Self {
        Self {
            id: "notifications".into(),
            kind: ColumnKind::Notifications,
            title: "Notifications".into(),
            settings: ColumnSettings::default(),
        }
    }

    /// Bluesky's "Discover" custom feed (the system whats-hot generator).
    /// AT-URI is well-known.
    pub fn discover() -> Self {
        Self {
            id: "discover".into(),
            kind: ColumnKind::Feed {
                uri: "at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.generator/whats-hot"
                    .into(),
            },
            title: "Discover".into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn search(query: impl Into<String>) -> Self {
        let q: String = query.into();
        Self {
            id: format!("search:{}", q),
            kind: ColumnKind::Search { query: q.clone() },
            title: format!("Search · {q}"),
            settings: ColumnSettings::default(),
        }
    }

    pub fn suggestions() -> Self {
        Self {
            id: "suggestions".into(),
            kind: ColumnKind::Suggestions,
            title: "Suggested follows".into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn messages() -> Self {
        Self {
            id: "messages".into(),
            kind: ColumnKind::Messages,
            title: "Messages".into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn inbox() -> Self {
        Self {
            id: "inbox".into(),
            kind: ColumnKind::Inbox,
            title: "Inbox".into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn analytics() -> Self {
        Self {
            id: "analytics".into(),
            kind: ColumnKind::Analytics,
            title: "Analytics".into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn list(uri: impl Into<String>, title: impl Into<String>) -> Self {
        let uri: String = uri.into();
        Self {
            id: format!("list:{uri}"),
            kind: ColumnKind::List { uri },
            title: title.into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn feed_with_title(uri: impl Into<String>, title: impl Into<String>) -> Self {
        let uri: String = uri.into();
        Self {
            id: format!("feed:{uri}"),
            kind: ColumnKind::Feed { uri },
            title: title.into(),
            settings: ColumnSettings::default(),
        }
    }

    pub fn author(actor: impl Into<String>, title: impl Into<String>) -> Self {
        let a: String = actor.into();
        Self {
            id: format!("author:{}", a),
            kind: ColumnKind::AuthorFeed { actor: a.clone() },
            title: title.into(),
            settings: ColumnSettings::default(),
        }
    }
}

/// Mutate one column's settings in place and persist the deck. No-op if
/// the id isn't found. Used by the per-column settings panel.
pub fn update_column_settings(
    cols: &mut Signal<Vec<ColumnSpec>>,
    id: &str,
    f: impl FnOnce(&mut ColumnSettings),
) {
    let mut list = cols.write();
    if let Some(c) = list.iter_mut().find(|c| c.id == id) {
        f(&mut c.settings);
    }
    let _ = crate::persistence::save_columns(&list);
}

/// Append a column to the deck if no column with the same id is already
/// present. Persists the new layout to disk.
pub fn add_column_unique(cols: &mut Signal<Vec<ColumnSpec>>, spec: ColumnSpec) {
    let mut list = cols.write();
    if list.iter().any(|c| c.id == spec.id) {
        return;
    }
    list.push(spec);
    let _ = crate::persistence::save_columns(&list);
}

/// Add the column if missing, then signal the deck to scroll the
/// matching column into view + briefly flash its border so the
/// user can see *where* it went. Tick is always bumped so repeat
/// clicks on the same sidebar button keep triggering the scroll
/// (signal equality on `id` alone would drop redundant sets).
pub fn add_or_focus_column(
    cols: &mut Signal<Vec<ColumnSpec>>,
    focus: &mut Signal<FocusColumn>,
    spec: ColumnSpec,
) {
    let id = spec.id.clone();
    add_column_unique(cols, spec);
    let mut w = focus.write();
    w.id = Some(id);
    w.tick = w.tick.wrapping_add(1);
}

/// Remove the column with the given id and persist.
pub fn remove_column(cols: &mut Signal<Vec<ColumnSpec>>, id: &str) {
    let mut list = cols.write();
    list.retain(|c| c.id != id);
    let _ = crate::persistence::save_columns(&list);
}

/// Move the column with id `dragged` to the position currently held by
/// `target`. Persists. No-op when either id is missing or the two are
/// the same. Insertion side is "before" the target, which gives the
/// usual deck.blue/Tweetdeck feel: drop on the left half of a column
/// → it lands before; drop on the right doesn't currently distinguish
/// (we'd need pixel-position math at the call site).
pub fn move_column(cols: &mut Signal<Vec<ColumnSpec>>, dragged: &str, target: &str) {
    if dragged == target {
        return;
    }
    let mut list = cols.write();
    let Some(src) = list.iter().position(|c| c.id == dragged) else {
        return;
    };
    let spec = list.remove(src);
    // Re-find the target after removal in case it shifted.
    let dst = list
        .iter()
        .position(|c| c.id == target)
        .unwrap_or(list.len());
    list.insert(dst, spec);
    let _ = crate::persistence::save_columns(&list);
}

/// Transient drag state shared between every column header. Lives in
/// a context so the dragged-column shrink + drop-target highlight can
/// both render without prop-drilling through the whole deck.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ColumnDrag {
    /// Id of the column currently being dragged, or None when idle.
    pub dragging: Option<String>,
    /// Id of the column currently hovered as a drop target.
    pub target: Option<String>,
}

/// Per-post optimistic state for likes + reposts. Lives in a
/// context-shared map keyed by the post's AT-URI so the optimistic flip
/// survives column re-renders triggered by polling.
///
/// We track the *intended* server state (liked / reposted, plus the
/// record URIs we got back) so the next polling cycle reconciles cleanly.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OptimisticPostState {
    /// `true` if the user has liked this post locally (whether or not the
    /// server has confirmed yet). `false` if they explicitly un-liked.
    pub liked: Option<bool>,
    /// AT-URI of our like record once createRecord returned. Needed to
    /// call deleteRecord on un-like.
    pub like_uri: Option<String>,
    /// Same shape as `liked`, but for reposts.
    pub reposted: Option<bool>,
    /// AT-URI of our repost record.
    pub repost_uri: Option<String>,
}

pub type OptimisticMap = std::collections::HashMap<String, OptimisticPostState>;

/// Compose-sheet context. When `reply_to` is `Some`, the sheet renders in
/// reply mode (parent shown above the textarea, posts with a reply ref).
/// When `quote_to` is `Some`, the sheet renders the quoted post above
/// the textarea + attaches an `app.bsky.embed.record` to the new post
/// on submit. Reply + quote are mutually exclusive in the UI — picking
/// quote clears reply and vice versa.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ComposeContext {
    pub open: bool,
    pub reply_to: Option<ReplyTarget>,
    pub quote_to: Option<QuoteTarget>,
    /// One-shot draft text to seed the composer with when it opens —
    /// handed off from another surface (e.g. the inbox quick-reply
    /// "pop out" button so an in-progress reply isn't lost). The
    /// composer consumes and clears it on open.
    pub prefill: Option<String>,
}

/// Just enough of a parent post to render the quoted context in the
/// compose sheet and build the reply ref on submit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyTarget {
    pub uri: String,
    pub cid: String,
    /// Thread-root strong ref (`uri`, `cid`) that the new reply must
    /// carry as its own `reply.root`. For a reply to a top-level post
    /// this equals (`uri`, `cid`); for a reply to a deeper post it's
    /// the ancestor thread root, propagated so bsky threads the reply
    /// under the real conversation instead of orphaning it.
    pub root_uri: String,
    pub root_cid: String,
    pub handle: String,
    pub text: String,
}

/// The post being quoted in a compose. Same shape as ReplyTarget —
/// kept as its own type so render code can distinguish quote-context
/// from reply-context at a glance (different visual treatment).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteTarget {
    pub uri: String,
    pub cid: String,
    pub handle: String,
    pub text: String,
}

/// Vim-style cursor for the deck — which column and which item-
/// within-column the user is "on" for keyboard navigation. Updated
/// by j/k/h/l etc. PostCard reads this to highlight the focused
/// card; ColumnHeader reads it to highlight the focused column.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusedItem {
    pub column: usize,
    pub item: usize,
}

/// Open state for the keyboard-shortcut help overlay. Toggled by `?`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct KeyboardHelp(pub bool);

/// Set on boot if the GitHub releases API reports a newer tag than
/// CARGO_PKG_VERSION. The deck renders a small dismissible toast
/// linking to the release page when this is `Some`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct UpdateBanner(pub Option<crate::updates::UpdateAvailable>);

/// Current visual theme. Default Dark — the brand sheet from
/// smooai-ui is dark-first; Light is implemented as a CSS token
/// override scoped to `[data-theme="light"]`. Persisted across
/// launches via [`crate::persistence::save_theme`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn as_attr(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// Parse from the persisted string. Named `parse` rather than
    /// `from_str` so we don't collide with `std::str::FromStr` (which
    /// would require a different return type).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            _ => Self::Dark,
        }
    }
}

/// Open state for the profile editor sheet. Toggled by the
/// "Edit profile" button on the own-profile view.
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct ProfileEditOpen(pub bool);

/// Open state for the full-screen Analytics "pop-out" page. Toggled by
/// the expand button on the Analytics column header.
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct AnalyticsExpanded(pub bool);

/// Transient "scroll-into-view + flash this column" signal. Set
/// by the sidebar nav buttons (and anywhere else that adds a
/// column) so the user can see *where* the requested column went
/// — both when it was just added and when it was already there.
///
/// The `tick` counter is bumped on each set so consecutive
/// requests for the SAME column id (e.g. mashing the Notifications
/// button) still trigger a fresh scroll/flash even though `id`
/// didn't change. Signal equality on the outer struct would
/// otherwise drop the redundant set.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct FocusColumn {
    pub id: Option<String>,
    pub tick: u64,
}

/// In-app lightbox for images / videos. `None` ⇒ closed; `Some(item)`
/// ⇒ render a full-screen overlay with the media centered.
/// Clicks on a post image / video populate this rather than shelling
/// out to the system browser — the browser jump is jarring and
/// drops the user out of the deck.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LightboxFocus(pub Option<LightboxItem>);

#[derive(Clone, PartialEq, Eq)]
pub enum LightboxItem {
    /// `url` is the fullsize CDN URL. `alt` shown as title attribute
    /// for screen readers.
    Image { url: String, alt: String },
    /// `url` is the bsky video CDN URL (or whatever the embed view
    /// resolved to). WKWebView's native `<video>` decodes mp4 + m3u8
    /// directly.
    Video { url: String, poster: Option<String> },
}

/// What the report sheet is targeting (account or post). `None` when
/// the sheet is closed. Set by ProfileSheet's Report action or a
/// future per-post menu.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ReportFocus(pub Option<crate::components::report_sheet::ReportTarget>);

/// Target + anchor for the post overflow ("…") menu. Rendered at the
/// deck level (PostMenu) rather than inside the card, so the popover
/// escapes the column's `overflow` and the post's `contain: paint`
/// clip box — otherwise it gets cut off at the card edge. `x`/`y` are
/// the click's viewport coordinates; the menu anchors there.
#[derive(Clone, PartialEq)]
pub struct PostMenuState {
    pub uri: String,
    pub cid: String,
    pub author_did: String,
    pub author_handle: String,
    pub is_mine: bool,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Default, PartialEq)]
pub struct PostMenuFocus(pub Option<PostMenuState>);

/// URIs the user deleted this session. PostCard renders nothing for
/// these immediately; the feed drops them for real on the next poll.
#[derive(Clone, Default, PartialEq)]
pub struct DeletedPosts(pub std::collections::HashSet<String>);

/// Tracks an in-flight key chord. Vim has two-key chords like `gg`
/// (top), `gh` (home column), `gd` (discover), `gp` (profile). When
/// the user types `g`, we set this to `Some(now)`; the next key
/// either consumes the chord or times out 1.5s later via the
/// keyboard handler clearing it. Leader chords (`<space>X`) use
/// the same field.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PendingChord {
    /// `Some("g")` after a `g` press waiting for the second key.
    /// `Some(" ")` after `<space>` (leader) waiting.
    pub prefix: Option<String>,
}

/// Which actor (if any) the user has currently focused into a
/// profile view. `None` ⇒ closed; `Some(actor)` ⇒ the ProfileSheet
/// loads + renders the profile + recent posts for this DID or handle.
/// Either form is accepted by app.bsky.actor.getProfile.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProfileFocus(pub Option<String>);

/// What the EngagementSheet should show — the modal that opens when
/// the user taps a like/repost/quote count on a post card.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct EngagementFocus(pub Option<Engagement>);

#[derive(Clone, PartialEq, Eq)]
pub enum Engagement {
    /// `app.bsky.feed.getLikes` for the given post URI.
    Likes(String),
    /// `app.bsky.feed.getRepostedBy` for the given post URI.
    Reposters(String),
    /// `app.bsky.feed.getQuotes` for the given post URI.
    Quotes(String),
}

/// Which post (if any) the user has currently focused into a thread
/// view. `None` ⇒ closed; `Some(uri)` ⇒ the ThreadSheet loads + renders
/// the conversation around this AT-URI.
///
/// Wrapped in its own context (rather than reusing ComposeContext)
/// because (a) thread and compose can both be open in different
/// session flows, and (b) the close-on-click-backdrop semantics are
/// the same modal pattern but the data lifecycle differs.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ThreadFocus(pub Option<String>);

/// Which DM conversation `MessagesSheet` is currently showing.
/// `None` ⇒ closed; `Some(convo_id)` ⇒ the sheet loads message
/// history for that convo, polls for new messages, marks-read on
/// open, and renders the compose input. Parallel structure to
/// [`ThreadFocus`].
#[derive(Clone, Default, PartialEq, Eq)]
pub struct MessagesFocus(pub Option<String>);

/// Global tick counter, bumped every second by [`DeckShell`]'s tick task.
/// Components that render time-relative text (post / notification
/// timestamps) read this signal so their render re-runs each tick —
/// that's how "11s" becomes "12s" without a manual refresh.
#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub struct Tick(pub u64);

/// Install global signals into the Dioxus context root.
/// Idempotent — safe to call on every render.
pub fn use_bootstrap() {
    use_context_provider::<Signal<Tick>>(|| Signal::new(Tick(0)));
    use_context_provider::<Signal<OptimisticMap>>(|| Signal::new(OptimisticMap::new()));
    use_context_provider::<Signal<ComposeContext>>(|| {
        // SMOOBLUE_DEBUG_OPEN_COMPOSE=1 → boot straight into the compose
        // sheet. Useful for screenshots and iterating the UI.
        let open = std::env::var("SMOOBLUE_DEBUG_OPEN_COMPOSE")
            .ok()
            .filter(|v| v == "1")
            .is_some();
        Signal::new(ComposeContext {
            open,
            reply_to: None,
            quote_to: None,
            prefill: None,
        })
    });
    use_context_provider::<Signal<ColumnDrag>>(|| Signal::new(ColumnDrag::default()));
    use_context_provider::<Signal<EngagementFocus>>(|| {
        // SMOOBLUE_DEBUG_OPEN_ENGAGEMENT=likes|reposters|quotes opens
        // the corresponding sheet against a synthetic post URI on boot.
        // Useful for screenshots and iterating the UI.
        let initial = std::env::var("SMOOBLUE_DEBUG_OPEN_ENGAGEMENT")
            .ok()
            .and_then(|v| {
                let uri = "at://did:plc:demo/app.bsky.feed.post/demo".to_string();
                match v.as_str() {
                    "likes" => Some(Engagement::Likes(uri)),
                    "reposters" => Some(Engagement::Reposters(uri)),
                    "quotes" => Some(Engagement::Quotes(uri)),
                    _ => None,
                }
            });
        Signal::new(EngagementFocus(initial))
    });
    use_context_provider::<Signal<FocusedItem>>(|| Signal::new(FocusedItem::default()));
    use_context_provider::<Signal<KeyboardHelp>>(|| Signal::new(KeyboardHelp(false)));
    use_context_provider::<Signal<UpdateBanner>>(|| Signal::new(UpdateBanner::default()));
    use_context_provider::<Signal<ReportFocus>>(|| Signal::new(ReportFocus::default()));
    use_context_provider::<Signal<PostMenuFocus>>(|| Signal::new(PostMenuFocus::default()));
    use_context_provider::<Signal<DeletedPosts>>(|| Signal::new(DeletedPosts::default()));
    use_context_provider::<Signal<ProfileEditOpen>>(|| Signal::new(ProfileEditOpen(false)));
    use_context_provider::<Signal<AnalyticsExpanded>>(|| Signal::new(AnalyticsExpanded(false)));
    use_context_provider::<Signal<FocusColumn>>(|| Signal::new(FocusColumn::default()));
    use_context_provider::<Signal<LightboxFocus>>(|| Signal::new(LightboxFocus::default()));
    use_context_provider::<Signal<ThemeMode>>(|| {
        let mode = crate::persistence::load_theme()
            .map(|s| ThemeMode::parse(&s))
            .unwrap_or_default();
        Signal::new(mode)
    });
    use_context_provider::<Signal<PendingChord>>(|| Signal::new(PendingChord::default()));
    use_context_provider::<Signal<ProfileFocus>>(|| {
        // SMOOBLUE_DEBUG_OPEN_PROFILE=<handle-or-did> → boot straight
        // into a profile view. `demo` resolves to the synth demo actor.
        let initial = std::env::var("SMOOBLUE_DEBUG_OPEN_PROFILE")
            .ok()
            .filter(|v| !v.is_empty())
            .map(|v| {
                if v == "demo" {
                    "you.bsky.social".to_string()
                } else {
                    v
                }
            });
        Signal::new(ProfileFocus(initial))
    });
    use_context_provider::<Signal<ThreadFocus>>(|| {
        // SMOOBLUE_DEBUG_OPEN_THREAD=<at-uri> → boot straight into a
        // thread view for that URI. In demo mode the special value
        // `demo` resolves to the synthesized demo thread.
        let initial = std::env::var("SMOOBLUE_DEBUG_OPEN_THREAD")
            .ok()
            .filter(|v| !v.is_empty())
            .map(|v| {
                if v == "demo" {
                    "at://did:plc:demo/app.bsky.feed.post/thread-root".to_string()
                } else {
                    v
                }
            });
        Signal::new(ThreadFocus(initial))
    });
    use_context_provider::<Signal<MessagesFocus>>(|| Signal::new(MessagesFocus(None)));
    use_context_provider::<Signal<Option<Session>>>(|| {
        // Demo mode (SMOOBLUE_DEMO=1) injects a synthetic session so the
        // app boots straight into the deck — no OAuth + no network.
        let initial = if crate::demo::is_active() {
            Some(crate::demo::fake_session())
        } else {
            // Multi-account: prefer the keyed session for the active
            // DID from accounts.json; fall back to the legacy single
            // slot. Between the two, prefer the one with the freshest
            // token / latest expiry so an out-of-sync write (e.g. a
            // refresh that only landed in one slot) doesn't ship an
            // already-dead session to the boot path.
            let accounts = crate::persistence::load_accounts();
            let keyed = accounts
                .active_did
                .as_deref()
                .and_then(crate::persistence::load_session_for);
            let legacy = crate::persistence::load_session();
            match (keyed, legacy) {
                (Some(k), Some(l)) => {
                    if l.expires_at > k.expires_at {
                        Some(l)
                    } else {
                        Some(k)
                    }
                }
                (Some(k), None) => Some(k),
                (None, Some(l)) => Some(l),
                (None, None) => None,
            }
        };
        Signal::new(initial)
    });
    // Multi-account index — kept as its own signal so the settings
    // sheet can list other accounts and the sidebar can offer a
    // quick switcher without round-tripping through the session
    // signal (which only carries the *active* account's tokens).
    use_context_provider::<Signal<crate::persistence::Accounts>>(|| {
        if crate::demo::is_active() {
            return Signal::new(crate::persistence::Accounts::default());
        }
        Signal::new(crate::persistence::load_accounts())
    });
    use_context_provider::<Signal<Vec<ColumnSpec>>>(|| {
        let initial = if crate::demo::is_active() {
            // Multi-column deck for the screenshot. Each column reuses the
            // demo Home feed so the layout reads as fully-populated even
            // before the non-Home column kinds have demo data.
            vec![
                ColumnSpec {
                    id: "home".into(),
                    kind: ColumnKind::Home,
                    title: "Home".into(),
                    settings: ColumnSettings::default(),
                },
                ColumnSpec {
                    id: "notifs".into(),
                    kind: ColumnKind::Notifications,
                    title: "Notifications".into(),
                    settings: ColumnSettings::default(),
                },
                ColumnSpec {
                    id: "discover".into(),
                    kind: ColumnKind::Home,
                    title: "Discover".into(),
                    settings: ColumnSettings::default(),
                },
                ColumnSpec {
                    id: "rust".into(),
                    kind: ColumnKind::Home,
                    title: "Rust".into(),
                    settings: ColumnSettings::default(),
                },
            ]
        } else {
            crate::persistence::load_columns().unwrap_or_else(|| vec![ColumnSpec::home()])
        };
        Signal::new(initial)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_spec_has_expected_shape() {
        let h = ColumnSpec::home();
        assert_eq!(h.id, "home");
        assert_eq!(h.title, "Home");
        assert!(matches!(h.kind, ColumnKind::Home));
    }

    #[test]
    fn move_column_reorders_in_place() {
        // Pure logic check on a Vec — same algorithm as the Signal
        // path but easier to test without a Dioxus runtime.
        let mut list = vec![
            ColumnSpec::home(),
            ColumnSpec::notifications(),
            ColumnSpec::search("rust"),
        ];
        // Simulate the move_column algorithm directly.
        let dragged = "search:rust";
        let target = "home";
        let src = list.iter().position(|c| c.id == dragged).unwrap();
        let spec = list.remove(src);
        let dst = list
            .iter()
            .position(|c| c.id == target)
            .unwrap_or(list.len());
        list.insert(dst, spec);
        // search:rust should now be first.
        assert_eq!(list[0].id, "search:rust");
        assert_eq!(list[1].id, "home");
        assert_eq!(list[2].id, "notifications");
    }

    #[test]
    fn column_specs_round_trip_through_serde() {
        let cols = vec![
            ColumnSpec::home(),
            ColumnSpec {
                id: "notifs".into(),
                kind: ColumnKind::Notifications,
                title: "Notifications".into(),
                settings: ColumnSettings::default(),
            },
            ColumnSpec {
                id: "alice".into(),
                kind: ColumnKind::AuthorFeed {
                    actor: "alice.bsky.social".into(),
                },
                title: "Alice".into(),
                settings: ColumnSettings::default(),
            },
        ];
        let json = serde_json::to_string(&cols).unwrap();
        let back: Vec<ColumnSpec> = serde_json::from_str(&json).unwrap();
        assert_eq!(cols, back);
    }
}
