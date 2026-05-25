---
cssclasses:
    - home-page
---

# Smooblue Documentation

#moc

<div align="center">

![[smooblue-256.png|180]]

</div>

> [!smoo] About Smooblue
> A native, multi-column [Bluesky](https://bsky.app) desktop client. Rust + [Dioxus](https://dioxuslabs.com/) on macOS, backed by Bluesky's official OAuth flow (PAR + PKCE + DPoP-bound tokens). Single binary, ~11 MB, opinionated about brand + keyboard navigation.

---

## Section Index

| Section                                  | Description                                                              |
| ---------------------------------------- | ------------------------------------------------------------------------ |
| [[Start-Here/Onboarding]]                | Clone, build, run; required tooling; layout of the workspace             |
| [[Architecture/Architecture-Overview]]   | Crate breakdown, deck model, OAuth + DPoP, persistence, render pipeline  |
| [[Engineering/Engineering-Guide]]        | Daily workflow, commit conventions, testing, release-plz, demo mode      |
| [[Operations/Operations-Overview]]       | Bundle + install + auto-updater + log locations + branch protection      |
| [[Decisions/ADR-Index]]                  | Architecture Decision Records (one-way doors documented here)            |
| [[Projects/_Projects-Index]]             | Status snapshots from `/save-status` (in-flight context for next agent)  |

---

## Quick Links

### Build + run

- **First build** — [[Start-Here/Onboarding]]
- **Bundle + install the .app** — [[Operations/Bundle-and-Install]]
- **Auto-updater on launchd** — [[Operations/Auto-Updater]]
- **Demo mode for screenshots / scale tests** — [[Engineering/Demo-Mode]]

### Architecture cheat sheet

- **How a column fetches** — [[Architecture/Architecture-Overview#Columns]]
- **OAuth + DPoP** — [[Architecture/OAuth-and-Session]]
- **Why we file-store the session** — [[Decisions/ADR-001-Session-File-vs-Keychain]]
- **URL scheme allowlist on `open`** — [[Decisions/ADR-002-Safe-Open-Allowlist]]

### Editing flow

- **Add a new column type** — [[Engineering/Adding-a-Column-Type]]
- **Add an XRPC endpoint to the client** — [[Engineering/Adding-an-XRPC-Endpoint]]
- **Land a fix** — [[Engineering/Engineering-Guide#Workflow]]

---

## Status at a glance

- **Version**: `1.0.0` (workspace-wide; tagged `v1.0.0`)
- **Platform**: macOS only (code portable; CI for Linux/Windows is a future pearl)
- **Tests**: 110 unit, all green
- **Auth**: OAuth + DPoP; session stored as `~/Library/Application Support/ai.Smoo.smooblue/session.json` (0600)
- **Branch protection**: required status checks + linear history on `main`
- **Releases**: managed by [release-plz](https://release-plz.dev/) — `publish = false` per crate (smooblue is an app, not a library)

See [[Projects/_Projects-Index]] for the most recent status snapshot.
