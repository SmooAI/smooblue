# Auto-Updater

#operations

Hourly launchd job that keeps `/Applications/Smooblue.app` current with `main`. Optional — manual `bash scripts/smooblue-update.sh` works the same way.

---

## Install the launchd job

```bash
sed -e "s|@USER@|$USER|g" -e "s|@HOME@|$HOME|g" \
    scripts/ai.smoo.smooblue.updater.plist.template \
    > ~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist
launchctl load ~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist
```

Fires at login + every 3600s. Logs to `~/Library/Logs/Smooblue/update.log`.

---

## Uninstall

```bash
launchctl unload ~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist
rm ~/Library/LaunchAgents/ai.smoo.smooblue.updater.plist
```

---

## What the script does

`scripts/smooblue-update.sh`:

1. **Safety checks**: skip if HEAD isn't on `main`, skip if working tree is dirty
2. **Compare**: `git fetch origin main`; compare local vs remote SHA
3. **Rebuild trigger**:
   - New commits → `git pull --rebase` then rebuild
   - No new commits → check if `target/release/smooblue-app` or `dist/Smooblue.app/Contents/MacOS/Smooblue` is newer than the installed binary (handles "user manually built but didn't reinstall"). If so, reinstall.
   - Otherwise → exit, no-op
4. **Build**: `bash scripts/bundle-macos.sh`
5. **Running-app guard**: `pgrep -f "$INSTALL_PATH/Contents/MacOS/Smooblue"` — if Smooblue is open, skip the install and log it. Replacing a live binary risks SIGBUS on the next demand-paged code page fault.
6. **Atomic install**: move old `.app` aside, `cp -R` new one, `xattr -dr com.apple.quarantine`, clean up the backup

---

## Why these guards exist

| Guard | Without it |
| --- | --- |
| Branch != main | Updater clobbers your feature work mid-edit |
| Working tree dirty | Updater `git pull --rebase`s over uncommitted changes |
| Running-app check | Live binary overwrite → SIGBUS on next code page fault |
| Log not under `/tmp` | World-writable + sticky bit — any local user can symlink-attack the log path |
| `mkdir ~/Library/Logs/Smooblue` | First-run `tee -a` fails silently if the dir doesn't exist |

---

## Logs

```bash
tail -f ~/Library/Logs/Smooblue/update.log
```

Each run logs a UTC-timestamped header so you can tell when it last fired. Typical idle output:

```
=== 2026-05-25T03:00:00Z smooblue-update start ===
Already up to date at 468f2cc5e8f3.
```

On a real update:

```
=== 2026-05-25T04:00:00Z smooblue-update start ===
New commits: 468f2cc5e8f3 → a1b2c3d4e5f6
Building release bundle…
[bundle-macos.sh output]
Installing to /Applications/Smooblue.app…
Installed Smooblue a1b2c3d4e5f6 to /Applications/Smooblue.app
=== smooblue-update done ===
```

---

## Manual trigger

```bash
bash scripts/smooblue-update.sh
```

Same safety guards. Useful after merging a PR to `main` if you don't want to wait an hour.

---

## Override env vars

| Var | Default | Why |
| --- | --- | --- |
| `SMOOBLUE_REPO` | `$HOME/dev/smooai/smooblue` | Repo to pull from + build |
| `SMOOBLUE_INSTALL` | `/Applications/Smooblue.app` | Where the `.app` lands |
| `SMOOBLUE_LOG_DIR` | `$HOME/Library/Logs/Smooblue` | Log directory |
| `SMOOBLUE_LOG` | `$SMOOBLUE_LOG_DIR/update.log` | Log file path |

---

## Related

- [[Bundle-and-Install]]
- [[../Decisions/ADR-Index]]
