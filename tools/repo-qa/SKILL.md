---
name: repo-qa
description: Run evidence-based repository QA, create researched GitHub issues, prioritize the backlog, implement reviewed fixes, and preserve budgets and handoffs across agent sessions. Use when the User explicitly starts or resumes repository QA, or requests adoption of this QA Protocol in another repository.
compatibility: Requires Git, Node.js 22 or newer, authenticated GitHub CLI, and repository-specific development tools. Works as ordinary Markdown without automatic skill discovery.
---

# Repository QA

Read [the QA Protocol](references/PROTOCOL.md) and the target repository's QA
profile and applicable agent/contributor instructions. User instructions take
precedence; document decisions and unresolved contradictions.

For first adoption, follow [README.md](README.md). Copying or discovering this
skill does not start a run, grant access, authorize a quota reset, or enable a
scheduler. Do not assume that instructions in another repository authorize work
here. Never copy CPD host names, absolute paths, credentials or install commands.

For a started/resumed run:

1. Recover the User's scope, run descriptor and canonical handoff from the repo
   and GitHub, not remembered chat. A descriptor stores intent, not counters.
2. Refresh the live weekly meter when configured. Read current budget with the configured script and the same `--run` file and
   authorized `QA_BATCH`, if any. Check open PR feedback/CI before new work.
3. Ground the repo and prove candidates through real callers. Find two defensible
   issues or explore for 30 minutes, then prioritize the whole open backlog.
4. Research one actionable fix, work on a branch, run scoped checks, commit and
   open a PR. Issues hold rationale/research; PRs hold implementation/validation.
   User reviews and merges. No autonomous merge, deployment or live-host changes.
5. Save a [handoff](assets/handoff.md), recheck counts, and repeat only within
   the User's configured count/weekly limits. Never manufacture issues to fill a number.
   Do not start another run to evade an exhausted one.

Use `scripts/qa.mjs` for creation; commands are in the README. The script enforces
countable limits for one serialized operator per budget group. It does not judge
finding quality or authenticate a prose authorization reference. Independent
agents must use disjoint repository groups; overlapping repositories need one
shared configuration and serialized creator. Do not invent a distributed lock.

If a decision needs the User, record the question and alternatives in the issue,
apply `needs-human-input`, and continue independent authorized work. Preserve
all attribution/batch/run markers when editing bodies. A failed write may have
succeeded remotely: inspect before retrying.
