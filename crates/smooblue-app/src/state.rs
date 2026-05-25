//! Global app state.
//!
//! Held in Dioxus contexts so any component can read/write without prop drilling:
//! - `Signal<Option<Session>>` — current OAuth session (None ⇒ logged out)
//! - `Signal<Vec<ColumnSpec>>` — column deck specification (which columns,
//!   in what order). Persisted to disk via [`crate::persistence`].

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use smooblue_oauth::Session;

/// What a single deck column shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnSpec {
    pub id: String,
    pub kind: ColumnKind,
    pub title: String,
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
}

impl ColumnSpec {
    pub fn home() -> Self {
        Self {
            id: "home".into(),
            kind: ColumnKind::Home,
            title: "Home".into(),
        }
    }

    pub fn notifications() -> Self {
        Self {
            id: "notifications".into(),
            kind: ColumnKind::Notifications,
            title: "Notifications".into(),
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
        }
    }

    pub fn search(query: impl Into<String>) -> Self {
        let q: String = query.into();
        Self {
            id: format!("search:{}", q),
            kind: ColumnKind::Search { query: q.clone() },
            title: format!("Search · {q}"),
        }
    }

    pub fn suggestions() -> Self {
        Self {
            id: "suggestions".into(),
            kind: ColumnKind::Suggestions,
            title: "Suggested follows".into(),
        }
    }

    pub fn list(uri: impl Into<String>, title: impl Into<String>) -> Self {
        let uri: String = uri.into();
        Self {
            id: format!("list:{uri}"),
            kind: ColumnKind::List { uri },
            title: title.into(),
        }
    }

    pub fn feed_with_title(uri: impl Into<String>, title: impl Into<String>) -> Self {
        let uri: String = uri.into();
        Self {
            id: format!("feed:{uri}"),
            kind: ColumnKind::Feed { uri },
            title: title.into(),
        }
    }

    pub fn author(actor: impl Into<String>, title: impl Into<String>) -> Self {
        let a: String = actor.into();
        Self {
            id: format!("author:{}", a),
            kind: ColumnKind::AuthorFeed { actor: a.clone() },
            title: title.into(),
        }
    }
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
}

/// Just enough of a parent post to render the quoted context in the
/// compose sheet and build the reply ref on submit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyTarget {
    pub uri: String,
    pub cid: String,
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
    use_context_provider::<Signal<Option<Session>>>(|| {
        // Demo mode (SMOOBLUE_DEMO=1) injects a synthetic session so the
        // app boots straight into the deck — no OAuth + no network.
        let initial = if crate::demo::is_active() {
            Some(crate::demo::fake_session())
        } else {
            crate::persistence::load_session()
        };
        Signal::new(initial)
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
                },
                ColumnSpec {
                    id: "notifs".into(),
                    kind: ColumnKind::Notifications,
                    title: "Notifications".into(),
                },
                ColumnSpec {
                    id: "discover".into(),
                    kind: ColumnKind::Home,
                    title: "Discover".into(),
                },
                ColumnSpec {
                    id: "rust".into(),
                    kind: ColumnKind::Home,
                    title: "Rust".into(),
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
            },
            ColumnSpec {
                id: "alice".into(),
                kind: ColumnKind::AuthorFeed {
                    actor: "alice.bsky.social".into(),
                },
                title: "Alice".into(),
            },
        ];
        let json = serde_json::to_string(&cols).unwrap();
        let back: Vec<ColumnSpec> = serde_json::from_str(&json).unwrap();
        assert_eq!(cols, back);
    }
}
