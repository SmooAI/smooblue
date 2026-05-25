# OAuth + Session

#architecture

ATproto's OAuth flow is PAR + PKCE + DPoP, none of which a "just use the bsky SDK" answer covers. This page is the load-bearing one for understanding why `smooblue-oauth` is its own crate.

---

## The flow

1. **PDS discovery**. User types `alice.bsky.social`. We do `com.atproto.identity.resolveHandle` against bsky.social, get a DID, then fetch the DID document to learn the user's PDS URL.
2. **PAR** (Pushed Authorization Request). We POST our authorization request — `client_id` (our hosted client metadata URL), `redirect_uri` (loopback), `code_challenge` (PKCE S256), `dpop_jkt` (the thumbprint of our DPoP key) — to the AS's `pushed_authorization_request_endpoint`. We get back a `request_uri`.
3. **Browser launch**. We start an ephemeral loopback HTTP server on `127.0.0.1:<random>`, then launch the system browser at `<authorization_endpoint>?client_id=…&request_uri=…`. The user logs in on Bluesky's site.
4. **Callback**. Bluesky redirects to `http://127.0.0.1:<port>/callback?code=…&state=…`. The loopback listener captures it, validates state, and tears itself down.
5. **Token exchange**. POST to the token endpoint with the code + DPoP proof (signed JWS over the exchange request). We get back a DPoP-bound access token + refresh token.
6. **Persist**. The `Session` (including the DPoP PKCS8 private key) goes to `~/Library/Application Support/ai.Smoo.smooblue/session.json` (mode 0600).

---

## DPoP

Every XRPC request signs a fresh DPoP proof (JWS over method + URL + nonce + access-token hash). The server uses the proof to verify the request is from the same client that minted the token. Replays of an access token from a different client fail.

We hold one ES256 keypair per Session. The keypair is regenerated on every fresh sign-in (not reused across accounts).

---

## Refresh

Access tokens last ~2 hours. Refresh tokens **rotate** — every refresh returns a new refresh token; the old one is dead. This is the source of the "auth doesn't carry over" bug we fixed: we used to write only the legacy single-slot keyring entry on refresh, leaving the per-DID slot with a dead refresh token that boot would pick up on the next launch.

`auth_refresh.rs::refresh_and_persist` now writes to **both** the legacy `session.json` and the per-DID `session-<did>.json` on every successful refresh, and boot picks the slot with the later `expires_at` when both exist.

---

## Session shape

```rust
pub struct Session {
    pub did: String,           // did:plc:abc
    pub handle: String,
    pub pds: String,           // user's PDS URL — what we hit for XRPC
    pub issuer: String,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,    // always "DPoP"
    pub expires_at: i64,       // unix seconds
    pub dpop_pem: String,      // ES256 PKCS8 PEM — the secret half
    pub dpop_nonce: Option<String>,
    pub token_endpoint: Option<String>,
}
```

`dpop_pem` is the most sensitive field — possession lets you forge proofs for the access token. Lives only on disk in a 0600 file owned by the user. Never sent over the wire.

---

## XRPC routing

All `app.bsky.*` calls go through the user's **PDS**, not directly to the AppView. The PDS proxies to the AppView with service-auth on behalf of the user. We never hit `api.bsky.app` directly with a user token (the AppView would return 401 AuthMissing).

`AtClient::new(session, base)` sets `base = session.pds` so every `session_pds_url(path)` builds a PDS-rooted URL.

---

## Why a custom OAuth crate vs. a library

ATproto OAuth is its own spec (PAR is RFC 9126, DPoP is RFC 9449, PKCE is RFC 7636 — none of the off-the-shelf Rust OAuth crates handle all three with the ATproto-specific glue). The crate exists so we control the loopback listener teardown, error model, and persistence shape end-to-end.

---

## Failure modes worth knowing

| Symptom | Cause | Fix |
| --- | --- | --- |
| `invalid_grant` on refresh | refresh token already used (rotated) | Boot pick-the-later-expires_at handles this; if it still happens, sign in again. |
| OAuth callback never fires | loopback port collided | Quit any prior dev instance — OAuth uses ephemeral ports per attempt. |
| `expected ES256 public point must have x` panic in tests | `p256` crate ABI change | Unreachable in practice; only fires if the crate generates malformed keys. |
| User has to sign in every launch | session file write failing silently | Check perms on `~/Library/Application Support/ai.Smoo.smooblue/`. |

---

## Related

- [[Architecture-Overview]]
- [[../Decisions/ADR-001-Session-File-vs-Keychain]] — why we left the Keychain
