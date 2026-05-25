# Branch Protection

#operations

What `main` enforces and how to update it.

---

## Current rules (set via the GitHub REST API at 1.0 cut)

| Rule | Setting |
| --- | --- |
| Required status checks | `cargo test (ubuntu-latest)` + `cargo test (macos-latest)` |
| Strict mode | On — branch must be up-to-date with `main` before merge |
| Required PR reviews | 0 approving reviewers (we're a small team; CI is the gate) |
| Dismiss stale reviews | On — push after approval re-requests review |
| Required linear history | On — merge commits not allowed; rebase + fast-forward only |
| Force pushes | Disabled |
| Branch deletions | Disabled |
| Required conversation resolution | On |
| Enforce on admins | Off — admins bypass for emergencies |

---

## How to inspect

```bash
gh api repos/SmooAI/smooblue/branches/main/protection | jq
```

---

## How to update

JSON body via the API, not the CLI flags (the CLI mangles types — `strict=true` becomes the string `"true"`):

```bash
cat > /tmp/branch-protection.json <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": ["cargo test (ubuntu-latest)", "cargo test (macos-latest)"]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "required_approving_review_count": 0
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_linear_history": true,
  "required_conversation_resolution": true,
  "block_creations": false
}
EOF
gh api -X PUT repos/SmooAI/smooblue/branches/main/protection --input /tmp/branch-protection.json
```

---

## When to relax

- **Required reviews → 1** when the team is more than two people and we're shipping less aggressively
- **Enforce on admins → on** when we're past the "hotfix bypass" phase of the project
- **Restrict who can push** — empty today; tighten if we add release engineers

---

## Related

- [[../Engineering/Engineering-Guide#Commit conventions]]
