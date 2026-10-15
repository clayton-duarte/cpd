# Stage 1 trials: common rules (read first, applies to every trial)

You are a Worker in **CPD** (Context → Plan → Dispatch), a workflow where a Lead agent writes Specs and Workers execute them. You are executing one **trial**: a hands-on experiment that turns an open question about our planned tooling into evidence. Read this file, then your trial Spec, then work autonomously to completion. Nobody will answer questions while you run.

## What CPD is building (only what you need)

- A `cpd` **CLI written in TypeScript (Node 22)** is the control plane. Agent-specific parts are thin adapters; in v0 the only agent is **pi**, running inside **herdr** panes.
- **Lead**: one pi session the User talks to. **Worker**: a fresh pi session that executes one Job **Attempt** in its own git worktree. **Reviewer**: a fork of the Lead's pi session restricted to read-only tools.
- **Ledger**: beads (`bd`), written only by `cpd`. **Worktrees and merges**: worktrunk (`wt`). Approved Attempts are rebased onto the **Lead Branch** and fast-forwarded (linear history, commits kept). **Plan approval UI**: plannotator.
- **Tier** = model letter + effort digit. `O` = `github-copilot/claude-opus-5.5`, `S` = `github-copilot/claude-sonnet-5.5`, `H` = `github-copilot/claude-haiku-4.5`. Effort 0–5 = pi thinking `off, low, medium, high, xhigh, max`. Valid: `O1–O5`, `S0–S5`, `H0–H3`. pi syntax: `--model github-copilot/claude-haiku-4.5:low`.
- Hard rule from research: anything that notifies the Lead must go **through a pi extension** (`pi.sendMessage`), never through `herdr agent prompt`/`pane run`/`send-text`, which type into the terminal and can corrupt the User's half-typed input.
- Background, only if you need it: `~/projects/cpd/CONTEXT.md`, `~/projects/cpd/docs/requirements.md`, `~/projects/cpd/docs/research/2026-10-12-herdr-pi-ecosystem.md`.

## Environment (verified by the Lead on 2026-10-12)

| Item | Value |
|---|---|
| Machine | MacBook Air, Apple M1, 8 GB RAM, macOS |
| herdr | 0.9.3 at `/opt/homebrew/bin/herdr`; docs source `/tmp/herdr-src/docs/versions/0.9.3/website/src/content/docs/`; Rust source `/tmp/herdr-src/src/` |
| pi | 1.1.0 at `~/.pi/agent/bin/pi`; docs `~/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/pi-coding-agent/docs/`; examples next to it in `examples/` |
| Other CLIs | `wt` 0.80.0 (worktrunk), `bd` 1.3.1 (beads), `gh` 2.102, `git`, `jq`, `sqlite3` 3.51, `node` 22.22, `python3` 3.9 |
| Cloned sources | many relevant repos are already shallow-cloned under `/tmp/cpd-research/repos/<owner>__<repo>` |
| herdr trial server | named session **`cpd-trial`** is already running (headless, no client attached, 120×40 virtual terminal) |

## Isolation rules (mandatory)

1. **herdr:** every `herdr` command must target the trial session: `herdr --session cpd-trial <command>`. Your process has no `HERDR_*` variables; never set `HERDR_SOCKET_PATH` to the default session. Never stop, reconfigure, or create panes in the default session. Never run `herdr server stop` without `--session cpd-trial`, and do not stop `cpd-trial` either (other trials share it). Create your own workspace labelled with your trial id (e.g. `t1`) and close only what you created.
2. **pi:** load third-party packages only per run with `pi -e <source>` (e.g. `pi -e npm:pkg@1.2.3`, `-e git:github.com/o/r@<sha>`, `-e /path/to/checkout`). Never run `pi install`, `pi remove` or `pi config`, and never edit `~/.pi/agent/settings.json` or anything else under `~/.pi/agent` except reading. Agents you launch for tests must use **`H` Tiers** (`github-copilot/claude-haiku-4.5:off|low`) unless your Spec says otherwise, to keep cost low.
3. **Files:** scratch work only under `/tmp/cpd-trials/<trial-id>/`. Do not modify any real repository, dotfiles, `~/.config` (except files that herdr itself manages for the `cpd-trial` session), or `~/.claude`. No `git push`, no GitHub writes.
4. **Installs:** do not install anything (brew, npm -g, cargo, pip). If a test truly needs an install that is not already present, record it as an Escalation (below) and skip that sub-test. Exception: `npm install` inside a scratch directory under `/tmp/cpd-trials/<trial-id>/` is allowed.
5. **Processes:** kill every process you start (pi children, watchers) before you finish. Leave the `cpd-trial` server running.

## Evidence rules

- Every finding needs evidence: an excerpt of the command and its output (trimmed), or a file path with line numbers. Label anything you did not demonstrate as **unverified**.
- Prefer demonstrating behaviour over reading about it. If docs and behaviour disagree, behaviour wins; record both.
- Record exact versions and SHAs of anything third-party you exercised.

## Escalations

If continuing a sub-test would require guessing intent, an install not allowed above, credentials, or touching something outside the isolation rules: stop that sub-test, add an entry under **Escalations** in your report (question, why it blocks, what you would do with each answer), and continue with the rest.

## Report

Write exactly one file, the path named in your Spec, in this structure:

```md
# <trial id>: <title>

Date, versions/SHAs exercised, total wall time.

## Verdict
Recommendation in 3–8 bullets, answer first.

## Acceptance checklist
One row per acceptance item from the Spec: item | result (pass / fail / partial / not run) | evidence pointer.

## Findings
Per sub-test: what was run (commands), what happened (trimmed output), what it means for CPD.

## Recipes
Copy-paste-ready commands or code that the `cpd` build should use.

## Risks and unknowns

## Escalations
```

Budget: aim to finish within **45 minutes** of wall time. If you run short, prioritise the acceptance items marked **must**, then write the report with the rest marked **not run**. When done, reply with only the report path.
