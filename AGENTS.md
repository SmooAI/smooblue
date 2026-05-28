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

Plain-English commit subjects. **No conventional-commit prefixes** (`feat:` / `fix:` / `chore:` / etc.) — versioning is decoupled from commit messages and runs through changesets instead.

## Versioning — changesets

```bash
pnpm changeset           # drop a changeset alongside any user-visible change
pnpm changeset:status    # see what's queued for the next release
pnpm run version         # consume changesets → bump Cargo.toml + write CHANGELOG.md
pnpm run release         # tag vX.Y.Z + push (CI builds + uploads the .app)
```

Full playbook in [`.changeset/README.md`](.changeset/README.md).

## Land the plane (session completion)

**Work is NOT done until everything is pushed AND a changeset is filed.** When you finish a chunk of work, run these in order. Skip steps only when they obviously don't apply (typo fix → no test run needed; nothing user-visible → no changeset).

1. `cargo fmt --all`
2. `cargo clippy --workspace --tests` — must be zero warnings
3. `cargo test --workspace --lib` — must be green
4. **Rebuild and install the .app locally** so the user can verify the change without waiting for a release. Skip only for changes that can't affect the running app (docs-only, CI tweaks, `.gitignore`). Hot-reload / `cargo run` is NOT a substitute — the bundle is what the user actually has open.

    ```bash
    bash scripts/bundle-macos.sh
    rm -rf /Applications/Smooblue.app
    cp -R dist/Smooblue.app /Applications/Smooblue.app
    xattr -dr com.apple.quarantine /Applications/Smooblue.app
    ```

    On Linux: `cargo build --release -p smooblue-app && cp target/release/smooblue ~/.local/bin/smooblue`.

5. **Drop a changeset** when the change is user-visible (any `crates/smooblue-*` source change qualifies):

    ```bash
    pnpm changeset
    ```

    Plain-English summary, pick `patch` / `minor` / `major` per impact. Internal-only changes (CI tweaks, docs-only, `.gitignore`) can skip.

6. `git add -A && git commit -m "Plain English subject"` — let the pre-commit hook re-run fmt/clippy/tests
7. `git push` — local-only is "halfway landed"; not done until origin has it

If a CI check fails after push, fix it in a follow-up commit. Don't leave a red build for the next session.

**Cutting a release** (after a chunk of changesets has accumulated):

```bash
pnpm run release    # bumps versions, commits, pushes, tags, pushes tag
```

That's the whole ritual. The script:
1. Refuses to start on a dirty tree (so the Release commit only contains the version bump)
2. `git pull --rebase` so we don't push a Release commit on top of stale main
3. Consumes pending changesets → bumps `package.json` + `Cargo.toml` + writes CHANGELOG.md
4. Commits + pushes the bump
5. Tags + pushes `vX.Y.Z`

CI takes it from there — builds .app + .deb + tarball, attaches them to the GitHub release, and auto-bumps the Homebrew tap. `GITHUB_TOKEN` is auto-derived from `gh auth token` if not exported; needed by the `@changesets/changelog-github` plugin to resolve PR titles. Idempotent — re-runs with no pending changesets or an existing tag no-op cleanly.

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
