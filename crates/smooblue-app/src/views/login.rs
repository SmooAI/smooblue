//! Handle-entry login view. Captures the user's handle and kicks off the
//! OAuth dance. The browser is opened via the system's default opener;
//! the loopback callback runs inside the OAuth crate.
//!
//! Includes an opt-in checkbox for syncing the user's Bluesky profile to
//! the Smoo AI CRM. Default OFF. Only triggers after sign-in succeeds
//! AND consent was given — the [`smooblue_crm::Consent`] token makes
//! "fire without consent" a compile error.

use dioxus::prelude::*;
use smooblue_atproto::AtClient;
use smooblue_crm::{BlueskyProfile, Consent, CrmClient};
use smooblue_oauth::{OAuthClient, OAuthClientConfig, Session};
use url::Url;

#[component]
pub fn LoginView() -> Element {
    let mut handle = use_signal(String::new);
    let mut status = use_signal(|| Status::Idle);
    let mut share_with_smoo = use_signal(|| false);
    let session = use_context::<Signal<Option<Session>>>();

    let start_signin = move |_evt: MouseEvent| {
        let entered = handle.read().trim().to_string();
        if entered.is_empty() {
            status.set(Status::Error(
                "Enter your handle (e.g., alice.bsky.social)".into(),
            ));
            return;
        }
        let consented = *share_with_smoo.read();
        status.set(Status::Pending);
        let mut status_sig = status;
        let mut session_sig = session;
        spawn(async move {
            let client = OAuthClient::new(OAuthClientConfig::default_public());
            let res = client.sign_in(&entered, open_default).await;
            match res {
                Ok(mut s) => {
                    s.handle = entered.clone();
                    if let Err(e) = crate::persistence::save_session(&s) {
                        status_sig.set(Status::Error(format!(
                            "Signed in but couldn't persist session: {e}"
                        )));
                        return;
                    }
                    // Opt-in CRM sync. Non-blocking — if it fails, the user is
                    // still signed in; we just log + show a soft toast.
                    if consented {
                        if let Err(e) = sync_to_smoo_crm(&s).await {
                            tracing::warn!(error = %e, "smooblue: CRM sync failed (non-blocking)");
                        }
                    }
                    status_sig.set(Status::Idle);
                    session_sig.set(Some(s));
                }
                Err(e) => {
                    status_sig.set(Status::Error(format!("{e}")));
                }
            }
        });
    };

    rsx! {
        div { class: "login",
            div { class: "login__card",
                div { class: "brand-badge login__logo",
                    dangerous_inner_html: "{smooblue_theme::MONOGRAM_SVG}",
                }
                h1 { class: "login__title", "Smooblue" }
                p { class: "login__sub", "Sign in with your Bluesky handle" }
                input {
                    class: "login__input",
                    placeholder: "alice.bsky.social",
                    autofocus: true,
                    value: "{handle}",
                    oninput: move |e| handle.set(e.value()),
                }
                label { class: "login__consent",
                    input {
                        r#type: "checkbox",
                        checked: *share_with_smoo.read(),
                        oninput: move |e| share_with_smoo.set(e.value() == "true"),
                    }
                    span { class: "login__consent-text",
                        "Stay in touch with Smoo AI — send my public Bluesky profile (handle, display name, avatar, bio, follower counts) to "
                        a { href: "https://smoo.ai", "smoo.ai" }
                        ". No password or auth tokens are ever shared. You can opt out anytime in settings."
                    }
                }
                button {
                    class: "btn btn--primary btn--lg login__btn",
                    disabled: matches!(*status.read(), Status::Pending),
                    onclick: start_signin,
                    if matches!(*status.read(), Status::Pending) { "Opening browser…" } else { "Continue with Bluesky" }
                }
                match &*status.read() {
                    Status::Error(msg) => rsx! { div { class: "login__error", "{msg}" } },
                    Status::Pending => rsx! { div { class: "login__pending", "Complete the authorization in your browser, then return here." } },
                    Status::Idle => rsx! {},
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Idle,
    Pending,
    Error(String),
}

/// Fetch the user's Bluesky profile via the now-authenticated session and
/// forward it to the Smoo AI CRM intake endpoint. Called only when the
/// user explicitly opted in on the login screen.
async fn sync_to_smoo_crm(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    let appview = Url::parse("https://api.bsky.app")?;
    let at = AtClient::new(session.clone(), appview);
    let profile = at.get_profile(&session.did).await?;
    let crm = CrmClient::smoo_default();
    crm.report_signup(
        Consent::granted(),
        &BlueskyProfile {
            did: profile.did,
            handle: profile.handle,
            display_name: profile.display_name,
            description: profile.description,
            avatar: profile.avatar,
            followers_count: profile.followers_count,
            follows_count: profile.follows_count,
        },
    )
    .await?;
    Ok(())
}

/// Open `url` in the user's default browser.
///
/// We don't depend on the `open` crate to keep dependency count low — `macOS`
/// uses `open`, Linux uses `xdg-open`, Windows uses `cmd /c start`.
fn open_default(url: &str) -> Result<(), smooblue_oauth::OAuthError> {
    use std::process::Command;
    let cmd = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    };
    match cmd {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(smooblue_oauth::OAuthError::BrowserOpen(format!("exit {s}"))),
        Err(e) => Err(smooblue_oauth::OAuthError::BrowserOpen(e.to_string())),
    }
}
