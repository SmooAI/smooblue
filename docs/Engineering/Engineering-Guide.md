# Engineering Guide

#engineering #moc

Daily workflow, commit conventions, test discipline, release.

---

## Workflow

```bash
# Pull, branch, work, push
git switch -c fix/whatever
# … code, test …
cargo test --workspace --lib
git commit -m "fix: …"      # conventional-commit prefix is load-bearing (release-plz)
git push -u origin fix/whatever
gh pr create

# After PR merges:
git switch main && git pull --rebase
```

Branch protection on `main`: linear history required, both `cargo test (ubuntu-latest)` and `cargo test (macos-latest)` must pass, no force-pushes, no deletions. Admins may bypass.

---

## Commit conventions

release-plz reads conventional commits to generate the CHANGELOG and bump versions:

| Prefix | Bumps | Use for |
| --- | --- | --- |
| `feat:` | minor | New user-visible feature |
| `feat!:` | **major** | Breaking change (note the `!`) |
| `fix:` | patch | Bug fix |
| `chore:` | patch | Internal / non-user-visible |
| `perf:` | patch | Performance with no behavior change |
| `docs:` | patch | Documentation only |

We collapse small fixes into the next feature commit when shipping fast — release-plz is fine either way.

---

## Testing

- **Unit tests** live alongside the code (`mod tests` at the bottom of each file). `cargo test --workspace --lib` runs them all (~110 at 1.0, ~20s).
- **Integration tests** with mock HTTP via `wiremock-rs` are in `crates/smooblue-atproto/src/client.rs` test module.
- **Real-bluesky tests** in `crates/smooblue-atproto/tests/real_bluesky.rs` hit `api.bsky.app`. Run with `cargo test -- --ignored --test-threads=1` and a valid `BSKY_TEST_*` env (intentionally manual — don't want CI hammering the public AppView).
- **UI tests** are mostly absent — Dioxus doesn't have a headless renderer for our setup. We test by running locally.

Test discipline: any new XRPC method gets a wiremock test. Any new lexicon shape decode gets at least one serde round-trip test. The defensive `serde_json::Value` fields on `FeedItem.reply` / `.reason` exist *because* we got bitten by strict struct deserialization breaking the whole feed page on one weird item.

---

## Demo mode

`SMOOBLUE_DEMO=1` injects a synthetic session + canned data so the app boots straight into the deck. `SMOOBLUE_DEMO_SCALE={small,medium,large,huge,insane}` controls how many posts each column returns. Use `large` (500/column) for scale tests — anything bigger usually OOMs the renderer's image cache, which is real-app territory we don't optimize for.

See [[Demo-Mode]] for the full setup.

---

## Persistence locations

Everything user-writable lands in `directories::ProjectDirs::from("ai", "Smoo", "smooblue").config_dir()`, which on macOS is `~/Library/Application Support/ai.Smoo.smooblue/`:

| File | What |
| --- | --- |
| `session.json` | Legacy single-account session blob (0600) |
| `session-<did>.json` | Per-account session blob for multi-account |
| `accounts.json` | Index `{ active_did, accounts: [{did, handle}] }` |
| `columns.json` | Deck layout |
| `last_handle.txt` | Login pre-fill |
| `draft.txt` | In-progress compose body (saved on every keystroke) |
| `theme.txt` | `dark` or `light` |

Auto-updater logs live at `~/Library/Logs/Smooblue/update.log`.

---

## Release

We use [release-plz](https://release-plz.dev) — `publish = false` on every crate (smooblue is an app, not a library). After commits land on `main`, release-plz opens a PR with version bumps + `CHANGELOG.md` entries derived from conventional commits. Merging that PR tags + creates the GitHub release.

The user-visible distribution is the macOS `.app` bundle from `scripts/bundle-macos.sh` — Apple Developer notarization is a future pearl; for now bundles are adhoc-signed (first-run Gatekeeper requires right-click → Open).

---

## Tracking work — pearls

Pearls (`th pearls`) is the local work-item tracker. Lives at `.smooth/dolt/`, syncs via git (auto-commit hooks set up by `th pearls init`).

```bash
th pearls ready                              # Show issues ready to work
th pearls list --status=in_progress          # Your active work
th pearls show <id>                          # Full view
th pearls create --title="..." --description="..." --type=task --priority=2
th pearls update <id> --status=in_progress
th pearls close <id1> <id2> ...
th pearls push                               # Push pearl DB to git
```

Priorities are `0–4` (`0` = critical, `2` = medium, `4` = backlog).

---

## /save-status

Long sessions accumulate context that's expensive to rebuild. `/save-status [topic]` writes a snapshot to `docs/Projects/Status-YYYY-MM-DD-HHMM-<slug>.md` capturing the git/gh/pearls state plus the **why** behind in-flight work. Future agents (or future you) can pick up cold from one of those files.

Run it before walking away from non-trivial work or before a long context-compaction.

---

## Related

- [[Adding-a-Column-Type]]
- [[Adding-an-XRPC-Endpoint]]
- [[Demo-Mode]]
- [[../Operations/Bundle-and-Install]]
