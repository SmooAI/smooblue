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
}

impl ColumnSpec {
    pub fn home() -> Self {
        Self {
            id: "home".into(),
            kind: ColumnKind::Home,
            title: "Home".into(),
        }
    }
}

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
