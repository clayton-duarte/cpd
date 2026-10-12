# CPD v0: plan

Status: **Stage 1 approved by the User on 2026-10-12; Jobs Locked; dispatching.** Stage 2 is an outline only; it is planned and approved separately after Stage 1 reports land. Requirements: [requirements.md](requirements.md). Format: `id [Tier] needs → deliverable`.

## Why two stages

The v0 stack is decided except for Worker spawning, and every chosen dependency is unverified at runtime (nothing has been installed or run yet). Specifying build Jobs now would bake guesses into Locked Specs. Stage 1 is a short Workflow of trials that turns those unknowns into facts. Stage 2 then specifies the build from evidence.

## How Stage 1 runs (bootstrap)

CPD does not exist yet, so the Lead runs Stage 1 by hand.
- **Workers.** One fresh headless pi Worker per Job, at the Job's Tier, each with a self-contained Spec in `docs/trials/specs/`, writing its report to `docs/trials/<job>.md`.
- **Review.** The Lead checks every report against its acceptance criteria. A rejected report gets one more Attempt one Tier step higher.
- **Ledger.** This file plus git history (beads is not adopted yet).
- **Concurrency.** This machine is the MacBook Air (M1, 8 GB), so the cap is **3**: batch 1 = t1, t2, t3; batch 2 = t4, t5, t6.
- **Specs:** [`docs/trials/specs/`](trials/specs/) (`_common.md` plus one per Job).
- **Isolation rules** (in every Spec):
  - **herdr:** only the named session `cpd-trial` (`herdr --session cpd-trial …`, a separate server). Never touch, stop or reconfigure the default session.
  - **pi:** load packages per run with `pi -e <source>`. Never `pi install`, never edit `~/.pi/agent/settings.json`.
  - **Installs:** worktrunk 0.80.0 and beads 1.3.1 are declared in the dotfiles (`home/.chezmoidata/packages.yaml`, dotfiles commit `612c840`) so they travel to every Mac, and are installed here. Trials install nothing else. The only global, non-dotfiles change is t1's temporary herdr plugin link for modem-dev, unlinked at the end.
  - **Scratch only:** repos under `/tmp/cpd-trials/<job>/`. No pushes, no changes to dotfiles or real repos.
  - **Evidence:** every claim is backed by a command transcript excerpt. Untested claims are labelled.

## Stage 1: Workflow `cpd-v0-trials`

| Job | Tier | Needs | Question | Acceptance (summary) |
|---|---|---|---|---|
| t1 spawn | S4 | — | How do we start Workers and Reviewers? (A) `cpd`-native: `herdr agent start <name> --kind pi --pane <id> -- --model … --tools …`, plus a ~30-line test extension that wakes the Lead with `pi.sendMessage`; (B) giuseppecrj/pi-herdr-agents; (C) modem-dev/pi-herdr-subagents. | For each option, measured: 10/10 launches; per-spawn model + thinking honoured (read from the child's session file); child runs in a given worktree; read-only forked Reviewer (no `bash` in its tools); completion arrives in the Lead's session as a custom message; a half-typed Lead draft is still intact afterwards; a killed child is detected; escalation path works; cleanup. A recommendation with the evidence table. |
| t2 ledger | S3 | — | How does CPD's Ledger map onto beads? | `BEADS_DIR` outside the repo, init without touching it (repo byte-identical, no `AGENTS.md`). Mapping of Workflow, Job, Dependency, Attempt, review, Escalation, Tier and metrics onto beads (types, dependencies, metadata, statuses, comments); "ready" = blockers closed = Merged; atomic claim; history/audit read-back; behaviour of two concurrent writers; latency per command; whether beads' freeze marker can protect Locked Jobs. A proposed `cpd`↔`bd` contract. |
| t3 worktrees + merge | S3 | — | What are the exact worktrunk recipes? | Create a Job worktree from the Lead Branch HEAD with a blocking setup hook; register it in herdr (`worktree open`); rebase + fast-forward merge keeping commits (`wt merge --no-squash`). Failure modes, each demonstrated: Gate (`pre-merge` verify) fails; rebase conflict; two concurrent merges (compare-and-swap); dirty Lead Branch worktree. Cleanup semantics; non-interactive hook approval; rebasing the Lead Branch onto the default branch for the PR. Copy-paste recipes. |
| t4 plan approval | S3 | — | Can `cpd` drive plannotator for plan approval? | A test extension emits `plannotator:request` (`plan-review`) with a Markdown plan; the decision (approve and deny, simulated through plannotator's local HTTP API) returns as an event and a follow-up message; plan mode stays off (model, thinking and tools unchanged); state survives `/reload`. A recipe plus the integration surface. |
| t5 questions + escalation | S3 | — | How does the User answer structured questions, and how does a Worker ask the Lead? | `rpiv-ask-user-question` driven in a pi TUI pane via `herdr pane send-keys`, answers returned structured. `pi-intercom` `ask`/`reply` between two pi panes: blocking, timeout, Lead busy, Lead idle with a draft, Lead restarted. Compared with a `cpd`-native file-based blocking `ask` stub. A recommendation. |
| t6 tier catalog | H2 | — | What is a supported way to read each model's valid thinking levels? | Compare a pi-ai export, the pi CLI and `models-store.json` for stability across `pi update`. A generator snippet that reproduces the Tier table in requirements §3. |

Stage 1 is done when all six reports are accepted. The findings are then folded into `requirements.md` and ADRs.

## Stage 2: build (outline only, not for approval)

Expected components, to be specified as Jobs from Stage 1 evidence:
1. `cpd` CLI skeleton, config (`~/.cpd`), Tier module (from t6).
2. Ledger adapter over `bd` (from t2): Workflow init, Job create/lock/append, Attempt lifecycle, history.
3. Worktree and merge adapter over `wt` (from t3): create, serialised merge queue, Gate, cleanup.
4. Spawn adapter (from t1): Worker launch, forked read-only Reviewer, completion, crash detection.
5. Scheduler: ready set, cross-process concurrency cap, timeout, retry ladder (`cpd tick` plus a `cpd watch` loop).
6. Escalation (from t5).
7. Metrics collector (pane → `agent_session.path` → usage per Attempt).
8. PR loop: `gh` poller appending Jobs for CI failures and reviewer-bot comments.
9. pi adapter extension: draft-safe Lead wake, plannotator glue (from t4), `herdr:blocked`, sidebar tagging that hides Workers.
10. `cpd-lead` skill, Worker protocol, Reviewer definition, Spec template.
11. Dotfiles wiring (`~/.cpd` through chezmoi; pi packages through the `~/.pi/agent` repo).
12. End-to-end dry run on a toy repo; first metrics for tuning defaults.
