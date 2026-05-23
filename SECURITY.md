# Security Policy

## Supported versions

Smooblue is in early development. Only `main` receives security fixes.

## Reporting a vulnerability

Please email **security@smoo.ai** rather than filing a public issue. Include:

- a description of the issue,
- steps to reproduce (or a proof-of-concept),
- the impact you've identified,
- your suggested fix, if you have one.

We aim to respond within 72 hours. If the issue is confirmed, we'll work
with you on a coordinated disclosure timeline before public release.

## Scope

In scope:

- The Smooblue desktop binary and its OAuth/DPoP handling.
- The `smooblue-oauth`, `smooblue-atproto`, `smooblue-app`, and
  `smooblue-theme` crates.

Out of scope:

- Bluesky / ATproto protocol-level issues — please report those to
  [Bluesky Social](https://bsky.social).
- Issues that depend on the user installing a malicious build of Smooblue.
