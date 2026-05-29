# Smooblue Security

#security

This is the honest writeup. Where Smooblue is stronger than a browser tab, it says so. Where the browser wins, it says so too. If you're security-conscious enough to be reading this, you don't want hand-waving — so this page leans toward "here's the exact mechanism, here's how to verify it yourself."

If you spot something wrong or want to coordinate disclosure on a vulnerability, email **brent@smoo.ai** or open a private security advisory at [github.com/SmooAI/smooblue/security/advisories](https://github.com/SmooAI/smooblue/security/advisories).

---

## TL;DR

| Concern                    | Status                                                                                                                          |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Authentication             | ATproto OAuth (PAR + PKCE + DPoP). Never sees your password. Strictly stronger than app passwords.                              |
| Transport                  | TLS via `rustls` (no OpenSSL). Cert verification is on by default; no insecure fallbacks.                                       |
| Per-request authenticity   | DPoP-signed JWS over each XRPC call — captured requests can't be replayed without the local ES256 private key.                  |
| Public post content        | Cleartext at the PDS / Relay / AppView **by AT Protocol design**. Not a client choice; we're faithful to the protocol.          |
| DMs                        | Not supported in Smooblue today. Bluesky's `chat.bsky.*` is TLS-in-transit but not yet E2E-encrypted upstream.                  |
| Token storage              | Local file at `0600` in your user config dir. DPoP key never leaves disk.                                                       |
| Data egress                | Your PDS + AppView only. Zero third-party telemetry. One opt-in to a Smoo AI CRM, off by default.                               |
| URL/scheme hardening       | All "open in browser" calls go through a scheme allowlist (`http`/`https` only). Documented as [ADR-002](../Decisions/ADR-002-Safe-Open-Allowlist.md). |
| macOS code signing         | Adhoc-signed today (gap — see "What's NOT done" below). Apple Developer ID notarization tracked.                                |
| Sandboxing                 | Not sandboxed. Plain UNIX process with your user's full permissions. Gap, see below.                                            |
| Update mechanism           | Optional. When enabled, only pulls from the project's `main` branch on GitHub. No telemetry, no auto-installs from third parties. |
| Supply chain               | Pure-Rust dependency tree; `cargo audit` runnable on every release. No prebuilt binary downloads at runtime.                    |

---

## Authentication — why this is stronger than a browser app-password flow

Smooblue uses [ATproto's OAuth flow](https://docs.bsky.app/docs/advanced-guides/oauth-client) end-to-end. The Bluesky-recommended OAuth path is **strictly more secure than app passwords** because:

1. **Smooblue never sees your password.** Sign-in happens in your system browser, on `bsky.social` (or whatever PDS you use). Smooblue only gets back a short-lived access token. App passwords, by contrast, are typed *into* the client app — every app password user is trusting every client app they've ever entered the password into.
2. **PKCE** ([RFC 7636](https://datatracker.ietf.org/doc/html/rfc7636)) protects the OAuth code exchange against interception. Even if a network attacker intercepts the authorization code, they can't redeem it for tokens without the code verifier that Smooblue holds in memory.
3. **PAR** ([RFC 9126](https://datatracker.ietf.org/doc/html/rfc9126)) pushes the authorization parameters to the AS server-side before launching the browser, so a malicious link can't alter scopes or redirect URIs mid-flow.
4. **DPoP** ([RFC 9449](https://datatracker.ietf.org/doc/html/rfc9449)) binds the access token to a key Smooblue generates locally. Every XRPC request signs a fresh DPoP proof; even if the access token is stolen, an attacker can't use it without also stealing the DPoP private key. That key never leaves your machine.

Full mechanism: [Architecture/OAuth-and-Session.md](../Architecture/OAuth-and-Session.md).

**One tradeoff to be aware of:** tokens persist across launches so you don't have to sign in every time. They live in a `0600`-permissioned file in your user config directory:

- macOS: `~/Library/Application Support/ai.Smoo.smooblue/session.json`
- Linux: `~/.config/ai.Smoo.smooblue/session.json` (XDG)

Any process running as your user can read that file. That's the same threat model as your browser's cookie/credential store — `~/Library/Cookies/Cookies.binarycookies` in Safari, `~/Library/Application Support/Google/Chrome/Default/Cookies` in Chrome — except those stores are usually encrypted at rest. Ours is plaintext + `0600`. Tracked as a future improvement; for now, full-disk encryption (FileVault) is your line of defense for this file, same as your browser cookies.

---

## Transport — what's on the wire

Every network request uses `reqwest` configured with `rustls-tls` (a pure-Rust TLS stack with no OpenSSL anywhere in the dep tree):

```toml
reqwest = { version = "0.12", default-features = false,
            features = ["json", "multipart", "rustls-tls"] }
```

This means:

- **TLS 1.2 and 1.3 only.** No SSLv3/TLS 1.0/1.1 fallback.
- **Certificate verification is on.** There is no "ignore cert errors" flag exposed anywhere in the codebase. Grep `crates/ -e accept_invalid -e danger_accept` returns nothing.
- **HSTS** is implicit — we hardcode the upstream URLs to `https://`. Plain `http://` URLs cannot reach any upstream from inside Smooblue except the OAuth loopback callback (`http://127.0.0.1:<random>/callback`), which never crosses the network.
- **No proxy auto-config.** Smooblue doesn't read `HTTP_PROXY` / system proxies. If you need to route through a corporate proxy, file an issue — today it's unsupported intentionally (avoids accidentally trusting a corporate MITM CA).

**Comparison vs. a browser:** browsers add HSTS preload lists, certificate transparency checks via SCTs, OCSP stapling, and per-tab cookie isolation. We have none of those. Realistically the practical attack surface for what Smooblue does (talks to bsky.network and your PDS, both well-known endpoints) is the same — but a browser's defense-in-depth is broader if you're concerned about novel TLS attacks.

---

## Post-authentication: what protects your content in transit and at rest

A reasonable question is: OAuth handles *authentication* and *authorization* — proving who you are and what scopes you have. So what's actually protecting the post / reply / packet *content* once you're past the sign-in?

Three layers, each doing a different job:

1. **TLS** (covered above) gives you transport confidentiality + integrity. Every byte of every XRPC request and response is encrypted with TLS 1.2+/1.3 between Smooblue and your PDS, and between your PDS and the AppView. A passive network attacker on the wire sees a TLS handshake and then opaque ciphertext.
2. **DPoP-bound requests** give you per-request authenticity. Every XRPC call signs a fresh DPoP proof — a JWS over the HTTP method, full URL, server-issued nonce, and a hash of the access token. Even if an attacker captures one of your requests (e.g. via a leaked log, a memory dump, or a stolen access token), they can't replay it or forge a new one without also stealing the ES256 DPoP private key that lives only in your local session file. Smooblue regenerates the DPoP keypair on every fresh sign-in.
3. **AT Protocol content model** is the honest layer to be transparent about. Bluesky posts (replies, quotes, likes, reposts, profile fields) are **public by protocol design**. They're stored cleartext on your PDS, propagated through the Relay (firehose), and replicated to every AppView and every downstream archival / search / analytics consumer that subscribes to the firehose. No client can encrypt public-post content end-to-end, because it would no longer be public — that's a protocol property, not a client choice. Smooblue is faithful to the protocol here; we don't add and don't withhold anything.

**Practical implications:**

- **For public posts:** transport is encrypted (TLS) and request authenticity is guaranteed (DPoP), but the content itself is intentionally world-readable once it lands at the PDS / Relay. Treat anything you post on Bluesky like anything you post on a public timeline anywhere — it's a public record.
- **For DMs:** the `chat.bsky.*` lexicon is a separate channel, hosted today by Bluesky on a different service from public posts. **Bluesky has NOT yet shipped end-to-end encryption for DMs** — they're TLS-in-transit but readable by Bluesky's chat infrastructure. Smooblue **does not currently support DMs at all** (intentional follow-up — adding DM UI is a meaningful product surface, not a quick add, and we'd want to wait for E2EE if/when it ships). So this is a moot point for Smooblue users today.
- **For drafts you haven't posted:** the compose draft is persisted to a local file (`~/Library/Application Support/ai.Smoo.smooblue/draft.txt`, mode 0600) so your in-progress text survives a crash / restart. Same on-disk threat model as the session file — `0600` perms, no encryption at rest beyond FileVault.

**What Smooblue specifically does NOT do with your content:**

- No analytics on what you post or read. No "5% of users clicked X" pixel; there is no analytics stack.
- No content forwarded to any third party. The only outbound destinations are the endpoints in the egress table below.
- No request bodies cached on disk by Smooblue beyond what the OS HTTP cache decides to do (which `rustls`/`reqwest` does not enable by default for our config).
- No error reporting / crash uploader. A panic logs to your local terminal / the macOS Console; nothing is sent off-machine.

---

## Data egress — who Smooblue talks to

| Destination                                                    | What's sent                                                                  | When                                                                                              |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Your PDS (`bsky.social` for default users; varies for self-hosted) | DPoP-bound access token + the XRPC method + payload                          | Every read or write you initiate (feed, profile, like, post, etc.)                                |
| Bluesky's AppView                                              | Routed *through* your PDS via service-auth proxy                             | Indirectly, as the PDS proxies your reads. Smooblue never contacts the AppView directly.          |
| `cdn.bsky.app` (image CDN)                                     | HTTP GET for thumbnails / avatars / video frames in feeds you scroll         | Per-image, on demand                                                                              |
| `video.bsky.app` (video CDN)                                   | HTTP GET for video segments                                                  | When you tap a video                                                                              |
| `api.github.com`                                               | One GET to `repos/SmooAI/smooblue/releases/latest`                           | Once per launch, to check for updates. Disable with `SMOOBLUE_NO_UPDATE_CHECK=1`.                 |
| `api.smoo.ai` (CRM, **opt-in only**)                           | Your handle, DID, and display name. No tokens, no email, no post content.    | Only if you tick "Stay in touch with Smoo AI" during sign-in. Reversible from Settings → Account. |

That's the whole list. There is no telemetry, no analytics, no error reporting, no crash uploader. `grep -r "POST" crates/` against the source tree returns only Bluesky XRPC calls and (if opted in) the one Smoo CRM call.

You can verify: launch Smooblue with `RUST_LOG=reqwest=debug` and watch the URLs scroll. Or run it under a packet-capturing proxy (mitmproxy with the system cert installed) and inspect every request.

---

## URL hardening

Anywhere Smooblue would shell out to open a URL (clicked link in a post, profile bio link, external embed), the URL passes through `crate::safe_open::open_in_browser` which **rejects every scheme except `http` and `https`**. This was deliberate hardening — see [ADR-002](../Decisions/ADR-002-Safe-Open-Allowlist.md). It prevents:

- `file://` exfiltration (clicking a link can't open local files)
- `mailto:` / `slack://` / `zoom://` / custom protocol handlers (no opening third-party apps)
- `javascript:` (Smooblue is native, but the WebView could theoretically receive these)

A malicious post embedding `<a href="file:///etc/passwd">` is a no-op. You see "couldn't open" in the corner; nothing happens.

---

## What about browser security extensions?

This is a real loss going to a native client and we shouldn't dance around it. Your `uBlock Origin` / `Privacy Badger` / `Decentraleyes` don't apply to Smooblue.

**What we do instead:**

- **No ad / tracker surface.** Bluesky itself has no ads, no tracking pixels, no third-party scripts. The reason you run `uBlock` against bsky.app is mostly habit — there's nothing for it to block on the wire today. Smooblue's network footprint is even narrower than bsky.app because we don't run any analytics / Sentry / Plausible.
- **No JavaScript execution in user content.** Smooblue's WebView renders our own Rust-generated HTML for the deck UI. User-supplied content (post bodies, embeds) is text, never executed as JS. There is no XSS surface for a malicious post to attack.
- **No third-party iframes.** Embeds (videos, images) are rendered with native `<img>` / `<video>` tags pulled from `cdn.bsky.app`. No `<iframe src="...">` from arbitrary domains.

**What you give up:**

- Browser extensions that block third-party trackers won't help (because there are none to block).
- Browser extensions that strip URL trackers (UTM params, fbclid, etc.) won't help. If this matters to you, file an issue — we can add native URL parameter stripping for outbound clicks.
- Browser dev tools (Network tab, Inspect Element) don't exist. To inspect Smooblue's behavior, run it under `mitmproxy` or with `RUST_LOG=reqwest=debug` (see "Data egress" above).

---

## Process model — what Smooblue can do on your machine

Honest gap section.

**Smooblue runs as a normal user process. It is not sandboxed.** That means it has the same permissions as anything else you run as your user:

- It can read any file under your home directory.
- It can make outbound network connections to anywhere.
- It can spawn subprocesses.

In practice, Smooblue only:

- Reads/writes its own config directory (`~/Library/Application Support/ai.Smoo.smooblue/`).
- Reads files **you explicitly pick** through the file-picker (when attaching images / video to a post — uses the system file dialog, no automatic filesystem scan).
- Spawns one subprocess: the OS "open this URL" command, only for `http`/`https` URLs through the allowlist above.
- Makes outbound network connections to the endpoints in the egress table above.

You can verify all three with `lsof -p $(pgrep -x smooblue)` while it's running.

**This is roughly the same threat model as any Mac App Store app that's NOT sandboxed**, or any indie Mac app. Browsers, by contrast, ARE sandboxed (each tab is its own restricted process). On macOS, full app sandboxing is something we plan to add ([tracked in a pearl](https://github.com/SmooAI/smooblue/issues)) — it requires entitlements declarations + Apple Developer ID + notarization, which is a chunk of work tied to the same maturity gate.

---

## What's NOT done (gaps we're honest about)

These are real, tracked, and we'd rather you know about them than discover them later.

1. **macOS adhoc code signing only.** Releases ship with an adhoc signature (`codesign --sign -`), not Apple Developer ID. macOS Sequoia's Gatekeeper refuses to launch adhoc-signed apps on first run with "Apple could not verify Smooblue is free of malware" and offers no GUI "Open Anyway" button. The Homebrew cask works around this with a `postflight` block that runs `xattr -cr` on the installed `.app` so the first launch succeeds — this is a deliberate, documented trust trade ("you tap'd `SmooAI/tools`, you've already extended trust to the org"). Direct .zip downloads from a GitHub release are NOT modified by the cask, so users on that path need to run `xattr -cr /Applications/Smooblue.app` themselves. Apple Developer ID enrollment + notarization is tracked as the proper fix; held until usage justifies the $99/yr + signing-infra overhead.
2. **No macOS App Sandbox.** Plain UNIX-style process. Tracked as a separate hardening task.
3. **Session file is plaintext on disk.** Permissions are `0600` (only your user can read) but the file itself isn't encrypted at rest. FileVault / disk encryption is the line of defense today. Tracked.
4. **No SRI / hash-pinning on auto-updater.** When the optional launchd auto-updater runs, it `git pull`s from the project's `main` branch and rebuilds locally. Trust model = "you trust the GitHub repo." If the GitHub repo is compromised, the auto-updater will pull the compromise. The non-auto path (manual install from a tagged release) inherits the same trust model. We can layer Sigstore signing / signed tags on top — file an issue if you'd use it.
5. **No CSP-equivalent in the WebView.** The Dioxus webview loads our own bundled HTML/JS. No external scripts are loaded, but there's no programmatic CSP header forbidding it. If a future bug allowed user-supplied JS, CSP would be a defense; today the absence is theoretical because we don't render user-supplied scripts.

---

## What you can verify yourself

- **Read the source.** It's MIT-licensed Rust at [github.com/SmooAI/smooblue](https://github.com/SmooAI/smooblue). The OAuth crate is `crates/smooblue-oauth/`; the network egress is `crates/smooblue-atproto/`; URL opening is `crates/smooblue-app/src/safe_open.rs`.
- **Inspect the wire.** Run under `mitmproxy` or `tcpdump`, or with `RUST_LOG=reqwest=debug cargo run`. Every URL Smooblue contacts will show up.
- **Inspect the file:** `cat ~/Library/Application\ Support/ai.Smoo.smooblue/session.json` to see exactly what's stored locally. It's a JSON file you can pretty-print and audit.
- **Audit the dep tree.** `cargo tree -p smooblue-app` lists every transitive dependency. `cargo audit` checks them against the RustSec advisory database.
- **Reproduce a build.** `./scripts/bundle-macos.sh` produces the same `.app` bundle CI produces (modulo timestamps in the binary). No prebuilt blobs in the repo.

---

## Reporting a vulnerability

Email **brent@smoo.ai** or use [GitHub's private security advisories](https://github.com/SmooAI/smooblue/security/advisories). We'll respond within 48 hours and coordinate disclosure timing with you.

No bug bounty program today — Smooblue is a free open-source project. But we will credit you in the changelog and the [github.com/SmooAI](https://github.com/SmooAI) profile if you'd like the recognition.

---

## Related

- [[../Architecture/OAuth-and-Session]] — full OAuth + session flow
- [[../Decisions/ADR-001-Session-File-vs-Keychain]] — why we left the macOS Keychain
- [[../Decisions/ADR-002-Safe-Open-Allowlist]] — URL scheme hardening
- [[../Operations/Auto-Updater]] — the optional auto-updater
