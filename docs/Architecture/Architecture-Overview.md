# Architecture Overview

#architecture #moc

How Smooblue is put together at the crate level, and what data flows where.

---

## Crate map

```
smooblue-app  ─┬──>  smooblue-atproto  ─┐
               ├──>  smooblue-oauth ────┼─>  Bluesky AppView / PDS
               ├──>  smooblue-crm  ─────┘   (HTTPS + DPoP-bound tokens)
               └──>  smooblue-theme
```

| Crate | Role |
| --- | --- |
| `smooblue-app` | Dioxus desktop binary. Components, state, persistence, OS-level glue (NSApp activation, drag-drop, file dialogs, OCR via Apple Vision). |
| `smooblue-atproto` | XRPC client. One method per endpoint (`get_timeline`, `create_post_full`, `get_feed`, ...). Serde models for the parts of the lexicon we render. |
| `smooblue-oauth` | ATproto OAuth flow — PAR, PKCE, DPoP key generation + proof signing, loopback listener for the callback. |
| `smooblue-crm` | Opt-in Smoo CRM sync. Compile-time `Consent` token (impossible to fire without consent). |
| `smooblue-theme` | CSS tokens + the canonical `STYLES` constant served into the Dioxus webview. |

---

## Columns

Each column in the deck is one `Column` component with its own:
- `data: Signal<ColumnData>` — current page (Posts / Notifications / Suggestions)
- `error: Signal<Option<String>>`
- `loading: Signal<bool>`
- A long-running `use_future` that polls the right endpoint at the column-kind's poll interval (e.g. Notifications: 25s, custom Feed: 25s)

Click → focus model:
- Click a post body → `ThreadFocus::Some(uri)` opens the thread sheet
- Click an avatar / handle → `ProfileFocus::Some(did)` opens the profile sheet
- Click a notification card → opens the relevant thread (or the actor's profile for follows)

See `crates/smooblue-app/src/components/column.rs` for the poll loop and `fetch_once`.

---

## OAuth + session

Detailed in [[OAuth-and-Session]]. Quick version:

1. User types handle → `OAuthClient::sign_in(handle, browser_opener)` resolves the PDS, hits PAR with PKCE + DPoP proof, launches the system browser at the authorize URL.
2. Browser redirects to `http://127.0.0.1:<ephemeral>/callback` which the loopback listener catches.
3. Exchange code for a DPoP-bound access + refresh token, store in `~/Library/Application Support/ai.Smoo.smooblue/session.json` (mode 0600).
4. Every XRPC call goes through `fresh_client()` which transparently refreshes the access token if expiring (rotated refresh token persisted to both legacy and per-DID slots).

Why file-based instead of Keychain: [[Decisions/ADR-001-Session-File-vs-Keychain]].

---

## Multi-account

`accounts.json` at the same path as `session.json` is the index:
```json
{
  "active_did": "did:plc:abc",
  "accounts": [
    { "did": "did:plc:abc", "handle": "alice.bsky.social" },
    { "did": "did:plc:def", "handle": "alice-alt.bsky.social" }
  ]
}
```

Each account has its own `session-<did>.json` (sanitized for FS). Switching = load + swap the active Session signal + write `active_did` back. The legacy single-slot `session.json` is also written on every refresh so external tools that don't speak the keyed scheme still see something coherent.

---

## State management

Dioxus signals only. No external store. Signal categories:

- **Persistent across launches**: `Option<Session>`, `Vec<ColumnSpec>`, `ThemeMode`, `Accounts`
- **Per-session UI focus**: `ProfileFocus`, `ThreadFocus`, `EngagementFocus`, `ReportFocus`, `ProfileEditOpen`, `FocusedItem`, `KeyboardHelp`, `PendingChord`
- **Drag state**: `ColumnDrag`
- **Tick**: 1Hz counter that drives "11s → 12s" timestamps (only the `TimeAgo` component subscribes — see [[#Performance footguns]])
- **OptimisticMap**: optimistic like/repost state so the UI doesn't wait for the XRPC round-trip to render the heart fill

Bootstrapped once in `state::use_bootstrap()` (idempotent — safe to call every render).

---

## Reactive use_resource gotcha

`use_resource` only re-runs when **signals read inside the closure** change. If you do `let key = focus.read().0.clone()` at the outer render and then capture `key` by value, the resource sees the value frozen at first mount. **Always read the focus signal inside the async closure.** Several sheets (Profile, Thread, SavedFeeds) hit this bug at 1.0; tests aren't easy here because Dioxus needs a runtime, so the regression vector is "test it interactively after touching any sheet's resource."

---

## Performance footguns

- **Timestamp re-renders**: every post in a feed showing "11s ago" used to subscribe to the global 1Hz Tick signal. 500 posts × 1Hz = 100% CPU. Fix: extract the tiny `icons::TimeAgo` component, only it subscribes to `Tick`; the parent PostCard does not.
- **Lossy image fan-out**: feeds with many images stutter while the network fills. Mitigation: every `<img>` has `loading="lazy" decoding="async"`. The webview throttles automatically.
- **Notifications subject hydration**: the Notifications poll caches resolved subject posts in an LRU bounded at 500 entries (across polls). Avoids re-fetching the same 30 URIs every cycle.

---

## Render pipeline

- Dioxus desktop → wry → WKWebView (on macOS)
- One window, no iframes, no Electron
- HLS playback uses the native `<video>` tag — WKWebView decodes m3u8 directly

---

## Related

- [[OAuth-and-Session]] — deep dive on auth
- [[Decisions/ADR-Index]] — non-obvious architectural choices
- [[../Engineering/Adding-a-Column-Type]] — adding new column kinds
- [[../Engineering/Adding-an-XRPC-Endpoint]] — adding new bsky endpoints
