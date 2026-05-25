//! Settings modal — sign-out, version info, config-dir reveal, and
//! placeholders for things on the polish roadmap (theme, keyboard
//! shortcuts, mute lists, multi-account).
//!
//! Kept deliberately spartan — the goal is to surface the things a
//! user actually needs (sign out, what version am I running, where
//! does my data live) without committing to a full preferences
//! framework before we know what users will ask for.

use crate::icons;
use crate::persistence;
use crate::state::ThemeMode;
use dioxus::prelude::*;
use smooblue_oauth::Session;

#[component]
pub fn SettingsSheet(open: Signal<bool>) -> Element {
    let mut session = use_context::<Signal<Option<Session>>>();
    if !*open.read() {
        return rsx! { Fragment {} };
    }

    let mut open_close = open;
    let close = move |_| {
        open_close.set(false);
    };

    let mut open_signout = open;
    let mut accounts_for_signout = use_context::<Signal<persistence::Accounts>>();
    let sign_out = move |_| {
        // Pull the active DID *before* we wipe the legacy slot so
        // we can also drop the keyed keyring entry for that account.
        let active = accounts_for_signout.read().active_did.clone();
        let _ = persistence::clear_session();
        if let Some(did) = active {
            let _ = persistence::delete_session_for(&did);
            let mut idx = accounts_for_signout.write();
            idx.accounts.retain(|a| a.did != did);
            idx.active_did = idx.accounts.first().map(|a| a.did.clone());
            let _ = persistence::save_accounts(&idx);
        }
        session.set(None);
        open_signout.set(false);
    };

    let reveal_config_dir = move |_| {
        if let Some(dir) = directories::ProjectDirs::from("ai", "Smoo", "smooblue") {
            let path = dir.config_dir().to_path_buf();
            // Spawn is fine — open is fire-and-forget; the child handle
            // drops immediately.
            let _ = std::process::Command::new("open").arg(&path).spawn();
        }
    };

    let version = env!("CARGO_PKG_VERSION");
    let current_handle = session
        .read()
        .as_ref()
        .map(|s| s.handle.clone())
        .unwrap_or_default();

    rsx! {
        div { class: "modal__backdrop", onclick: close,
            div { class: "modal__sheet settings__sheet",
                onclick: move |e| e.stop_propagation(),
                div { class: "settings__head",
                    span { class: "settings__title", "Settings" }
                    button { class: "settings__close", title: "Close (Esc)",
                        onclick: close,
                        icons::X { size: icons::Size::Sm }
                    }
                }
                div { class: "settings__body",
                    // ── Account ─────────────────────────────────────
                    section { class: "settings__section",
                        h3 { class: "settings__section-title", "Account" }
                        if !current_handle.is_empty() {
                            div { class: "settings__row",
                                span { class: "settings__row-label", "Signed in as" }
                                span { class: "settings__row-value", "@{current_handle}" }
                            }
                        }
                        button { class: "settings__action settings__action--danger",
                            onclick: sign_out,
                            icons::LogOut { size: icons::Size::Sm }
                            "Sign out"
                        }
                    }

                    // ── Appearance ─────────────────────────────────
                    section { class: "settings__section",
                        h3 { class: "settings__section-title", "Appearance" }
                        ThemePicker {}
                    }

                    // ── Multi-account switcher ─────────────────────
                    section { class: "settings__section",
                        h3 { class: "settings__section-title", "Accounts" }
                        AccountSwitcher {}
                    }

                    // ── Moderation: mutes + blocks ─────────────────
                    section { class: "settings__section",
                        h3 { class: "settings__section-title", "Mute & block lists" }
                        ModerationLists {}
                    }

                    // ── About ──────────────────────────────────────
                    section { class: "settings__section",
                        h3 { class: "settings__section-title", "About" }
                        div { class: "settings__row",
                            span { class: "settings__row-label", "Version" }
                            span { class: "settings__row-value", "{version}" }
                        }
                        div { class: "settings__row",
                            span { class: "settings__row-label", "Source" }
                            a { class: "settings__row-link",
                                href: "https://github.com/SmooAI/smooblue",
                                "github.com/SmooAI/smooblue"
                            }
                        }
                        button { class: "settings__action",
                            onclick: reveal_config_dir,
                            "Reveal config folder in Finder"
                        }
                    }

                    // ── Roadmap placeholders ───────────────────────
                    section { class: "settings__section",
                        h3 { class: "settings__section-title", "Coming soon" }
                        p { class: "settings__roadmap",
                            "Multi-account switching · DM inbox · profile editing · "
                            "thread compose · pinned post · video upload."
                        }
                    }
                }
            }
        }
    }
}

/// Account switcher — lists every signed-in account and lets the
/// user flip the active one. Switching loads the keyed session
/// out of keyring and swaps it into the active Session signal,
/// which the rest of the app reads. "+ Add" signs out the current
/// session so the LoginView appears; the new sign-in will append
/// to the index.
#[component]
fn AccountSwitcher() -> Element {
    let mut session = use_context::<Signal<Option<Session>>>();
    let mut accounts_sig = use_context::<Signal<persistence::Accounts>>();
    let snapshot = accounts_sig.read().clone();
    let active_did = snapshot.active_did.clone();

    // Switch to a different already-signed-in account.
    let switch_to = move |did: String| {
        let Some(s) = persistence::load_session_for(&did) else {
            return;
        };
        // Also update the legacy single-slot keyring so anything
        // (other tools, debug helpers) reading the old name sees
        // the right session.
        let _ = persistence::save_session(&s);
        let mut idx = accounts_sig.write();
        idx.active_did = Some(did);
        let _ = persistence::save_accounts(&idx);
        session.set(Some(s));
    };

    // Remove an account from the index (and its keyring entry).
    // If we just deleted the active one, sign out — the user will
    // land on LoginView and can pick another from there.
    let remove = move |did: String| {
        let _ = persistence::delete_session_for(&did);
        let mut idx = accounts_sig.write();
        idx.accounts.retain(|a| a.did != did);
        if idx.active_did.as_deref() == Some(did.as_str()) {
            idx.active_did = idx.accounts.first().map(|a| a.did.clone());
            let _ = persistence::save_accounts(&idx);
            drop(idx);
            // Promote the next account if any.
            let next = accounts_sig.read().active_did.clone();
            match next {
                Some(d) => {
                    if let Some(s) = persistence::load_session_for(&d) {
                        let _ = persistence::save_session(&s);
                        session.set(Some(s));
                    } else {
                        let _ = persistence::clear_session();
                        session.set(None);
                    }
                }
                None => {
                    let _ = persistence::clear_session();
                    session.set(None);
                }
            }
        } else {
            let _ = persistence::save_accounts(&idx);
        }
    };

    // Add: sign out, which routes back to LoginView. After the new
    // OAuth completes, that flow appends the new account into the
    // index via [`save_session_for`] + [`save_accounts`].
    let add_account = move |_| {
        // Don't drop existing accounts — just clear the active
        // session so the LoginView paints. The keyed entries
        // persist in keyring; the next switch_to can re-activate.
        let _ = persistence::clear_session();
        let mut idx = accounts_sig.write();
        idx.active_did = None;
        let _ = persistence::save_accounts(&idx);
        session.set(None);
    };

    rsx! {
        if snapshot.accounts.is_empty() {
            p { class: "moderation__empty", "Only one account signed in." }
        } else {
            for acc in snapshot.accounts.iter().cloned() {
                {
                    let is_active = active_did.as_deref() == Some(acc.did.as_str());
                    let did_for_switch = acc.did.clone();
                    let did_for_remove = acc.did.clone();
                    let mut switch_to = switch_to;
                    let mut remove = remove;
                    rsx! {
                        div { class: "account-row",
                            key: "{acc.did}",
                            div { class: "account-row__id",
                                span { class: "account-row__handle", "@{acc.handle}" }
                                span { class: "account-row__did", "{acc.did}" }
                            }
                            if is_active {
                                span { class: "account-row__active-badge", "Active" }
                            } else {
                                button { class: "btn btn--ghost account-row__switch",
                                    onclick: move |_| switch_to(did_for_switch.clone()),
                                    "Switch"
                                }
                            }
                            button { class: "account-row__remove",
                                title: "Remove account from this device",
                                onclick: move |_| remove(did_for_remove.clone()),
                                icons::X { size: icons::Size::Sm }
                            }
                        }
                    }
                }
            }
        }
        button { class: "btn btn--ghost",
            onclick: add_account,
            "+ Add another account"
        }
    }
}

/// Mute / block management inside Settings. Loads
/// `app.bsky.graph.getMutes` + `getBlocks` lazily (only when this
/// section paints) so opening Settings stays snappy when the user
/// just wants to flip the theme. Each row has an Unmute / Unblock
/// button that calls the inverse XRPC procedure and optimistically
/// removes the row.
#[component]
fn ModerationLists() -> Element {
    use crate::auth_refresh::fresh_client;

    let session = use_context::<Signal<Option<Session>>>();

    let mutes = use_resource(move || {
        let session_sig = session;
        async move {
            let client = fresh_client(session_sig).await.ok_or("not signed in")?;
            client.get_mutes().await.map_err(|e| e.to_string())
        }
    });
    let blocks = use_resource(move || {
        let session_sig = session;
        async move {
            let client = fresh_client(session_sig).await.ok_or("not signed in")?;
            client.get_blocks().await.map_err(|e| e.to_string())
        }
    });

    let mut muted_list = use_signal(Vec::<smooblue_atproto::feed::ActorProfile>::new);
    let mut blocked_list = use_signal(Vec::<smooblue_atproto::feed::ActorProfile>::new);

    use_effect(move || {
        if let Some(Ok(r)) = &*mutes.read_unchecked() {
            muted_list.set(r.mutes.clone());
        }
    });
    use_effect(move || {
        if let Some(Ok(r)) = &*blocks.read_unchecked() {
            blocked_list.set(r.blocks.clone());
        }
    });

    rsx! {
        // Muted
        div { class: "moderation__group",
            h4 { class: "moderation__group-title", "Muted ({muted_list.read().len()})" }
            if muted_list.read().is_empty() {
                p { class: "moderation__empty", "No muted accounts." }
            } else {
                for actor in muted_list.read().clone().into_iter() {
                    ModerationRow {
                        key: "m-{actor.did}",
                        actor: actor.clone(),
                        kind: ModerationKind::Mute,
                        on_remove: move |did: String| {
                            muted_list.write().retain(|a| a.did != did);
                        },
                    }
                }
            }
        }
        // Blocked
        div { class: "moderation__group",
            h4 { class: "moderation__group-title", "Blocked ({blocked_list.read().len()})" }
            if blocked_list.read().is_empty() {
                p { class: "moderation__empty", "No blocked accounts." }
            } else {
                for actor in blocked_list.read().clone().into_iter() {
                    ModerationRow {
                        key: "b-{actor.did}",
                        actor: actor.clone(),
                        kind: ModerationKind::Block,
                        on_remove: move |did: String| {
                            blocked_list.write().retain(|a| a.did != did);
                        },
                    }
                }
            }
        }
    }
}

/// Which side of the mute/block divide a row is on. Determines the
/// undo XRPC call (unmuteActor vs deleteRecord of the block).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModerationKind {
    Mute,
    Block,
}

#[component]
fn ModerationRow(
    actor: smooblue_atproto::feed::ActorProfile,
    kind: ModerationKind,
    on_remove: EventHandler<String>,
) -> Element {
    use crate::auth_refresh::fresh_client;
    let session = use_context::<Signal<Option<Session>>>();
    let mut pending = use_signal(|| false);

    let did = actor.did.clone();
    let handle = actor.handle.clone();
    let name = actor.display_name.clone().unwrap_or_else(|| handle.clone());
    let avatar = actor.avatar.clone();
    let block_uri = actor.viewer.as_ref().and_then(|v| v.blocking.clone());

    let remove = move |_| {
        if *pending.read() {
            return;
        }
        pending.set(true);
        let did_clone = did.clone();
        let block_uri_clone = block_uri.clone();
        spawn(async move {
            let Some(client) = fresh_client(session).await else {
                pending.set(false);
                return;
            };
            let result = match kind {
                ModerationKind::Mute => client.unmute_actor(&did_clone).await,
                ModerationKind::Block => match block_uri_clone {
                    Some(uri) => client.delete_record(&uri).await,
                    None => Ok(()), // No block record to delete — already removed.
                },
            };
            pending.set(false);
            if result.is_ok() {
                on_remove.call(did_clone);
            }
        });
    };

    let action_label = match kind {
        ModerationKind::Mute => "Unmute",
        ModerationKind::Block => "Unblock",
    };

    rsx! {
        div { class: "moderation__row",
            if let Some(url) = &avatar {
                img { class: "moderation__avatar", src: "{url}",
                    loading: "lazy", decoding: "async" }
            } else {
                div { class: "moderation__avatar moderation__avatar--blank" }
            }
            div { class: "moderation__id",
                span { class: "moderation__name", "{name}" }
                span { class: "moderation__handle", "@{handle}" }
            }
            button { class: "btn btn--ghost moderation__remove",
                disabled: *pending.read(),
                onclick: remove,
                if *pending.read() { "…" } else { "{action_label}" }
            }
        }
    }
}

/// Two-pill theme switcher. Writes through to disk so the choice
/// persists across launches. Lives inline here because no other
/// component needs to render a theme picker.
#[component]
fn ThemePicker() -> Element {
    let mut theme = use_context::<Signal<ThemeMode>>();
    let current = *theme.read();

    let set_dark = move |_| {
        if current != ThemeMode::Dark {
            theme.set(ThemeMode::Dark);
            let _ = persistence::save_theme("dark");
        }
    };
    let set_light = move |_| {
        if current != ThemeMode::Light {
            theme.set(ThemeMode::Light);
            let _ = persistence::save_theme("light");
        }
    };

    rsx! {
        div { class: "theme-picker",
            button {
                class: if current == ThemeMode::Dark {
                    "theme-picker__opt theme-picker__opt--selected"
                } else {
                    "theme-picker__opt"
                },
                onclick: set_dark,
                "Dark"
            }
            button {
                class: if current == ThemeMode::Light {
                    "theme-picker__opt theme-picker__opt--selected"
                } else {
                    "theme-picker__opt"
                },
                onclick: set_light,
                "Light"
            }
        }
    }
}
