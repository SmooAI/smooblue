# Resume Status — smooblue Pickup

Find the most recent `docs/Projects/Status-*.md` checkpoint(s) written by `/save-status`, present them, and execute on what the user wants to pick back up.

Symmetric counterpart to `/save-status` in this repo (smooblue — Bluesky AT Protocol client).

## smooblue-specific context this skill must know

- **Pearls + Jira both in play.** Pearls (`.smooth/dolt/pearls/`) + Jira SMOODEV project. Most smooblue pearls are tagged `SMOODEV-1163` (or a sub-pearl thereof).
- **Tauri/Rust + TypeScript dual stack.** A checkpoint may reference Rust crate changes or TS/React changes. Verify both.
- **OAuth + DPoP nonce concerns** show up frequently in checkpoints — verify the nonce-handling state if relevant.
- **Releases are macOS .app bundles** built via signed GitHub Actions. "In flight" can mean "merged but not yet in a signed bundle."

## When to use this

- Start of session after `/save-status`
- Resuming after multi-day pause on the Bluesky client work
- Picking up a specific SMOODEV-1163-* sub-ticket

## Steps

### 1. Find recent checkpoints

```bash
ls -t docs/Projects/Status-*.md 2>/dev/null | head -10
```

Empty → say so + suggest `/save-status` first.

### 2. Present options

`AskUserQuestion`: 5 most recent, labelled `YYYY-MM-DD HH:MM — <topic-slug>`. Skip if only one.

### 3. Read the chosen checkpoint

Internalize TL;DR, in-flight items, active worktrees, open pearls (SMOODEV-1163-*), Jira state, blockers, release-bundle state.

### 4. Reality-check before asking

```bash
# Activity since checkpoint
git log --since="<date>" --oneline main 2>/dev/null | head -10

# Worktrees
git worktree list 2>/dev/null

# Pearls referenced
th pearls list --status=open 2>&1 | head -20

# Jira state for SMOODEV-1163-* sub-tickets in the checkpoint
python3 <<PY
import os, base64, urllib.request, json
auth = base64.b64encode(f"{os.environ['JIRA_EMAIL']}:{os.environ['JIRA_API_TOKEN']}".encode()).decode()
# substitute referenced keys
for key in ["SMOODEV-XXXX"]:
    req = urllib.request.Request(f"https://smooai.atlassian.net/rest/api/3/issue/{key}?fields=status",
                                  headers={"Authorization": f"Basic {auth}"})
    j = json.loads(urllib.request.urlopen(req).read())
    print(key, j["fields"]["status"]["name"])
PY

# Recent builds
gh run list --workflow=build.yml --limit 5 2>/dev/null
```

Surface drift — released bundle since checkpoint? Closed pearl? Updated Jira?

### 5. Ask what to pick back up

`AskUserQuestion`. Tailor:

**Multiple open SMOODEV-1163-* sub-tickets:**
- Header: "Pick up where?"
- Text: "N open smooblue threads. Which?"
- Options: per sub-ticket with next-action description

**One active thread:**
- Header: "Confirm resume"
- Text: "Resume <thread>? (Next: <action>)"
- Options: ["Yes", "Pick another open ticket", "Off-script"]

**OAuth/DPoP issue mid-investigation:**
- Header: "DPoP debug resume"
- Text: "Checkpoint mid-investigation of <issue>. Continue or restart fresh?"
- Options: ["Continue from checkpoint trail", "Re-test from scratch", "Pivot to another bug"]

**Release shipped:**
- Header: "Bundle shipped — next?"
- Text: "Release <vN.N> bundle landed. What's next?"
- Options: derived from open ready-pearls

Always include "off-script."

### 6. Act on the user's choice

Execute. `th pearls update`, Jira transition, worktree checkout, `pnpm dev` or `cargo run` as needed.

Print one summary line. Then begin.

## Constraints

- **Read-only on checkpoint.**
- **One AskUserQuestion batch.**
- **Don't recreate the checkpoint.**
- **Don't hallucinate IDs.**
- **Tauri/Rust + TS discipline applies.** If resuming requires both crate + TS changes, do both.

## Anti-patterns

- **Dumping the whole checkpoint at the user.**
- **Ignoring release-bundle state** — a checkpoint that says "ready to ship" but the signed bundle isn't built yet is different from "shipped."
- **Treating OAuth nonces casually.** If the checkpoint mid-debugs an auth flow, replay the exact reproduction steps; don't assume "it was working when I saved."
