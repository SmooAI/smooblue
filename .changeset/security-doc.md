---
"smooblue": patch
---

Add a comprehensive security writeup at `docs/Security/Security.md` — auth model (PAR + PKCE + DPoP, why this is stronger than app passwords), transport (rustls TLS, no insecure fallbacks), the complete data egress table, URL hardening, what browser security extensions buy you vs don't, the process / sandboxing model, and an honest "what's NOT done" section (adhoc signing, no App Sandbox, plaintext session file, no SRI on auto-updater). Linked from the README and from Settings → About so users can find it in-app.
