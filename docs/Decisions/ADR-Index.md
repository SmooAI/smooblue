# ADR Index

#decisions #moc

One-way doors documented here. Numbered sequentially. If you're considering a non-obvious architectural choice — write the ADR *before* the implementation lands so reviewers can read the rationale alongside the code.

---

| # | Title | Status |
| --- | --- | --- |
| [[ADR-001-Session-File-vs-Keychain]] | Store the OAuth session in a 0600 file instead of the macOS Keychain | Accepted (1.0) |
| [[ADR-002-Safe-Open-Allowlist]] | Allowlist http/https for every `open` call site | Accepted (1.0) |
| [[ADR-003-Publish-False-Workspace-Wide]] | Don't publish smooblue crates to crates.io | Accepted (1.0) |

---

## Template

Copy [[../_templates/ADR-Template]] to start a new one. Pick the next number from this table.

ADR statuses: `Proposed` · `Accepted` · `Superseded by ADR-NNN` · `Deprecated`.
