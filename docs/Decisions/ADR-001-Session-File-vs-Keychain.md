# ADR-001 · Session File vs Keychain

#decisions

**Status**: Accepted (1.0)
**Date**: 2026-05-25

---

## Context

OAuth sessions include a DPoP private key + access + refresh tokens. They're sensitive: possession of the DPoP key + a live access token lets an attacker impersonate the user against their PDS. Where do we store them?

The conventional choice on macOS is the **Keychain**: encrypted at rest, ACL-gated per-app, surveyed by Apple's own security tooling. The `keyring-rs` crate makes Keychain access a one-liner.

We shipped pre-1.0 with Keychain storage. It broke on every rebuild.

---

## The bug

macOS Keychain ACLs are bound to the **code signature** of the app that wrote the entry. Smooblue is adhoc-signed (no Developer ID cert); every `cargo build --release` produces a binary with a different ad-hoc hash, which Keychain reads as "a different app trying to access the entry." First read after each rebuild either:

- Pops the user a "Smooblue wants to access keychain item …" dialog (annoying), or
- Silently fails — `keyring::Entry::get_password` returns `Err(NoEntry)` and we treat it as "no session," dropping the user back at login.

Users hit "auth doesn't carry over" on every install of a fresh build. The auto-updater amplified the problem: hourly rebuild → hourly forced re-auth.

---

## Decision

Move sessions to **plain-text JSON files** under `~/Library/Application Support/ai.Smoo.smooblue/`:

- `session.json` — legacy single-account slot
- `session-<sanitized_did>.json` — per-account slots for multi-account
- All written via atomic `write_secret(path, data)` — write to `.tmp`, `chmod 0600`, then `rename`

File permission `0600` (owner-only read/write) is the storage-level mitigation. The threat model:

- **Local attacker with the user's UID** can read the file regardless of any encryption scheme — same risk as cookie databases, ssh keys, `.aws/credentials`. Out of scope.
- **Local attacker as a different UID** can't read `0600` files owned by us.
- **Backup snapshots** (Time Machine, etc.) include `~/Library/Application Support/` — same as cookies/keys.

---

## Consequences

**Pro:**
- Auth survives every rebuild — adhoc-signed or not
- Works identically on Linux when we eventually port (Linux has Secret Service but it's also app-identity-tied)
- One less dependency (`keyring` crate dropped)
- Easier debugging: `cat session.json | jq` instead of `security find-generic-password`

**Con:**
- No OS-level encryption-at-rest. Same posture as ~every other developer-tool that caches auth tokens locally (Slack, Discord, GH CLI, Docker, ...). Acceptable.
- Multi-user macOS install (rare) means each user has their own session, which is correct.

---

## Alternatives considered

| Option | Why not |
| --- | --- |
| Get a real Apple Developer cert + notarize | Worth doing eventually, separate decision. Doesn't address Linux portability. |
| Use Keychain with `allow_any_app=true` | Defeats the security model the Keychain offers. If we don't want the ACL, we shouldn't use the Keychain. |
| Encrypt the file with a user-derived key | Bootstrap problem — where do we store the key? Just shifts the goalpost. |
| OS-derived key (Secure Enclave) | Same code-signature problem as Keychain. |

---

## Migration

Pre-1.0 users with Keychain-stored sessions need to re-auth once. Acceptable one-time cost for permanent fix.

`persistence::load_accounts` checks for a `session.json` if `accounts.json` is missing, treating the file as the migration source. Keychain entries are not read — they're effectively orphaned, but harmless (the user can clear them in Keychain Access if they care).

---

## Related

- [[../Architecture/OAuth-and-Session]]
- `crates/smooblue-app/src/persistence.rs::write_secret`
