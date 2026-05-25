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
    let sign_out = move |_| {
        let _ = persistence::clear_session();
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
                            "Light theme · keyboard shortcuts · multi-account switching · "
                            "mute / block lists · self-update notifier · DMs · "
                            "rich-text mentions & link facets in compose."
                        }
                    }
                }
            }
        }
    }
}
