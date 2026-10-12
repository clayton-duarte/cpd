# Research worker brief (shared by all workers)

You are a research Worker for the CPD project. Read this file completely, then read your cluster file (named in your prompt). Work autonomously; do not ask questions. If something cannot be determined, write "unverified" or "unknown" and move on.

## Background (read these first, quickly)

- `~/projects/cpd/CONTEXT.md`: vocabulary.
- `~/projects/cpd/docs/requirements.md`: what CPD needs. Section 5 lists candidates found so far.
- herdr 0.9.3 docs source (plugin system, CLI, socket API): `/tmp/herdr-src/docs/versions/0.9.3/website/src/content/docs/` (`plugins.mdx`, `socket-api.mdx`, `cli-reference.mdx`, `agent-automation.mdx`).
- pi docs (extension system): `~/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/pi-coding-agent/docs/` (`extensions.md`, `packages.md`, `sdk.md`, `rpc.md`).
- Ecosystem index of every herdr plugin (1,685 manifests): `/tmp/herdr-index.json` (keys: `plugins[].fullName, stars, pushedAt, description, topics, manifests[]`).

## CPD needs (use these IDs in your report)

| ID | Need |
|---|---|
| N1 | Spawn a pi Worker in a herdr pane/worktree with model + thinking level chosen per spawn |
| N2 | Non-blocking completion notification back to the Lead that does not clobber the User's half-typed input |
| N3 | Escalation: Worker asks its parent a question and waits for the answer |
| N4 | Reviewer as a fork of the Lead session, restricted to read-only tools |
| N5 | Job graph: dependencies, ready set, states, append-only history, queryable |
| N6 | Concurrency cap per machine |
| N7 | Timeout per Attempt |
| N8 | Retry ladder: re-spawn a fresh session with a different model/effort plus reviewer feedback |
| N9 | Worktree per Attempt from the Lead Branch HEAD, plus per-repo setup (env files, deps) |
| N10 | Merge an approved Attempt into the Lead Branch (keep commits), serialised merge queue |
| N11 | Run the mechanical Gate (repo `verify` script: tests, typecheck, lint) |
| N12 | PR feedback loop: failing CI and reviewer-bot comments become new Jobs |
| N13 | Metrics per Attempt: duration, turns, tokens/cost, outcome |
| N14 | Structured questions to the User, plan approval UI |
| N15 | Visibility: sidebar status, DAG/board view (nice-to-have; User only talks to the Lead) |
| N16 | Worker safety/sandboxing (post-MVP) |
| N17 | Agent-agnostic path (Claude Code, Hermes later) |
| N18 | Cheap auxiliary agents (Haiku) for small tasks |
| N19 | Building blocks for writing our own herdr plugin or pi extension (patterns, libraries, scaffolds worth copying) |

## Method

1. Metadata: `gh api repos/<owner>/<repo>` (stars, pushed_at, created_at, license, open_issues, default_branch, archived); `gh api repos/<o>/<r>/releases --jq '.[0].tag_name'`; npm monthly downloads: `curl -s https://api.npmjs.org/downloads/point/last-month/<pkg>`.
2. Shallow clone into `/tmp/cpd-research/repos/<owner>__<repo>` (`git clone --depth 1`). Record the commit SHA you read.
3. Read the README, the manifest (`herdr-plugin.toml`, or `package.json` `pi` field / extension entry), and **the source that implements the claims that matter for CPD**. Do not trust the README alone: confirm the mechanism in code and cite `path:line`.
4. Identify exactly which herdr primitives (CLI subcommands, socket methods, subscribed events, manifest entry types: actions/events/panes/startup/link_handlers) and which pi APIs (registered tools, commands, events hooked, `sendUserMessage`, session fork, custom UI) it uses.
5. Check maturity: tests present, CI, release cadence, issue activity, bus factor, min herdr / pi version.
6. Judge fit against the needs table and give a verdict.

## Rules

- Primary sources only: code, manifests, official docs, GitHub/npm API data. Label inference ("guess:", "unverified").
- Read-only. Do NOT install plugins or packages, do NOT run `herdr plugin install/link`, do NOT run `pi install`, do NOT modify anything under `~/.config`, `~/.pi`, `~/.claude` or any repo except your report file. Do NOT run any `herdr` command that mutates state. Cloning to `/tmp/cpd-research/repos/` is fine.
- Keep each item concise: depth on mechanism and fit, not on marketing.
- Budget: aim to finish within ~45 minutes. If you run short, finish the items that matter most for CPD (N1–N12) first and list the rest as "not reviewed".

## Output

Write exactly one file: the report path named in your cluster file. Structure:

```md
# <Cluster name>: research report

Read at: <date>. Items reviewed: N. Items not reviewed: list.

## Summary
- Best candidate per need (table: Need | Best item | Coverage full/partial | Notes)
- Overlaps and conflicts between items (e.g. same tool names, both hook the same event)
- Recommendation for CPD v0 (reuse / fork / borrow / skip), 5–10 bullets

## Items
### <owner/repo> (npm: <pkg> if any)
- Meta: ★stars · pushed YYYY-MM-DD · created YYYY-MM-DD · license · version/tag · downloads/mo · SHA read
- What: one sentence.
- Mechanism: herdr/pi primitives used, with path:line evidence.
- Maturity: tests/CI/releases/issues/compat.
- CPD fit: N-ids with full/partial and how.
- Gaps/risks:
- Verdict: REUSE | FORK | BORROW | SKIP: one-line reason.

## Additional finds
Anything relevant you discovered that was not on your list (with the same item format, shorter).
```

When done, reply with only the report path.
