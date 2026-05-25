# Operations Overview

#operations #moc

How Smooblue gets built, packaged, installed, updated, and logged.

---

## Sections

| Page | What |
| --- | --- |
| [[Bundle-and-Install]] | `scripts/bundle-macos.sh` → `dist/Smooblue.app` → `/Applications/` |
| [[Auto-Updater]] | Hourly launchd job that pulls + rebuilds + reinstalls from `main` |
| [[Branch-Protection]] | What's enforced on `main` and what bypass paths exist |

---

## Distribution

| Channel | Where | How |
| --- | --- | --- |
| **Direct (current)** | Clone + `bundle-macos.sh` + `cp -R` | Manual; `xattr -dr com.apple.quarantine` once on first install |
| **Auto-updater** | Per-developer launchd agent | One-time install of `~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist` |
| **GitHub Releases** | release-plz tags + GH releases | Tags `v1.0.0`-style; no `.app` artifact attached yet (future pearl) |
| **Notarized** | Apple Developer cert | Not yet — adhoc-signed today |
| **Crates.io** | Library crates | `publish = false` workspace-wide; smooblue is an app |

---

## Logs

| What | Where |
| --- | --- |
| Auto-updater | `~/Library/Logs/Smooblue/update.log` (moved off `/tmp` for safety) |
| App runtime logs | stdout/stderr — `tail -f /Applications/Smooblue.app/Contents/MacOS/Smooblue` via Console.app if you really need them |
| SST-style structured logs | n/a — smooblue uses `tracing` to stderr only |

---

## Branch / repo state

- **Branch protection** on `main`: required status checks (`cargo test (ubuntu-latest)` + `cargo test (macos-latest)`), linear history required, no force-pushes, no deletions. Admins may bypass (so emergency fixes don't get stuck behind a green build).
- **release-plz** opens version-bump PRs from `main` based on conventional-commit prefixes since the last release.
- **`publish = false`** on every crate — smooblue is a binary app, not a library, and we explicitly don't ship the workspace to crates.io.

---

## Incident playbook

| Symptom | First check | If that's fine, check |
| --- | --- | --- |
| Auto-updater stopped firing | `launchctl list | grep smooblue` | `tail ~/Library/Logs/Smooblue/update.log` |
| App opens to login every launch | `ls ~/Library/Application\ Support/ai.Smoo.smooblue/session*.json` | Re-auth and watch the file get written |
| Cmd+Up / BetterSnapTool doesn't reach Smooblue | Is `main.rs::activate_macos_app()` being called at startup? | Click the menu bar once to confirm activation is the gap |
| Compose 400 with `Missing required key "image"` | Check `PostImage`'s serde rename | Lexicon key is `image`, not `blob` — already fixed; regression test exists |
| Feed columns silently empty | `FeedItem.reply`/`.reason` deserialization | Both are `serde_json::Value` now — defensive; if they're strict structs again, that's the bug |
| URL handler popped open from a feed link click | This shouldn't happen | `safe_open` allowlist regression — every `Command::new("open")` site should go through `crate::safe_open` |

See [[../Architecture/OAuth-and-Session]] for auth-specific incidents.
