//! Pre-flight session-refresh helper used by every fetch site.
//!
//! Before any XRPC call, callers run [`fresh_client`]. If the access
//! token has more than ~30s left, this is a fast read from the
//! Signal. If it's expiring, we transparently swap in a fresh token
//! via the refresh-token flow (DPoP-signed POST to the token
//! endpoint) and persist the rotated session to the OS keyring.
//!
//! Failure modes:
//! - Network down / token endpoint 5xx → returns `None`, leaves the
//!   session as-is. Next call retries.
//! - `invalid_grant` (refresh token expired / revoked) → clears the
//!   persisted session and signals out (`Signal::set(None)`), which
//!   bounces the user back to the login view.
//!
//! Sessions in demo mode skip refresh entirely — they're synthetic
//! and never expire as far as the AppView is concerned.

use crate::persistence;
use dioxus::prelude::{Readable, Signal, Writable};
use smooblue_atproto::AtClient;
use smooblue_oauth::{OAuthClientConfig, Session};
use url::Url;

/// Return a ready-to-use [`AtClient`] for the current session,
/// refreshing the access token first if it's expired or expiring.
/// Returns `None` if the user isn't signed in or a refresh failed
/// hard enough to require re-auth.
pub async fn fresh_client(session_sig: Signal<Option<Session>>) -> Option<AtClient> {
    let session = ensure_fresh_session(session_sig).await?;
    let base = Url::parse(&session.pds).ok()?;
    Some(AtClient::new(session, base))
}

/// Lower-level: return the (possibly refreshed) Session without
/// wrapping in an AtClient. Used by the OAuth-driven sign-in flow
/// where the caller needs the bare session.
pub async fn ensure_fresh_session(session_sig: Signal<Option<Session>>) -> Option<Session> {
    let session = session_sig.read().clone()?;
    if crate::demo::is_active() {
        // Demo mode bypasses the network entirely; the synthetic
        // session is happy for 24h and never needs refreshing.
        return Some(session);
    }
    if !session.is_expired() {
        return Some(session);
    }
    refresh_and_persist(session, session_sig).await
}

async fn refresh_and_persist(
    session: Session,
    mut session_sig: Signal<Option<Session>>,
) -> Option<Session> {
    let http = reqwest::Client::new();
    let cfg = OAuthClientConfig::default_public();
    match smooblue_oauth::refresh_session(&http, &session, &cfg.client_id).await {
        Ok(new_session) => {
            // Best-effort persist; if the keyring write fails (e.g.,
            // user revoked permission) we still update the in-memory
            // signal so the current session keeps working until exit.
            //
            // Write BOTH the legacy single slot AND the multi-account
            // keyed-by-DID slot. The boot path prefers the keyed slot
            // (state.rs), so skipping it leaves an old refresh token
            // there that fails with invalid_grant on next launch and
            // forces the user to sign in again every restart.
            if let Err(e) = persistence::save_session(&new_session) {
                tracing::warn!(error = %e, "smooblue: failed to persist refreshed session (legacy slot)");
            }
            if let Err(e) = persistence::save_session_for(&new_session.did, &new_session) {
                tracing::warn!(error = %e, "smooblue: failed to persist refreshed session (keyed slot)");
            }
            session_sig.set(Some(new_session.clone()));
            Some(new_session)
        }
        Err(e) => {
            // Refresh failed. If it's an invalid_grant we MUST drop
            // the session — the refresh token is dead. Other errors
            // (transient network, 5xx) shouldn't boot the user, but
            // we can't distinguish without parsing the inner error
            // string. Conservative: only sign-out on invalid_grant.
            let msg = e.to_string();
            if msg.contains("invalid_grant") || msg.contains("re-auth") {
                tracing::warn!(error = %msg, "smooblue: refresh rejected, signing out");
                let _ = persistence::clear_session();
                session_sig.set(None);
            } else {
                tracing::warn!(error = %msg, "smooblue: refresh failed (transient)");
            }
            None
        }
    }
}
