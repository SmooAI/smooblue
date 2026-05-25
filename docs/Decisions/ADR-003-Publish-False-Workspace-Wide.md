# ADR-003 · `publish = false` Workspace-Wide

#decisions

**Status**: Accepted (1.0)
**Date**: 2026-05-25

---

## Context

Smooblue is a Cargo workspace with five crates:

- `smooblue-app` — the Dioxus desktop binary (the product)
- `smooblue-atproto` — XRPC client library
- `smooblue-oauth` — ATproto OAuth library
- `smooblue-crm` — opt-in Smoo CRM sync
- `smooblue-theme` — CSS tokens + shared sheet

`release-plz` (the CI release manager) needs every crate to declare whether it's published to crates.io.

We could publish the four library crates and keep the app private. There's some appeal — `smooblue-atproto` and `smooblue-oauth` are arguably general-purpose ATproto components.

---

## Decision

`publish = false` on **every** crate in the workspace. The release pipeline ships GitHub releases + the macOS `.app` bundle. No crates.io.

`release-plz.toml`:
```toml
[[package]]
name = "smooblue-app"
publish = false
[[package]]
name = "smooblue-atproto"
publish = false
# ...
```

---

## Why

1. **Pre-1.0 API churn we just absorbed**. The library crates' surface area was reshaped multiple times in the last two days (defensive `serde_json::Value` on FeedItem, multi-account session methods, new endpoints). Publishing them means we owe semver to downstream users. That's an obligation we don't want to take on while the surface is still settling.
2. **The bsky lexicon itself is unstable**. Several fields we model are on the `unspecced` lexicon path (trending topics, popular feed generators). When those move, we want to update without a coordinated release.
3. **The crate names assume context** — `smooblue-atproto` is "the XRPC client smooblue uses," not "a general-purpose ATproto client." A real library would be named accordingly.
4. **`smooblue-app` isn't a library at all**. It's a binary that links its own dependencies. Even if we wanted to publish it, `cargo install smooblue-app` would skip the icon assets, the Info.plist, the launchd plist template — everything that makes Smooblue a product.

---

## What we'd do if we changed our minds

If `smooblue-atproto` graduates to a general-purpose Rust ATproto client (separate from smooblue's product roadmap):

1. Fork it into its own repo, rename to something like `atproto-client` or `bsky-rs`
2. Stabilize the surface — `Url::parse` everywhere, real error types not strings, proper feature flags
3. Add docs.rs-quality rustdoc
4. Publish under a 0.1.x line with clear "API may break" upfront

That's a separate decision. Today, smooblue's library crates ride with the app and version in lock-step (workspace version, currently `1.0.0`).

---

## Consequences

**Pro:**
- Free to refactor library crates without breaking external users
- One version line for the whole workspace
- release-plz config stays simple

**Con:**
- Anyone building a Rust ATproto client has to vendor or git-dep our crates instead of pulling from crates.io
- "smooblue-atproto would be useful to others" is true and we're not capturing it

---

## Related

- `release-plz.toml`
- [[../Engineering/Engineering-Guide#Release]]
