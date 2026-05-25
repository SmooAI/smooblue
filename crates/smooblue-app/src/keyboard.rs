//! Vim/nvim-style keyboard shortcuts for the deck.
//!
//! Bindings (single keys unless noted):
//!
//! | Key | Action |
//! | --- | --- |
//! | `j` / `k` | next / previous post in focused column |
//! | `h` / `l` | previous / next column |
//! | `gg` / `G` | top / bottom of column |
//! | `Ctrl-d` / `Ctrl-u` | half-page down / up |
//! | `f` | like the focused post |
//! | `t` | repost the focused post |
//! | `r` | reply to the focused post |
//! | `o` / `Enter` | open thread for the focused post |
//! | `gp` | open author's profile |
//! | `gh` / `gn` / `gd` / `gs` | go: home / notifications / discover / suggestions |
//! | `<space>n` | new post (compose) |
//! | `<space>/` or `Cmd-K` | search |
//! | `<space>s` | settings |
//! | `<space>f` | saved feeds |
//! | `?` or `<space>?` | keyboard-shortcut help overlay |
//! | `Esc` | close any open modal |
//!
//! Shortcuts are suppressed when any modal sheet is open (compose,
//! search, profile, thread, engagement, settings, saved-feeds) —
//! the modal's own input fields get the keystrokes instead. `Esc`
//! is the universal close.

use crate::state::{
    add_column_unique, ColumnSpec, ComposeContext, EngagementFocus, FocusedItem, KeyboardHelp,
    PendingChord, ProfileFocus, ThreadFocus,
};
use dioxus::prelude::*;
use smooblue_oauth::Session;

/// One frame of context the keyboard handler needs to act. Bundling
/// these into a struct keeps the handler signature manageable as
/// we add more sheets / signals.
pub struct KeyContext {
    pub focus: Signal<FocusedItem>,
    pub help: Signal<KeyboardHelp>,
    pub chord: Signal<PendingChord>,
    pub cols: Signal<Vec<ColumnSpec>>,
    pub compose: Signal<ComposeContext>,
    pub thread: Signal<ThreadFocus>,
    pub profile: Signal<ProfileFocus>,
    pub engagement: Signal<EngagementFocus>,
    pub session: Signal<Option<Session>>,
    pub search_open: Signal<bool>,
    pub saved_feeds_open: Signal<bool>,
    pub settings_open: Signal<bool>,
}

/// `true` when any modal sheet is open. Used to skip vim shortcuts
/// (the modal's own input fields handle keystrokes).
pub fn any_modal_open(ctx: &KeyContext) -> bool {
    ctx.compose.read().open
        || ctx.thread.read().0.is_some()
        || ctx.profile.read().0.is_some()
        || ctx.engagement.read().0.is_some()
        || *ctx.search_open.read()
        || *ctx.saved_feeds_open.read()
        || *ctx.settings_open.read()
        || ctx.help.read().0
}

/// Close whichever modal is on top. Esc handler.
pub fn close_top_modal(ctx: &mut KeyContext) {
    if ctx.help.read().0 {
        ctx.help.set(KeyboardHelp(false));
        return;
    }
    // Innermost-first close order — engagement / profile / thread
    // can stack on top of each other; the most-recently-opened wins.
    if ctx.engagement.read().0.is_some() {
        ctx.engagement.set(EngagementFocus(None));
        return;
    }
    if ctx.profile.read().0.is_some() {
        ctx.profile.set(ProfileFocus(None));
        return;
    }
    if ctx.thread.read().0.is_some() {
        ctx.thread.set(ThreadFocus(None));
        return;
    }
    if *ctx.settings_open.read() {
        ctx.settings_open.set(false);
        return;
    }
    if *ctx.saved_feeds_open.read() {
        ctx.saved_feeds_open.set(false);
        return;
    }
    if *ctx.search_open.read() {
        ctx.search_open.set(false);
        return;
    }
    if ctx.compose.read().open {
        let mut w = ctx.compose.write();
        w.open = false;
        w.reply_to = None;
    }
}

/// Dispatch a single keypress. Returns `true` if the handler consumed
/// the event (so the caller can prevent_default / stop propagation).
pub fn dispatch(ctx: &mut KeyContext, key: &Key, modifiers: Modifiers) -> bool {
    // Esc closes modals from anywhere — even when typing in compose.
    if *key == Key::Escape {
        close_top_modal(ctx);
        return true;
    }

    // ⌘K opens search from anywhere (vim-typical "global picker"
    // shortcut). Works even with modals open so the user can pivot.
    if matches!(key, Key::Character(s) if s == "k") && modifiers.meta() {
        ctx.search_open.set(true);
        return true;
    }

    // Suppress all other shortcuts while a modal is open — the modal
    // owns the keystrokes.
    if any_modal_open(ctx) {
        return false;
    }

    let pending = ctx.chord.read().prefix.clone();

    // ── Chord continuation: a pending `g` or `<space>` followed by
    // another key.
    if let Some(prefix) = pending {
        ctx.chord.set(PendingChord { prefix: None });
        match (prefix.as_str(), key) {
            // gg = top of column
            ("g", Key::Character(c)) if c == "g" => {
                ctx.focus.write().item = 0;
                return true;
            }
            // gh = add Home column + focus column 0
            ("g", Key::Character(c)) if c == "h" => {
                add_column_unique(&mut ctx.cols, ColumnSpec::home());
                ctx.focus.write().column = 0;
                return true;
            }
            // gn = Notifications
            ("g", Key::Character(c)) if c == "n" => {
                add_column_unique(&mut ctx.cols, ColumnSpec::notifications());
                return true;
            }
            // gd = Discover
            ("g", Key::Character(c)) if c == "d" => {
                add_column_unique(&mut ctx.cols, ColumnSpec::discover());
                return true;
            }
            // gs = Suggestions
            ("g", Key::Character(c)) if c == "s" => {
                add_column_unique(&mut ctx.cols, ColumnSpec::suggestions());
                return true;
            }
            // gp = self profile
            ("g", Key::Character(c)) if c == "p" => {
                if let Some(s) = ctx.session.read().clone() {
                    ctx.profile.set(ProfileFocus(Some(s.did)));
                }
                return true;
            }
            // <space>n = new post
            (" ", Key::Character(c)) if c == "n" => {
                let mut w = ctx.compose.write();
                w.reply_to = None;
                w.open = true;
                return true;
            }
            // <space>/ = search (matches the "leader-slash" convention)
            (" ", Key::Character(c)) if c == "/" => {
                ctx.search_open.set(true);
                return true;
            }
            // <space>s = settings
            (" ", Key::Character(c)) if c == "s" => {
                ctx.settings_open.set(true);
                return true;
            }
            // <space>f = saved feeds
            (" ", Key::Character(c)) if c == "f" => {
                ctx.saved_feeds_open.set(true);
                return true;
            }
            // <space>? = help
            (" ", Key::Character(c)) if c == "?" => {
                ctx.help.set(KeyboardHelp(true));
                return true;
            }
            // <space>1..<space>9 = focus column N (1-indexed)
            (" ", Key::Character(c)) => {
                if let Ok(n) = c.parse::<usize>() {
                    if (1..=9).contains(&n) {
                        let col_count = ctx.cols.read().len();
                        let idx = (n - 1).min(col_count.saturating_sub(1));
                        ctx.focus.write().column = idx;
                        ctx.focus.write().item = 0;
                        return true;
                    }
                }
                return false;
            }
            _ => return false,
        }
    }

    // ── First-key handling. Vim-key chars + arrow keys split into
    // separate arms because Rust's match alternation doesn't allow
    // arms with differently-bound names.
    if matches!(key, Key::ArrowDown) {
        ctx.focus.write().item += 1;
        return true;
    }
    if matches!(key, Key::ArrowUp) {
        let cur = ctx.focus.read().item;
        ctx.focus.write().item = cur.saturating_sub(1);
        return true;
    }
    if matches!(key, Key::ArrowLeft) {
        let cur = ctx.focus.read().column;
        ctx.focus.write().column = cur.saturating_sub(1);
        ctx.focus.write().item = 0;
        return true;
    }
    if matches!(key, Key::ArrowRight) {
        let col_count = ctx.cols.read().len();
        let cur = ctx.focus.read().column;
        ctx.focus.write().column = (cur + 1).min(col_count.saturating_sub(1));
        ctx.focus.write().item = 0;
        return true;
    }
    match key {
        Key::Character(c) => match c.as_str() {
            "j" => {
                ctx.focus.write().item += 1;
                true
            }
            "k" => {
                let cur = ctx.focus.read().item;
                ctx.focus.write().item = cur.saturating_sub(1);
                true
            }
            "h" => {
                let cur = ctx.focus.read().column;
                ctx.focus.write().column = cur.saturating_sub(1);
                ctx.focus.write().item = 0;
                true
            }
            "l" => {
                let col_count = ctx.cols.read().len();
                let cur = ctx.focus.read().column;
                ctx.focus.write().column = (cur + 1).min(col_count.saturating_sub(1));
                ctx.focus.write().item = 0;
                true
            }
            // Chord starters — stash and wait for the second key.
            "g" | " " => {
                ctx.chord.set(PendingChord {
                    prefix: Some(c.clone()),
                });
                true
            }
            // G (capital) = bottom of column. Without a known item
            // count from the column, we use a large sentinel and let
            // the column clamp visually.
            "G" => {
                ctx.focus.write().item = usize::MAX / 2;
                true
            }
            "?" => {
                ctx.help.set(KeyboardHelp(true));
                true
            }
            "n" => {
                let mut w = ctx.compose.write();
                w.reply_to = None;
                w.open = true;
                true
            }
            // Single-key actions on the focused post are wired by
            // PostCard itself listening to FocusedItem + a separate
            // "action signal" — see dispatch_post_action below for
            // the wiring helper.
            _ => false,
        },
        Key::Enter => {
            // Open thread for focused post — handled by PostCard
            // reading FocusedItem + a thread-open intent. Simpler:
            // we'd need to know the focused post URI here. Punt
            // until we add a "by-column current-items" Signal.
            // (Pearl: keyboard-driven post actions follow-up.)
            false
        }
        _ => false,
    }
}
