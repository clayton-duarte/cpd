# CPD v0: requirements

Status: **Context complete for the v0 stack** (decisions 2026-10-12). Next: Plan stage 1 (trials) in [plan.md](plan.md), pending approval. Vocabulary: [CONTEXT.md](../CONTEXT.md). Decisions: [adr/](adr/). Background on herdr: `~/notes/herdr-usage-and-workflow.md`.

## 1. Scope and principles

- One Workflow = one ticket = one repo = one Pull Request.
- Agent runtime for v0: **pi only**, inside **herdr**. Claude Code and Hermes come later.
- **Adopt, don't build.** Use herdr built-ins, existing herdr plugins and pi packages first. Where a chosen plugin already defines a behaviour (worktree placement, cleanup, notification, config), adopt its behaviour instead of specifying our own. Custom code is last and minimal.
- Don't duplicate configuration that herdr, pi or a plugin already owns.
- **Control plane is the `cpd` CLI** (ADR-0005). Everything deterministic (Ledger writes, scheduling, merge, Gate, metrics, PR polling) is a `cpd` command any agent can call. Agent-specific parts (v0: pi) are thin adapters. pi-only is a v0 compromise to keep focus, not the target.
- **Dependency policy.** v0 may depend on young or unlicensed projects. We prefer software we can rely on, and replace weak dependencies with `cpd`-owned code over time.

## 2. Phases

### Context
- Every Workflow starts in a new Lead session at Tier `O3`.
- The User provides material (docs, repos, links); the Lead grounds itself, delegating research to Workers.
- The User states Requirements; the Lead interviews (`grilling` skill) until unknowns are minimised, contradictions resolved and remaining assumptions written down and accepted.

### Plan
- The Lead drafts Jobs, Dependencies and Tiers. Jobs are as small as reasonably possible to maximise parallelism.
- Jobs that would touch the same files are ordered by a Dependency. The Lead decides how to split.
- Every Spec is complete and guess-free, and contains only what its Worker needs.
- **Job sizing budget:** a Job should fit comfortably in one Attempt. Reference from the User's Hermes runs: **~13 min and ~150 turns per job on average**, including worktree setup and dependency install. The default timeout is 30 min.
- The User approves the Plan. Until then everything is a draft.

### Dispatch
- On approval: the Lead Branch is created from `origin/<default>`; all Jobs become Locked.
- A Job is Ready when all its Dependencies are Merged. Ready Jobs start up to the machine's concurrency cap.
- Each Attempt runs in a **fresh** Worker session on its own worktree, branched from the Lead Branch HEAD at start time (so it contains its Dependencies' commits).
- Attempt lifecycle: work → commit → Gate passes → Reviewer verdict → approved: rebase onto the Lead Branch and fast-forward it (commits kept, linear history; ADR-0006) → Gate on Lead Branch → **Merged**.
- **Reviewer:** a fork of the Lead session with read-only tools (ADR-0002).
- **Retry ladder:** each rejection starts a new fresh Attempt with the Spec plus the Reviewer's feedback, **effort +1**. If effort is already at the model's maximum, **model +1** (H → S → O) at the lowest valid effort ≥ the current one. Default cap: **2 retries** (3 Attempts), configurable. Beyond the cap the Job fails and Escalates.
- **Timeout:** default **30 min** per Attempt, configurable; on timeout the Attempt fails and Escalates to the Lead.
- Locked means: Jobs can only be re-attempted, cancelled, or have new Jobs appended. Nothing is edited or deleted; history is kept for retrospectives and post-mortems (ADR-0003).
- Escalation goes one level at a time: Worker → Lead → User. The Lead resolves what it can first.
- The Lead stays free for the User: it delegates coding, research and review; it writes Specs and orchestrates.

### Pull Request
- When all Jobs are Merged: the Lead pushes and opens the PR.
- The User's reviewer bot reviews it. Bot feedback and failing CI become **new appended Jobs**.
- Trunk-based development: the Lead Branch is short-lived and linear. It is not kept up to date with `<default>` during the run; once the PR is open, the Lead rebases it onto `<default>` only if there are conflicts to solve.
- The Workflow is done when the **PR is merged**.

## 3. Tiers

Format `<model><effort>`. Models are pinned to the highest available version.

| Letter | Model (pi, provider `github-copilot`) |
|---|---|
| `O` | `claude-opus-5.5` |
| `S` | `claude-sonnet-5.5` |
| `H` | `claude-haiku-4.5` |

Effort maps to pi `--thinking` (`--model <provider>/<id>:<level>`). herdr passes agent arguments unchanged, so the limit is what pi accepts per model. Pi **silently clamps** unsupported levels, so CPD must reject invalid Tiers itself.

| Effort | 0 | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|---|
| pi level | off | low | medium | high | xhigh | max |
| `O` | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `S` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `H` | ✓ | ✓ | ✓ | ✓ | ✗ | ✗ |

Source: pi 1.1.0 `getSupportedThinkingLevels` plus `thinkingLevelMap` in `~/.pi/agent/models-store.json`. This can change on `pi update`, so derive the table from pi's catalog; don't maintain it by hand. Valid Tiers: `O1–O5`, `S0–S5`, `H0–H3`. Defaults: Lead `O3`; examples: coding `S1`, PR review `S4`, docs `H2`.

## 4. Metrics (for tuning defaults)

Record per Attempt: Job, Tier, start/end time, turns, tokens/cost if the runtime exposes them, outcome, retry cause. The goal is to tune the defaults (timeout, retry cap, concurrency, Tier choices, Job size) from real data instead of guesses.

## 5. Stack

Evidence: [research/2026-10-12-herdr-pi-ecosystem.md](research/2026-10-12-herdr-pi-ecosystem.md). Decided by the User on 2026-10-12:

| Concern | Decision | Status |
|---|---|---|
| Control plane | `cpd` CLI; thin pi extension as the v0 adapter (ADR-0005) | decided |
| Ledger | beads (`bd`), written only through `cpd` (ADR-0004) | decided; trial t2 fixes the mapping |
| Worktrees, setup, merge | worktrunk (`wt`): create from the Lead Branch, blocking setup hooks, rebase + fast-forward merge (ADR-0006) | decided for v0; trial t3 |
| Worker spawning | chosen by trial t1 | open |
| Plan approval UI | plannotator pi extension, event API only (plan mode off) | decided; trial t4 |
| Structured questions to the User, Escalation transport | proposed `rpiv-ask-user-question`, `pi-intercom` | trial t5 |

Constraints established by the research:
- Anything that notifies the Lead goes through a pi extension (`pi.sendMessage`), never through `herdr agent prompt`, which types into the terminal and clobbers the User's draft.
- If a pi spawner package is used, install exactly one from the `subagent` family (identical tool names; first loaded wins silently).
- A Worker must not start before its worktree setup has finished.
- The Ledger lives outside the product repo (`BEADS_DIR`) and must not touch the repo (`AGENTS.md`, git config).

## 6. Configuration

- Lives in `~/.cpd/`, synced with chezmoi and the dotfiles sync script like the rest of the setup.
- Holds only what no tool owns: Tier map (if not derivable), retry cap, timeout, per-machine concurrency cap. Defaults: MacBook Air (M1, 8 GB) **3** (guess), Mac Pro **8** (tested by the User).
- Per-repo setup and Gate commands live in worktrunk's project config (`.config/wt.toml`: `pre-start` for setup, `pre-merge` for `verify`), pending trial t3.

## 7. Alternatives recorded

- Lead Branch in its own worktree; merges happen there, never in the User's main checkout (adopt whatever worktrunk supports; trial t3).
- After a merge, remove the Job's worktree and branch; keep failed/cancelled Attempts' worktrees until the Workflow ends (trial t3).
- Thin SQLite Ledger with append-only triggers, mu/herdr-dagr schema (superseded by beads, ADR-0004).
- `--no-ff` merge commits instead of rebase + fast-forward (rejected, ADR-0006).

## 8. Post-MVP backlog

- Multi-repo Workflows (one Lead Branch and PR per repo, cross-repo Dependencies).
- Epics: a shared context above several Workflows/PRs.
- Claude Code and Hermes Workers (consider `herdr-projects` / `pi-interactive-subagents`).
- Worker sandboxing (herdr plugins / pi extensions; Workers have full shell in v0).
- Collapsing nested worker workspaces in herdr.
- Resource classes for heavy Jobs (e.g. iOS simulators).
- Live DAG view (e.g. `herdr-dagr`, `herdr-beads` board).
