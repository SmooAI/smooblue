# Releasing Smooblue

Smooblue uses **[release-plz](https://release-plz.dev)** for version
management — the Rust equivalent of `@changesets/cli`. The flow
mirrors what the smoo monorepo does with changesets: a bot opens a
release PR, you review + merge, the tag + GitHub release happen
automatically.

## TL;DR

1. Land changes on `main` with **conventional-commit** messages
   (`feat:`, `fix:`, `chore:`, `docs:`, etc).
2. release-plz opens a "release v0.X.0" PR within a few minutes of
   the push, with version bumps in every `Cargo.toml` and a fresh
   `CHANGELOG.md` entry.
3. Review the PR. Approve + merge it.
4. The next push to `main` triggers the **release** job: tags the
   commit `v0.X.0`, creates a GitHub release with the changelog as
   the body, builds the macOS `.app` bundle (adhoc-signed), and
   uploads it as `Smooblue-macos-arm64.zip`.

That's it. Two clicks per release (approve + merge).

## Why conventional commits

release-plz reads `git log` between the previous tag and `HEAD` and
groups commits by their conventional prefix:

| Prefix | Changelog group | Bumps |
| --- | --- | --- |
| `feat:` | Added | **minor** (0.X.0) |
| `fix:` | Fixed | patch (0.0.X) |
| `perf:` | Performance | patch |
| `refactor:` / `chore:` | Changed | patch |
| `docs:` | Documentation | patch |
| `feat!:` or footer `BREAKING CHANGE:` | Breaking | **major** (X.0.0) |
| `test:` / `ci:` / `build:` / `style:` | (skipped — no entry) | — |
| `pearl:` / `release:` | (skipped) | — |

Anything else still lands under "Changed" so no commit ever silently
goes missing from the changelog.

## Per-package version

All workspace crates bump together — `smooblue-app`, `-atproto`,
`-crm`, `-oauth`, `-theme`. The version lives at `[workspace.package].version`
in the root `Cargo.toml`. Shipping mismatched per-crate versions
would just confuse users; this is a single product.

## Not publishing to crates.io

`publish = false` is set per-package in `release-plz.toml`. Smooblue
is an application, not a library, so we don't push to crates.io.
Future maintainers: don't enable that unless one of the crates
becomes genuinely useful as a standalone dependency.

## What ships in the release

- **Source code** — the standard GitHub "Source code (zip)" + "(tar.gz)" archives.
- **`Smooblue-macos-arm64.zip`** — the bundled `.app`, ready to
  drag to `/Applications`. **Adhoc-signed only** for now: first-run
  Gatekeeper warns the user to right-click → Open. Real Apple
  Developer ID signing + notarization wait on pearl `th-4149eb`
  (one-time enrollment in the Apple Developer Program). When that
  lands, `scripts/sign-and-notarize-macos.sh` is wired into the
  release workflow before the upload step.

## Releasing a hotfix on a side branch

release-plz only watches `main`. For a hotfix on a maintenance branch
(e.g. `v0.X-maint`), run release-plz locally:

```bash
cargo install release-plz   # one-time
release-plz update           # bumps Cargo.toml + writes CHANGELOG
git commit -am 'release: smooblue v0.X.1'
git tag v0.X.1
git push origin v0.X-maint --tags
# then trigger the release.yml workflow manually for that tag
```

## Manual sanity check

Before opening a release PR for the first time, run release-plz
locally in dry-run mode:

```bash
cargo install release-plz
release-plz update --dry-run
```

It'll print the proposed version bump + changelog content without
touching any files.
