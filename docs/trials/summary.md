# Stage 1 trials: summary

Workflow `cpd-v0-trials`, run 2026-10-12, 18:27–19:06 EDT on the MacBook Air (M1, 8 GB), concurrency cap 3. All Jobs Merged (accepted). Reports are the evidence; this page only records outcomes and decisions.

| Job | Attempts | Verdict | Report |
|---|---|---|---|
| t1 spawn | 1 (S4) accepted | **Option A**: `cpd` starts Workers and Reviewers itself with `herdr agent start --kind pi -- <args>`; a ~40-line Lead-side pi extension delivers wakes with `pi.sendMessage`. No spawner package. | [t1-spawn.md](t1-spawn.md) |
| t2 ledger | 1 (S3) accepted | beads works as a store; every Ledger concept maps onto a native construct (custom statuses and types, child issues for Attempts and Escalations, `blocks` dependencies, provenance). beads cannot enforce Locked or append-only; `cpd` must. | [t2-ledger.md](t2-ledger.md) |
| t3 worktrees + merge | 1 (S3) accepted | worktrunk covers create (blocking setup), rebase + fast-forward merge with commits kept, gate, cleanup and PR rebase, non-interactively. Exit codes need probing; branch naming must be flat. | [t3-worktrees-merge.md](t3-worktrees-merge.md) |
| t4 plan approval | 1 (S3) accepted | plannotator's event API works with plan mode off and no Tier side effects; the decision comes back as an event that the adapter must forward (no follow-up message on this path); recovery after `/reload` by polling. | [t4-plan-approval.md](t4-plan-approval.md) |
| t5 questions + escalation | 1 (S3) accepted | Keep `rpiv-ask-user-question` for structured questions (preview capped at 20 rows, so it is a decision gate, not a Plan reader). Escalation: **`cpd`-native file transport** (survives Lead restart, agent-agnostic); pi-intercom works but loses pending questions on restart. | [t5-questions-escalation.md](t5-questions-escalation.md) |
| t6 tier catalog | 2: H2 rejected ([a1](t6-tier-catalog.a1.md): SDK path not evaluated, unverified stability claims, ignored `PI_CODING_AGENT_DIR`, wrong date); H3 accepted | `ModelRuntime.create()` from `@earendil-works/pi-coding-agent` (documented SDK export, no session, no network) plus pi-ai's level rule; `models-store.json` only as fallback. | [t6-tier-catalog.md](t6-tier-catalog.md) |

Reviewer notes on accepted reports (to carry into Stage 2):
- **t6 a2:** the generator imports pi from a hard-coded `releases/1.1.0` path, and copies `getSupportedThinkingLevels` instead of importing it. `cpd` must resolve the installed pi release itself and import the rule.
- **t2:** two benign isolation slips outside the sandbox. `~/.dolt/` was created by a `dolt version` call, and `~/.config/bd/config.yaml` was set to `metrics.disabled: true`. The Lead's own first `bd --version` had already created `~/.beads/machine-id` and `~/.config/bd/config.yaml`. These are tool state that normal `bd` use creates anyway, so they stay. `cpd` sets `BD_DISABLE_METRICS=1 DO_NOT_TRACK=1` on every call so telemetry opt-out does not depend on per-machine config.

## Facts established (cross-trial)

- **Draft safety holds only through pi extensions.** Every `pi.sendMessage` path (A, B, C, plannotator adapter, pi-intercom, file stub) left a half-typed Lead draft intact. `herdr agent prompt` into a Lead merged and submitted the draft (t1 negative control).
- **A Worker's completion must be proven by an artefact.** After `kill -9`, `agent prompt --wait` returned rc 0 / `idle` (t1). Use a `cpd attempt submit` call by the Worker plus the session's final `stopReason`, and herdr's `released:true` event to detect death.
- **Trust dialog.** pi shows "Trust project folder?" in worktrees with a project `.pi/`; herdr still reports the agent ready. Workers need `--approve` or dialog detection (open question for the User).
- **pi sets `process.title = "pi"`.** `ps` cannot show its args; use `agent start`'s `.result.argv` and the session file.
- **Session paths come from herdr** (`agent_session.value`), never from string-encoding a cwd (macOS `/tmp` → `/private/tmp`).
- **Branch naming:** `<lead>/<job>` cannot coexist with `<lead>` in git. Attempt branches use a flat suffix: `<lead>--<job>-a<n>`.
- **Gate tampering:** worktrunk reads hooks and the Gate script from the Attempt being merged, so a Worker can weaken the Gate. `cpd` rejects Attempts whose diff touches `.config/wt.toml` or the Gate script.
- **beads:** `cpd` is the only writer (per-Workflow lock). Lock fingerprints are kept outside beads. Merged means `bd close` from `in_review` only. Cancelled means custom `cancelled` (frozen), which keeps dependents blocked; `bd close` on a cancelled Job would unblock them. Flags: `--if-status` gives compare-and-swap; init uses `--skip-agents --skip-hooks`, never `--stealth`.
- **Headless herdr panes started at 120×20** although the layout reports 40 rows (t5). Irrelevant when the User's client is attached; set `headless_*` for unattended runs.

## Decisions taken by the Lead within the approved scope

- `cpd` keeps a Locked-Job fingerprint store outside beads and audits `bd history --events` actors (t2 escalation 1; follows from ADR-0003/0004).
- `~/.dolt` and bd's home files stay (t2 escalation 2; normal tool state).
- plannotator and `rpiv-ask-user-question` are installed for the Lead as pi packages declared in `~/.pi/agent/settings.json` (the pi-config repo the dotfiles sync), pinned to the trialled versions (t4 escalation).

## Decided by the User

- **Workers run with `pi --approve`** (2026-10-12): trust project-local `.pi/` extensions and `AGENTS.md` of the repo under work, no dialog (t1 escalation).
