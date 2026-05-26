# Changesets

How smooblue manages versions + the CHANGELOG.

## Adding a changeset

When you land a change that should appear in a release, drop a changeset:

```bash
pnpm changeset
```

Pick the bump kind (patch / minor / major) and write a short summary. The CLI drops a markdown file in `.changeset/` — commit it alongside the change.

You don't need a changeset for purely-internal work (CI tweaks, doc fixes, etc.). When in doubt: patch + one-line summary.

## Cutting a release

A maintainer (or the release workflow) runs:

```bash
pnpm version          # consumes all .changeset/*.md → bumps Cargo.toml + CHANGELOG.md
git commit -am "Release vX.Y.Z"
pnpm release          # tags vX.Y.Z, pushes, GitHub Actions builds + uploads the .app
```

## Why changesets and not release-plz / conventional commits

Commit messages stay free-form English. Whether something is a patch / minor / major is decided in the PR itself via a small intent-declaring file — separate from the commit grammar. Same flow as the smoo monorepo + every other Brent repo. release-plz was tried briefly; the conventional-commit prefixes felt fiddly and the auto-PR was hard to control.

## Format reference

A changeset looks like:

```markdown
---
"smooblue": minor
---

Single-flight refresh — concurrent column polls were racing tokens.
```

(One paragraph is fine. Use bullet lists for multi-pointed changes.)
