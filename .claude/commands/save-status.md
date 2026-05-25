# Save Status — Project Checkpoint

Snapshot the current state of in-flight work to `docs/Projects/` so any agent (or future you) can pick up cold. Pulls real data from git, GitHub, pearls, and Jira; writes a single dated markdown file linking everything together.

**Default file location:** `docs/Projects/Status-YYYY-MM-DD-HHMM-<slug>.md`
**Default `<slug>`:** auto-derived from the topic argument, or `general` if no topic is passed.

Pass `<args>` as a free-text topic — used in the filename slug and as the H1 title. Examples:
- `/save-status` → `docs/Projects/Status-2026-05-25-1100-general.md`
- `/save-status obs dashboard coverage` → `docs/Projects/Status-2026-05-25-1100-obs-dashboard-coverage.md`
- `/save-status SMOODEV-1270` → `docs/Projects/Status-2026-05-25-1100-smoodev-1270.md`

## Why this skill exists

Long sessions accumulate context that's expensive to rebuild from raw git/gh/jira queries on the next turn. This skill produces a single Obsidian-vault-resident snapshot that captures: what's been merged, what's in flight, what's blocked, what the open pearls/Jira tickets look like, and the **why** behind any non-obvious state (so a fresh agent can act, not just inventory). The file path is stable + dated so a future `/load-status` could pick it up.

## Steps

### 1. Parse the topic

Read `<args>` as a free-text topic. Slugify for the filename:
- Lowercase
- Replace anything non-`[a-z0-9-]` with `-`
- Collapse `-+` to `-`
- Trim leading/trailing `-`
- If empty, use `general`

Compute the timestamp: `YYYY-MM-DD-HHMM` in local time (use `date +%Y-%m-%d-%H%M`).

### 2. Gather state in parallel

Run these in **one** Bash block (independent commands, parallelizable):

```bash
# Working tree
git status --short
git branch --show-current
git log --oneline -15

# Branches + worktrees
git worktree list
git branch --merged main | grep -v "^\* main$" | head -10

# GitHub state
gh pr list --state=open --limit 20
gh pr list --state=merged --limit 10 --json number,title,mergedAt --jq '.[] | "#\(.number) \(.mergedAt) \(.title)"'

# Deploys
gh run list --workflow=deploy-mac.yml --limit 8
```

In parallel, gather pearls + Jira:

```bash
# Pearls
th pearls list --status=in_progress
th pearls ready | head -30
```

Jira (last 24h activity, in-progress, recently transitioned):

```bash
python3 <<'EOF'
import os, base64, urllib.request, json
auth = base64.b64encode(f"{os.environ['JIRA_EMAIL']}:{os.environ['JIRA_API_TOKEN']}".encode()).decode()
# In-progress
req = urllib.request.Request(
    "https://smooai.atlassian.net/rest/api/3/search/jql?jql=project=SMOODEV+AND+status=%22In+Progress%22+ORDER+BY+updated+DESC&maxResults=20&fields=summary,status,priority,updated",
    headers={"Authorization": f"Basic {auth}"},
)
ip = json.loads(urllib.request.urlopen(req).read())
print("IN PROGRESS:")
for i in ip.get("issues", []):
    f = i["fields"]
    print(f"  {i['key']} [{f.get('priority',{}).get('name','-')}] {f['summary']}")
# Recently Done (last 48h)
req2 = urllib.request.Request(
    "https://smooai.atlassian.net/rest/api/3/search/jql?jql=project=SMOODEV+AND+status=Done+AND+resolved+%3E=+-2d+ORDER+BY+resolved+DESC&maxResults=15&fields=summary,resolutiondate",
    headers={"Authorization": f"Basic {auth}"},
)
rd = json.loads(urllib.request.urlopen(req2).read())
print()
print("RECENTLY DONE (last 48h):")
for i in rd.get("issues", []):
    f = i["fields"]
    print(f"  {i['key']} ({f.get('resolutiondate','?')[:10]}) {f['summary']}")
EOF
```

### 3. Capture in-flight changes per worktree

For each non-main worktree, show:
- branch name
- commits ahead of `origin/main` (via `git -C <worktree> rev-list --count origin/main..HEAD`)
- unstaged file count
- last commit message + age

Skip worktrees with **0 commits ahead AND no local changes** (already-merged or empty).

### 4. Identify what's blocking what

For each in-progress pearl / open PR / failed deploy:
- Who owns it (pearl assignee, PR author, recent committers)
- What it depends on (parent pearl, Jira ticket, in-flight PR)
- Why it's stuck (deploy block, review block, awaiting design, etc.) — **infer from the data**, don't just list

This is the load-bearing section. A future agent should be able to read "X blocks Y because Z" and act, not re-derive.

### 5. Write the file

Create `docs/Projects/Status-YYYY-MM-DD-HHMM-<slug>.md` (create `docs/Projects/` if missing). Use this skeleton:

```markdown
# {Title} — {YYYY-MM-DD HH:MM}

#status #checkpoint #{topic-tags}

> One-paragraph TL;DR — what someone reading this in 6 months needs to know first. Include the most important number / fact and the most important blocker (if any).

## What shipped this session

| PR | Ticket | What | When |
|---|---|---|---|
| #NNNN | SMOODEV-XXX | One-line summary | HH:MMZ |

(Only PRs **merged today**. If session is multi-day, scope to the relevant range.)

## In flight (merged, not deployed)

| Change | PR | Deploy state | Notes |
|---|---|---|---|

(Things on `main` that aren't in prod yet — usually because a deploy is failing or hasn't been fired. Include the deploy run id + why it failed if applicable.)

## Active worktrees

| Worktree | Branch | Ahead | Diff | Last commit |
|---|---|---|---|---|

(Only worktrees with commits ahead OR local changes. Owner/agent if known.)

## Open work — by priority

### P0 / blocker

- **SMOODEV-XXXX** — title — blocking <what> — owned by <who>
  - Why stuck: …
  - Next action: …

### P1

…

### P2 (sample, top 5)

…

## Blockers / cross-cutting

(Things that affect multiple workstreams — deploy pipeline broken, EKS not stable, external dep down.)

## How to verify when the deploy block clears

(Concrete commands or URL checks to confirm specific things landed. Saves the next agent from re-deriving.)

## Related artifacts

- [[Other-Checkpoint-Name]] — if there's a sibling status doc
- [[Adding-Observability-To-A-Service]] — relevant runbooks
- Pearl `th-xxxxxx` — relevant in-progress work
- SMOODEV-XXXX — relevant Jira epic

## Footnote — generation context

- Session range: <approx start>–<now>
- Files modified this session: <list> (or "see git log")
- Pearls touched: <list>
- Generated by `/save-status` on {YYYY-MM-DD HH:MM}
```

### 6. Print the artifact path

After writing, print one line: `Saved → docs/Projects/Status-YYYY-MM-DD-HHMM-<slug>.md`

Do NOT recap the contents — the user can `cat` the file. The whole point of this skill is offload, not duplicate output.

## Constraints

- **Read-only on external systems.** This skill should never `git push`, `gh pr create`, `gh workflow run`, `gh pr merge`, or `th pearls close`. Pure introspection + file write.
- **One file per invocation.** Don't update older status files; each invocation is a fresh snapshot. (Use git history if you need to diff snapshots.)
- **Idempotent within a minute.** If the same timestamp slug already exists, overwrite — running twice in a row should produce one consistent file, not two.
- **No secrets in the file.** Anything coming from `smooai-config get`, env vars, or DB queries must be redacted/summarized. Connection strings, tokens, API keys — never write them.
- **Anchor `git log` ranges to a base.** Default to `git log origin/main..HEAD` for worktree-local diffs and `git log --since="24 hours ago" main` for "what shipped today." Avoid unbounded `git log` that dumps everything.

## Anti-patterns

- **Inventorying instead of synthesizing.** Don't write "There are 5 open PRs: #X, #Y, …" without saying which ones matter and which are noise. The whole skill is bias-toward-the-relevant.
- **Hallucinating ticket ids.** Every SMOODEV-NNNN / th-XXXXXX referenced must be a real result of the queries in step 2. If you don't see it in the gathered data, don't write it.
- **Duplicating CLAUDE.md.** Skip standing conventions ("deploys go through smoo-hub", "use worktrees", etc.) — those live in CLAUDE.md. Status checkpoints document the *deviations* from steady state.
- **Stale data.** If a step's query failed or returned nothing, say so explicitly in the file ("Jira query returned 0 in-progress — likely transient API issue, not real absence of work"). Don't paper over.
