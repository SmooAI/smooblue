---
"smooblue": patch
---

No-op patch bump to smoke-test the PAT-driven auto-tag flow (pearl th-5b49e0). The previous publish path relied on `pull_request_target:closed` firing from a GITHUB_TOKEN-authored auto-merge — which GitHub's anti-loop guard silently suppressed. Auto-merge now runs under a fine-grained PAT (`RELEASE_PAT`) so the merge commit is attributed to a real user and the downstream event fires normally. If this changeset rides through to v1.4.1 hands-off (Release PR opens → CI passes → auto-merge → publish job fires → v1.4.1 tag → release.yml builds + ships .app/.deb/.tar.gz + brew tap bumps), the fix is verified.
