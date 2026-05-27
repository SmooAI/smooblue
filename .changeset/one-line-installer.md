---
"smooblue": patch
---

`install.sh` at the repo root: one-line installer that pulls the latest GitHub release zip, drops `Smooblue.app` into `/Applications` (or `~/Applications` if that's not writable), strips the quarantine xattr, and opens it.

```bash
curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
```

Idempotent — re-running upgrades in place. Apple Silicon only today (the release pipeline only ships `Smooblue-macos-arm64.zip`); x86_64 + Linux + Windows users get a clear error pointing at the build-from-source steps. `SMOOBLUE_NO_OPEN=1` to install without launching.
