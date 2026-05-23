# Smooblue

A native, multi-column [Bluesky](https://bsky.app) client for desktop. Inspired by [deck.blue](https://deck.blue).

Built in Rust with [Dioxus](https://dioxuslabs.com/), uses Bluesky's official
OAuth flow (PAR + PKCE + DPoP-bound tokens) — no app passwords, no storing
your password on disk.

<p align="center">
  <img src="assets/icons/icon-256.png" alt="Smooblue" width="180" />
</p>

## Features

- **Multi-column deck**: stack Home, Notifications, Discover, custom feeds, lists, and search side-by-side
- **Bluesky OAuth** with DPoP-bound access tokens — no password handling, full account portability
- **Native performance**, native window chrome, native scrolling
- **Tiny**: single-binary install, ~30 MB

## Status

🚧 **Early development.** Foundation is in place — OAuth + ATproto client + Home column work end-to-end against the real Bluesky network. Other column types and post composition are in progress.

| Feature                                  | Status   |
| ---------------------------------------- | -------- |
| Bluesky OAuth (PAR + PKCE + DPoP)        | ✅ done  |
| Home column (`getTimeline`)              | ✅ done  |
| Author / Profile column (`getAuthorFeed`)| ⏳ wip   |
| Notifications column                     | ⏳ wip   |
| Discover / custom feed columns           | ⏳ wip   |
| Search column                            | ⏳ wip   |
| Compose + reply + repost + like          | ⏳ wip   |
| Rich-media renderer (video, embeds)      | ⏳ wip   |
| Drag-to-reorder columns                  | ⏳ wip   |
| Packaged `.app` bundle / installer       | ⏳ wip   |

## Quick start

```bash
# requires Rust 1.80+
cargo run --release --bin smooblue-app
```

You'll need a Bluesky account. The app opens your default browser for sign-in;
tokens are kept in your OS keychain.

> **Note**: until the public client metadata is hosted at
> `https://smoo.ai/smooblue/client-metadata.json`, OAuth sign-in will fail.
> If you'd like to run it before then, set `SMOOBLUE_CLIENT_ID` to your own
> hosted [ATproto client metadata document](https://atproto.com/specs/oauth#clients).

## Privacy: what Smooblue sends where

| Data                      | Sent to                                  | When                                                                 |
| ------------------------- | ---------------------------------------- | -------------------------------------------------------------------- |
| Handle, password (typed)  | **Nowhere** — Bluesky handles auth       | Never; OAuth means Smooblue never sees your password                 |
| Bluesky access token      | The Bluesky AppView (`api.bsky.app`)     | Every API call (Home, timeline, notifications, etc.)                 |
| OS keychain entry         | Local only                               | After successful sign-in (tokens persist between launches)           |
| Display name, handle, DID | **Smoo AI CRM** *(opt-in only)*          | Only if you tick "Stay in touch with Smoo AI" during sign-in         |
| Crash reports             | None (yet)                               | Once observability is wired in, this row will be opt-in too          |

The Smoo AI CRM sync is off by default. Toggle it at any time in
**Settings → Account → Share with Smoo AI**. Opting out also requests
deletion of any previously-synced profile data.

## Building

```bash
# all tests (~30s)
cargo test --workspace

# real-Bluesky integration tests (hits api.bsky.app)
cargo test --workspace -- --ignored --test-threads=1

# format + lint
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings

# regenerate app icon PNGs from the source SVG (needs librsvg)
./scripts/build-icons.sh
```

## Layout

```
smooblue/
├── crates/
│   ├── smooblue-app/      # Dioxus desktop binary + components
│   ├── smooblue-atproto/  # XRPC client (timeline, profile, notifications)
│   ├── smooblue-oauth/    # ATproto OAuth (PAR + PKCE + DPoP)
│   └── smooblue-theme/    # smoo color tokens compiled to CSS
├── assets/
│   ├── icons/             # generated PNG app icons (16 → 1024)
│   ├── icon.svg           # source SVG (smoo monogram + Bluesky butterfly)
│   └── styles.css         # smoo design-system stylesheet
├── scripts/
│   └── build-icons.sh
└── Cargo.toml             # Cargo workspace
```

## Built on SmooAI's Rust crates

Smooblue uses the [Smoo AI](https://smoo.ai) Rust packages where they fit:

- [`smooai-config`](https://crates.io/crates/smooai-config) — config + secrets
- [`smooai-logger`](https://crates.io/crates/smooai-logger) — structured logs
- [`smooai-fetch`](https://crates.io/crates/smooai-fetch) — resilient HTTP

## Contributing

Issues and PRs welcome! See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE) © [Smoo AI](https://smoo.ai)

---

*Smooblue is not affiliated with Bluesky Social, PBC. "Bluesky" and the
Bluesky butterfly are trademarks of Bluesky Social, PBC.*
