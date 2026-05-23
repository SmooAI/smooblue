//! Handle-entry login view. Captures the user's handle and kicks off the
//! OAuth dance. The browser is opened via the system's default opener;
//! the loopback callback runs inside the OAuth crate.

use dioxus::prelude::*;
use smooblue_oauth::{OAuthClient, OAuthClientConfig, Session};

#[component]
pub fn LoginView() -> Element {
    let mut handle = use_signal(String::new);
    let mut status = use_signal(|| Status::Idle);
    let session = use_context::<Signal<Option<Session>>>();

    let start_signin = move |_evt: MouseEvent| {
        let entered = handle.read().trim().to_string();
        if entered.is_empty() {
            status.set(Status::Error(
                "Enter your handle (e.g., alice.bsky.social)".into(),
            ));
            return;
        }
        status.set(Status::Pending);
        let mut status_sig = status;
        let mut session_sig = session;
        spawn(async move {
            let client = OAuthClient::new(OAuthClientConfig::default_public());
            // `open_url` shells out to the OS default opener.
            let res = client.sign_in(&entered, open_default).await;
            match res {
                Ok(mut s) => {
                    s.handle = entered;
                    if let Err(e) = crate::persistence::save_session(&s) {
                        status_sig.set(Status::Error(format!(
                            "Signed in but couldn't persist session: {e}"
                        )));
                    } else {
                        status_sig.set(Status::Idle);
                    }
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
                div { class: "login__logo",
                    svg {
                        width: "44",
                        height: "44",
                        view_box: "0 0 135 135",
                        fill: "white",
                        path { d: "M45.63,15.38c-12.39,5.21-22.54,14.64-28.65,26.61-6.12,11.97-7.8,25.72-4.77,38.81,3.04,13.09,10.6,24.69,21.36,32.75,10.76,8.06,24.02,12.05,37.44,11.28,13.42-.77,26.13-6.26,35.9-15.5,9.76-9.24,15.95-21.63,17.46-34.99,1.51-13.36-1.74-26.82-9.19-38.01-1.07-1.61-.64-3.78.97-4.85,1.61-1.07,3.78-.64,4.85.97,8.36,12.56,12.02,27.68,10.32,42.67-1.7,15-8.64,28.91-19.61,39.28-10.96,10.37-25.24,16.54-40.31,17.4-15.07.87-29.96-3.62-42.04-12.66-12.08-9.05-20.58-22.07-23.99-36.77-3.41-14.7-1.51-30.14,5.35-43.58,6.87-13.44,18.26-24.02,32.17-29.87,13.91-5.85,29.44-6.6,43.85-2.11,1.85.57,2.88,2.54,2.3,4.38-.57,1.85-2.54,2.88-4.38,2.3-12.83-4-26.67-3.33-39.06,1.88Z" }
                    }
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
                button {
                    class: "login__btn",
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
