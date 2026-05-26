//! Pre-flight session-refresh helper used by every fetch site.
//!
//! Before any XRPC call, callers run [`fresh_client`]. If the access
//! token has more than ~30s left, this is a fast read from the
//! Signal. If it's expiring, we transparently swap in a fresh token
//! via the refresh-token flow (DPoP-signed POST to the token
//! endpoint) and persist the rotated session.
//!
//! ## Single-flight
//!
//! Multiple columns poll concurrently. Without a guard they all
//! call `refresh_session` in parallel with the SAME refresh token
//! when it's expiring. ATproto refresh tokens **rotate** — only one
//! call wins and gets the new token; the others fail with
//! `invalid_grant`, which we used to interpret as "refresh token
//! revoked, sign the user out." Result: the user got bounced to
//! login every ~2h.
//!
//! [`REFRESH_LOCK`] is a global `tokio::sync::Mutex` — the first
//! caller actually hits the network; concurrent callers await the
//! lock, then re-check expiry (it's now fresh) and skip the network
//! call entirely.
//!
//! ## Sign-out semantics
//!
//! We only flip session → None on `invalid_grant` from the server
//! when we DID hold the lock (so we're certain we used the canonical
//! refresh token, not a stale one). Transient errors (network /
//! 5xx) never sign the user out; the next call retries.
//!
//! Demo mode bypasses refresh entirely.

use crate::persistence;
use dioxus::prelude::{Readable, Signal, Writable};
use smooblue_atproto::AtClient;
use smooblue_oauth::{OAuthClientConfig, Session};
use std::sync::OnceLock;
use tokio::sync::Mutex;
use url::Url;

/// Process-global lock so only one refresh hits the network at a
/// time. tokio::Mutex (not std::Mutex) so concurrent awaiters yield
/// instead of blocking the runtime worker.
fn refresh_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

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
    // Serialize with the global refresh lock. Concurrent callers
    // await here; only the first one actually hits the network.
    let _guard = refresh_lock().lock().await;
    // Re-check after acquiring the lock — another task may have
    // refreshed while we were waiting. Reading the signal here
    // picks up the freshly-installed session.
    let session = session_sig.read().clone()?;
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
            // Best-effort persist. We update the in-memory signal
            // either way so the current session keeps working until
            // exit — if the write fails the user just has to re-auth
            // on next launch instead of immediately.
            //
            // Write BOTH the legacy single slot AND the multi-account
            // keyed-by-DID slot. The boot path prefers the keyed slot
            // (state.rs), so skipping it leaves an old refresh token
            // there that fails with invalid_grant on next launch.
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
            // Refresh failed. We're holding the global refresh lock,
            // so this isn't a "lost a race against another refresh"
            // false positive — if the server said invalid_grant, the
            // refresh token really is dead and the user has to re-auth.
            //
            // Even so, leave the in-memory session untouched and let
            // the next fetch attempt retry; the only thing we drop is
            // the access token's effective freshness. Sign out only
            // on confirmed invalid_grant.
            let msg = e.to_string();
            if msg.contains("invalid_grant") || msg.contains("re-auth") {
                tracing::warn!(error = %msg, "smooblue: refresh rejected (invalid_grant) — signing out");
                let _ = persistence::clear_session();
                session_sig.set(None);
            } else {
                tracing::warn!(error = %msg, "smooblue: refresh failed (transient) — will retry");
            }
            None
        }
    }
}
