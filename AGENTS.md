# Agent guide — Smooblue

If you're reading this, you're working in this repo on behalf of someone. Here's the load-bearing context.

## Where the docs live

The full Obsidian vault is at `docs/`. Start at [`docs/Home.md`](docs/Home.md). Highlights:

- **Architecture** — `docs/Architecture/Architecture-Overview.md` and `docs/Architecture/OAuth-and-Session.md`
- **Engineering** — `docs/Engineering/Engineering-Guide.md` (workflow, commit conventions, tests) + `docs/Engineering/Adding-a-Column-Type.md` + `docs/Engineering/Adding-an-XRPC-Endpoint.md`
- **Operations** — `docs/Operations/Bundle-and-Install.md`, `docs/Operations/Auto-Updater.md`, `docs/Operations/Branch-Protection.md`
- **Decisions** — `docs/Decisions/ADR-Index.md` (read before reaching for the Keychain, the `open` command, or `cargo publish`)
- **Projects** — `docs/Projects/_Projects-Index.md` (most recent status snapshot)

## Tracking work — pearls

This repo uses `th pearls` for work tracking (Dolt-backed, syncs via git):

```bash
th pearls ready                              # Issues ready to work
th pearls create --title="..." --description="..." --type=task --priority=2
th pearls update <id> --status=in_progress
th pearls close <id>
th pearls push                               # Push pearl DB to git
```

Don't use TodoWrite/ad-hoc markdown lists for multi-turn task tracking — use pearls.

## Commits

Conventional-commit prefixes (`feat:` / `fix:` / `chore:` / `perf:` / `docs:`) — release-plz reads them to generate CHANGELOG entries and bump versions. Use `feat!:` for breaking changes.

## Before doing anything load-bearing

- **Hardening anything around `open` / URL handling**? Read `docs/Decisions/ADR-002-Safe-Open-Allowlist.md`.
- **Touching the session / auth path**? Read `docs/Decisions/ADR-001-Session-File-vs-Keychain.md` and `docs/Architecture/OAuth-and-Session.md`.
- **Considering crates.io publish**? Read `docs/Decisions/ADR-003-Publish-False-Workspace-Wide.md` first.
- **About to walk away mid-task**? Run `/save-status <topic>` so the next agent can pick up cold.

## Don't

- Don't read `FeedItem.reply` / `.reason` as typed structs — they're `serde_json::Value` on purpose. Helpers on `FeedItem` (`reposter_display`, `reposter_did`, `reply_parent_handle`) are the supported access path.
- Don't capture signals by value outside an `async` closure for `use_resource` — read them inside.
- Don't add `Command::new("open").arg(url)` — route through `crate::safe_open::open_in_browser`.
- Don't bypass the `cargo test (ubuntu-latest)` + `cargo test (macos-latest)` checks on `main`. They're required; admins may bypass but shouldn't.
