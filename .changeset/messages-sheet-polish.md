---
"smooblue": minor
---

**MessagesSheet visual polish.** v1.12.0 shipped the inline DM thread with placeholder styling — bubbles used CSS vars that don't exist in Smooblue's theme (`--color-surface-alt`, `--color-brand`, etc.) so they rendered transparent and you could only tell a message was from you by right-alignment. Real implementation:

- **Convo header** — partner's avatar + display name + `@handle` at the top of the sheet (fetched once per open via `chat_get_convo`, member-list filtered to the non-self party).
- **Bubble colors that actually show**: self bubbles use smoo-orange brand, other bubbles use `--card` (subtle elevation against the body's `--background`). Self bubbles get a directional tail (bottom-right corner reduced); other bubbles get the same on bottom-left.
- **Message grouping**: consecutive messages from the same sender within 5 min stack tightly together; mid-group bubbles keep full rounded corners (no tail). Mirrors iMessage/Slack/Telegram convention.
- **Avatar on first-of-group only** for the other party — multi-message bursts don't repeat the avatar 5 times.
- **Time chip on last-of-group only**, side-aligned (right for self, left for other) and padded past the avatar slot so it lines up under the bubble.
- **Compose strip + header** on `--card` background, body on `--background`, so the chrome reads as separate from the conversation surface.

CSS-only changes use Smooblue's actual theme vars (`--card`, `--background`, `--border`, `--foreground`, `--muted`, `--muted-foreground`, `--color-smooai-orange`, `--color-smooai-red`) instead of the invented ones in the v1.12.0 shipped version.
