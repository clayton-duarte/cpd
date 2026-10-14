# `cpd` v0: CLI contract

Status: **draft for Stage 2 approval**. This document is normative for every Stage 2 build Job: a Job implements exactly the parts named in its Spec. Vocabulary: [CONTEXT.md](../CONTEXT.md). Rules: [requirements.md](requirements.md). Proven command recipes: the [Stage 1 trial reports](trials/summary.md). This contract references them instead of repeating them; where it differs, this contract wins.

## 1. Runtime and code conventions

- **Language:** TypeScript on Node 22, which runs `.ts` directly (verified: `process.features.typescript === "strip"` on 22.22). No build step. Only erasable TypeScript syntax: `tsconfig` sets `"erasableSyntaxOnly": true` and `"noEmit": true`, so no `enum`, `namespace` or parameter properties.
- **Dependencies:** runtime `zod` only. Dev: `typescript`, `@types/node`. Adding any other dependency is an Escalation, not a Job decision.
- **Entry point:** `bin/cpd.mjs`, `#!/usr/bin/env node`, which `import`s `../src/cli.ts`. `package.json` has `"type": "module"` and `"bin": {"cpd": "bin/cpd.mjs"}`.
- **One file per command:** `src/cli.ts` maps `cpd <group> <command>` to `src/commands/<group>-<command>.ts`, and single-word commands such as `cpd ask` to `src/commands/<command>.ts`. Each file exports `run(argv: string[]): Promise<Result>`. This keeps file ownership disjoint between Jobs.
- **Arguments:** parsed with `node:util` `parseArgs`. Unknown flags are usage errors.
- **Validation:** every external input (CLI args, config, spec files, `bd`/`wt`/`herdr`/`gh` JSON, queue and escalation files, pi session lines) is parsed with zod at the boundary. Inside, types are trusted. Never fall back silently.
- **Subprocesses:** run with `node:child_process` `spawn` (argv arrays, never a shell string), with stdin from `/dev/null`, explicit `cwd` and `env`. Capture stdout and stderr. Treat the exit code before parsing output.
- **Tests:** `node --test`, files `test/**/*.test.ts`.
  - Unit tests use fake binaries: executable scripts on a temp `PATH` that record argv and print canned JSON.
  - Integration tests against real `bd`, `wt` and `git` run in temp dirs with a sandboxed `HOME` and run by default.
  - Integration tests that need a herdr server or a real pi model run only when `CPD_IT=1`, against herdr session `cpd-test`.
- **Gate for this repo:** `npm run verify` = `npm run typecheck && npm test` (`tsc --noEmit -p .`, then `node --test`). `.config/wt.toml` sets `[pre-merge] verify = "npm run verify || exit 64"`. (Until 2026-10-13 this was `./verify.sh`; the User chose npm scripts. The tamper list in §11 still names `verify.sh` until v1 protects `package.json` instead.)

## 2. Output, errors and exit codes

- Every command prints exactly one JSON value to stdout.
  - Success: `{"ok": true, ...}`.
  - Failure: `{"ok": false, "error": {"code": "<snake_case>", "message": "<one sentence>", "details": {...}}}`.
- Human progress and debug text goes to stderr. Each Workflow also appends structured events to `<wf>/logs/cpd.jsonl` (`{at, level, event, ...}`).
- Exit codes: `0` ok · `1` failure (external tool or unexpected) · `2` usage error · `3` refused by an invariant (Locked, illegal transition, precondition) · `4` not found · `5` timeout.

## 3. Identifiers

| Thing | Format | Example |
|---|---|---|
| Workflow id | `^[a-z][a-z0-9-]{1,23}$` (also the beads prefix) | `pay-4521` |
| Job key | `^[a-z][a-z0-9-]{0,31}$`, unique in the Workflow | `api-schema` |
| Attempt id | `<job-key>-a<n>` (n from 1) | `api-schema-a2` |
| Lead Branch | `cpd/<wf>` | `cpd/pay-4521` |
| Attempt branch | `<lead-branch>--<attempt-id>` (flat: git forbids `cpd/<wf>/…`) | `cpd/pay-4521--api-schema-a2` |
| herdr agent name | `c` + first 12 hex chars of `sha1("<wf>/<attempt-id>/<role>")`, role `w` or `r` (meets `[a-z][a-z0-9_-]{0,31}`, unique per server) | `c3f9a0b1c2d4e` |
| Escalation id | `<attempt-id>-q<k>` (k from 1 per Attempt) | `api-schema-a2-q1` |

Readable labels (`<wf> <job> a<n>`) go on herdr workspaces and panes; names are for addressing.

## 4. State layout (`$CPD_HOME`, default `~/.cpd`)

```
$CPD_HOME/
  config.json                         machine config (dotfiles-managed), §5
  slots/slot-<i>.json                 machine-wide concurrency slots, §9
  locks/merge-<sha1(repo realpath)>.lock
  leads/<lead-session-id>/queue/      messages to that Lead (§10); rejected/ for invalid ones
  leads/<lead-session-id>/workflow    text file: the wf id this Lead session is attached to
  workflows/<wf>/
    workflow.json                     cpd-owned facts (§6)
    ledger/                           beads workspace; BEADS_DIR=ledger/.beads
    fingerprints.json                 Locked-Job fingerprints (§8)
    escalations/<esc-id>.q.json|.a.json
    specs/<attempt-id>.md             rendered Worker Spec
    reviews/<attempt-id>.md|.diff|.gate.txt
    plannotator/                      PLANNOTATOR_DATA_DIR for this Workflow's Lead
    pr-state.json                     last seen PR state (§13)
    watch.json                        {pid, startedAt, heartbeatAt} of the live `cpd watch`
    locks/ledger.lock
    logs/cpd.jsonl, logs/attempt-<attempt-id>.log
```

Locks are files created with `open(O_CREAT|O_EXCL)` containing `{pid, at, purpose}`. A lock whose pid is not alive is stale and may be taken over; log the takeover. Waiting is bounded (default 60 s), then exit 5 `lock_timeout`.

## 5. Config `$CPD_HOME/config.json`

Strict schema: unknown keys are rejected, and a missing file is error `config_missing` (exit 1).

```json
{
  "version": 1,
  "concurrency": 3,
  "retryCap": 2,
  "attemptTimeoutMinutes": 30,
  "askTimeoutMinutes": 60,
  "tickSeconds": 5,
  "prPollSeconds": 120,
  "herdrSession": null,
  "tiers": { "O": "github-copilot/claude-opus-5.5", "S": "github-copilot/claude-sonnet-5.5", "H": "github-copilot/claude-haiku-4.5" },
  "leadTier": "O3",
  "defaultReviewTier": "S4",
  "reviewMode": "fork",
  "leadPiPackages": ["npm:@plannotator/pi-extension@0.28.17", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"]
}
```

`herdrSession: null` means herdr's default session (no `--session` flag); a string adds `--session <name>` to every herdr call. `reviewMode` is `fork` (Reviewer forks the Lead session; the default, per ADR-0002) or `fresh` (a new read-only session with no Lead conversation; for very long Lead sessions where forking costs too much).

## 6. `workflow.json`

```json
{ "v": 1, "id": "pay-4521", "title": "…", "repo": "/abs/realpath", "base": "main",
  "leadBranch": "cpd/pay-4521", "leadWorktree": null, "createdAt": "ISO",
  "lead": null, "pr": null }
```

- `lead` = `{host, paneId, tmuxSocket?, sessionPath, sessionId, queueDir, attachedAt}`, set by `cpd lead attach`. `host` is `"herdr"` or `"tmux"`; `paneId` is that host's pane id; `tmuxSocket` (the socket path from `$TMUX`) is present exactly when `host` is `tmux`. A file without `host` (written before tmux support) reads as `herdr`.
- `leadWorktree` is set by `cpd plan lock`.
- `pr` = `{number, url, openedAt}`, set by `cpd pr open`.
- Writes are atomic (temp file plus rename) under `locks/ledger.lock`.

## 7. Tiers

- Syntax `^(O|S|H)([0-5])$`. Effort to pi level: 0 off, 1 low, 2 medium, 3 high, 4 xhigh, 5 max. pi argument: `--model <config.tiers[L]>:<level>`.
- **Valid efforts per model** come from pi's own catalog (trial t6):
  - Locate the installed pi release the way the launcher does: realpath of `pi` on `PATH`, then `<agentDir>/install/current-version`, then `<agentDir>/install/releases/<v>/node_modules/@earendil-works/pi-coding-agent/dist/index.js`.
  - Load it with `import()` and call `ModelRuntime.create({ allowModelNetwork: false })`, then `getModel(provider, id)`.
  - Supported levels follow pi-ai's `getSupportedThinkingLevels` rule: import it from the same release's `@earendil-works/pi-ai`; do not re-implement it. `minimal` is never used.
  - Unknown model → error `tier_model_unknown`.
- **Ladder** `next(t)`:
  - same model with effort+1 if that effort is valid;
  - otherwise the next model in `H → S → O` at the lowest valid effort ≥ the current effort;
  - otherwise none.
  - Examples: `H1→H2`, `H3→S3`, `S4→S5`, `S5→O5`, `O4→O5`, `O5→none`.
- **Attempt Tier:**
  - Attempt 1 uses the Job's Tier; Attempt n uses `next` applied n−1 times.
  - Automatic Attempts are capped at `1 + retryCap`, and stop early if `next` is none.
  - `cpd job retry` adds an Attempt with an explicit Tier (the Lead's decision).

## 8. Ledger (beads)

- **Normative mapping:** the trial t2 report "Recipes" (`docs/trials/t2-ledger.md`), with these choices:
  - **Init:** `bd init --init-if-missing --prefix <wf> --skip-agents --skip-hooks --non-interactive`, run with cwd `ledger/` and `BEADS_DIR` unset. Then set `status.custom` and `types.custom` exactly as in t2.
  - **Every `bd` call:** cwd `ledger/`, env `BEADS_DIR=<ledger>/.beads BEADS_ACTOR=cpd BD_DISABLE_METRICS=1 DO_NOT_TRACK=1 BD_NON_INTERACTIVE=1`, always `--json` (or `--silent`), and holding `locks/ledger.lock`.
  - **Issue types:** Workflow = `workflow`; Job = `job` (child of the Workflow, `metadata.key` = Job key); Attempt = `attempt` (child of the Job); Escalation = `escalation` (child of the Attempt).
  - **Job statuses:** `draft` → `queued` → `running` → `in_review` → `closed` (Merged) | `failed` | `cancelled`.
  - **Job metadata:** `key, tier, review_tier, context, touches, acceptance, out_of_scope`. The goal is the description.
  - **Attempt metadata:** `number, tier, worktree, branch, agent, session_path, started_at, submitted_at, ended_at, outcome, cause, metrics_*`.
  - **Dependencies:** `bd dep add <job> <dependency>` (type `blocks`).
- **Invariants `cpd` enforces before writing** (t2 F5):
  - Only `draft` Jobs may be edited.
  - Never `delete`, `prune`, `flatten`, `gc`, `compact` or `reopen`.
  - `close` a Job only from `in_review` with an approved last Attempt.
  - State transitions use `--if-status` compare-and-swap.
  - Each metadata key is written once (check before write).
- **Fingerprints:**
  - At `plan lock` and at append, store `sha256` of canonical JSON `{goal, tier, review_tier, context, touches, acceptance, out_of_scope, needs (sorted keys)}` per Job in `fingerprints.json`.
  - Every Dispatch step that reads a Job recomputes it and fails with `locked_job_tampered` (exit 3) on mismatch.
  - `cpd workflow show` also reports any `bd history --events` entries whose `actor` is not `cpd`.

## 9. Concurrency slots

Slots are `slots/slot-0.json` … `slot-<concurrency-1>.json`, each created exclusively and holding `{wf, attempt, pid, at}`. Acquire the first free or stale slot. `attempt run` releases its slot when it ends. The cap is machine-wide across Workflows.

## 10. Lead queue messages (cpd → Lead)

- **Location and write:** file `leads/<session-id>/queue/<epoch-ms>-<id>.json`, written as `.tmp` then renamed.
- **Schema:** `{ "v": 1, "id", "kind", "wf"?, "job"?, "attempt"?, "text", "data": {}, "createdAt" }`. `wf` is required for every kind except `rename`, which belongs to a session, not a Workflow. `text` is one short paragraph addressed to the Lead, ending with the exact `cpd` command(s) it may want to run.

| kind | When | Adapter delivery |
|---|---|---|
| `job-merged` | a Job became Merged | turn |
| `attempt-retry` | an Attempt failed or was rejected and the next Attempt (with Tier) was queued | no turn (`triggerTurn: false`) |
| `job-failed` | ladder exhausted, timeout, setup failure, crash past retries, merge needing a decision | turn |
| `escalation` | a Worker asked (`cpd ask`) | turn; set herdr blocked |
| `escalation-answered` | `cpd answer` recorded an answer | no message; clear blocked |
| `review-needed` | Reviewer gave no valid verdict twice | turn |
| `all-merged` | every Job is Merged or cancelled | turn |
| `pr-feedback` | new failing checks, reviews or comments on the PR | turn |
| `workflow-done` | PR merged | turn |
| `error` | `cpd watch` or `attempt run` hit an unexpected failure | turn |
| `rename` | `cpd session rename`; `data: {name}` (1–80 characters, no control characters) | no message, no turn; call `pi.setSessionName(name)` and show a one-line notice |

## 11. Commands

`--wf <id>` (or env `CPD_WF`) selects the Workflow where needed; error `workflow_required` (exit 2).

### Setup and diagnostics
- `cpd doctor`: `{ok, checks: [{name, ok, found?, expected?, message?}]}`; exit 1 if any check fails. Checks:
  - node ≥ 22.18 with type stripping;
  - `bd` 1.3.1, `wt` 0.80.0, `herdr` 0.9.3 (exact), pi `>=1.1.0 <2.0.0` (pi updates itself; 1.2.0 was installed during Stage 2), `gh` present;
  - config valid;
  - every Tier letter resolvable.
- `cpd tiers`: `{ok, tiers: {O: {model, efforts: [..]}, S: …, H: …}}`.
- `cpd lead start`: must run inside herdr (`HERDR_ENV=1`, else `not_in_herdr`, exit 3). It `exec`s `pi --model <leadTier> -e <repo>/pi -e <each leadPiPackages>` with env `CPD_HOME`, `PLANNOTATOR_DATA_DIR=$CPD_HOME/plannotator`, `PLANNOTATOR_UPDATE_CHECK=0`. It never returns.
- `cpd lead attach --wf <id>`: run by the Lead inside herdr or tmux.
  - Host: herdr when `HERDR_ENV=1` and `HERDR_PANE_ID` are set; else tmux when `TMUX_PANE` and `TMUX` are set; else `no_lead_pane` (exit 3), naming both hosts.
  - Session: `PI_SESSION_FILE` when set (pi sets it in its bash tool), read with the session header; otherwise, in herdr, the pane's `agent_session` from `herdr pane get`. Neither → `lead_session_unknown`.
  - `sessionId` = the pi session id from the session file header.
  - Writes `workflow.json.lead`, creates the queue dir, and writes `leads/<sessionId>/workflow`.
  - Re-attaching a new Lead session replaces the previous one (logged).

### Session
- `cpd session rename <name> [--session <id|path>]`: rename a pi session; needs no herdr or tmux.
  - Name: 1–80 characters, no control characters; else `invalid_name` (exit 2), nothing written.
  - Session: `--session` (a path when it contains `/` or ends in `.jsonl`, read with the session header; otherwise a session id whose `leads/<id>` dir must exist), else `PI_SESSION_FILE`. Neither, or an unknown id → `session_unknown` (exit 3).
  - Writes one `rename` message (§10) to `leads/<sessionId>/queue/`; the pi adapter (§15) applies it. Returns `{ok, sessionId, name}`.

### Workflow
- `cpd workflow init --wf <id> --repo <path> --title <text> [--base <branch>]`:
  - `--base` defaults to the repo's `origin` HEAD branch, else `main`.
  - Creates the state dirs, initialises the Ledger, creates the Workflow issue (`phase=context`) and writes `workflow.json`.
  - Refuses `workflow_exists` (exit 3).
- `cpd workflow show`: `{ok, workflow, phase, jobs: [{key, title, status, tier, needs, attempts: [{id, tier, outcome}]}], counts, tamper: []}`.
- `cpd workflow phase plan`: the only manual transition (context → plan). Every other transition belongs to `plan lock`, `pr open` and `pr poll`; anything else is `illegal_transition` (exit 3).

### Jobs
- `cpd job add --key <k> --title <t> --tier <T> --spec <file.json> [--needs k1,k2] [--review-tier <T>]`.
  - Spec file schema (all required): `{"goal": string, "context": string, "touches": string[≥1], "acceptance": string[≥1], "outOfScope": string[]}`.
  - In phase `plan`: status `draft`.
  - In phase `dispatch` or `pr`: an **appended** Job, status `queued` at once, fingerprint recorded, message logged. (Phase `pr` is where PR feedback becomes appended Jobs; `cpd watch` keeps scheduling until phase `done`. Amended 2026-10-13 after a Worker escalation.)
  - Other phases: `illegal_phase` (exit 3).
  - Validates the Tiers, that `needs` exist and are not `cancelled`/`failed`, no cycle, and a unique key.
- `cpd job edit <k> [--title] [--tier] [--spec] [--needs] [--review-tier]`: `draft` only, else `job_locked` (exit 3).
- `cpd job list [--status <s>]`, `cpd job show <k>`: show includes Attempts, verdict feedback and Escalations.
- `cpd job cancel <k> --reason <text>`:
  - Any Job not `closed` or `cancelled`.
  - Without a running Attempt the Job becomes `cancelled` at once.
  - With a running Attempt, `job cancel` writes write-once metadata `cancel_requested`. `cpd watch` then, within one tick, closes the Attempt's herdr panes, keeps its worktree, closes the Attempt with outcome `cancelled` and sets the Job `cancelled`.
  - Dependents stay blocked.
- `cpd job retry <k> --tier <T>`: only a `failed` Job. Records metadata `retry_<n>_tier` (write-once) and sets status `queued`.
- `cpd job dispatch <k>`: start one ready Job now, the same step `cpd watch` runs for each ready Job.
  - Acquire a slot, else `no_slot` (exit 3).
  - Start the Attempt with compare-and-swap at its Tier (§7, or `retry_<n>_tier`), else `cas_lost` or `job_not_ready` (exit 3).
  - Spawn `cpd attempt run <attempt-id> --wf <wf> --slot <i>` detached, logging to `logs/attempt-<id>.log`.
  - Returns `{ok, attempt, slot, pid}`.
  - Lets a Lead (or a bootstrap operator) dispatch by hand before or without `cpd watch`.

### Plan
- `cpd plan render [--out <file>]`: Markdown for review.
  - Title and phase.
  - A table with key, title, Tier, needs and touches.
  - A Mermaid `graph TD` of the Dependencies.
  - One section per Job with its Spec.
  - Only GFM, tables, code and Mermaid (no footnotes, no raw HTML; t4).
  - Returns `{ok, path, fingerprint}`, where the plan fingerprint is the sha256 of the canonical JSON of all `draft` Jobs.
- `cpd plan decision --review-id <id> (--pending | --approved | --rejected) [--feedback-file <f>]`: called by the pi adapter. Records Workflow metadata `plan_review_<id>` = `{fingerprint, status, approved?, feedback?}` (write-once per state).
- `cpd plan lock`:
  - Requires the latest plan review to be approved for the **current** plan fingerprint, else `plan_not_approved` (exit 3).
  - Validates the DAG and computes fingerprints.
  - Sets all Jobs `queued` with one `bd batch` and the phase to `dispatch`.
  - Creates the Lead Branch in its own worktree: `wt --yes -C <repo> switch --create cpd/<wf> --base <base> --no-cd --format json`, recorded as `leadWorktree`.
  - Unless `--no-watch`, opens `cpd watch --wf <wf>` in a new herdr pane split from the Lead pane (`--no-focus`).
  - Returns `{ok, leadBranch, leadWorktree, watchPane}`.

### Dispatch
- `cpd watch`: foreground loop, one live instance per Workflow (`watch.json`), every `tickSeconds`:
  1. **Escalations:** record each new `escalations/*.q.json` in the Ledger (Escalation issue). Record each new answer and close it.
  2. **Liveness:** for every running Attempt:
     - if its Worker agent is gone (`herdr agent get` not found) while `attempt run` is not alive, mark it crashed;
     - if active time exceeds `attemptTimeoutMinutes`, close its Worker pane and set `metadata.timed_out=true`. Active time = elapsed minus time with an open Escalation;
     - if its Job has `cancel_requested`, stop it as described under `cpd job cancel`.
  3. **Scheduling:** for each ready Job, in creation order and while a slot is free, run the `job dispatch` step (same function).
  4. **Completion:** when no Job is `queued`, `running` or `in_review` and at least one is Merged, send `all-merged` once.
  5. **PR:** in phase `pr`, run `cpd pr poll --wf <wf>` as a subprocess every `prPollSeconds` (a process boundary, so `watch` does not depend on the PR module).
  6. **Exit:** when the phase becomes `done`.
- `cpd attempt run <attempt-id>`: the lifecycle in §12. Exit 0 when the Attempt reaches a terminal state.
- `cpd attempt submit <attempt-id>`: called by the Worker. Preconditions, each refusing with exit 3 and a code the Worker can act on:
  - cwd inside the Attempt worktree (`wrong_worktree`);
  - clean tree (`dirty_tree`);
  - at least one commit ahead of the Attempt's base (`no_commits`);
  - diff does not touch `.config/wt.toml`, `verify.sh` or `.cpd/` (`gate_tampered`);
  - Gate passes: `wt --yes -C <worktree> hook pre-merge`; exit 64 → `gate_failed`, with the last 60 lines of output in `details`.

  On success: Attempt `submitted_at`, provenance `commit`, returns `{ok, submitted: true, next: "Stop now. Do not make more changes."}`.
- `cpd attempt read <attempt-id> [--lines N]`: debug view, the last N lines of the Worker's session transcript (assistant text and tool names).
- `cpd attempt list`, `cpd attempt show <attempt-id>`.
- `cpd review decide <attempt-id> (--approve | --reject) --feedback <text>`: the Lead's override when the Reviewer failed (`review-needed`). It continues §12 step 9 or 10.

### Escalation
- `cpd ask <attempt-id> <question> [--timeout-minutes N]`: run by the Worker; blocks.
  - Writes `escalations/<esc-id>.q.json` = `{v:1, id, wf, attempt, question, askedAt, askerPid}` and a Lead queue message `escalation`.
  - Polls for `.a.json` every 0.5 s.
  - Returns `{ok, answer}`.
  - On timeout (default `askTimeoutMinutes`) it renames the question to `<esc-id>.expired.json` (never deleted, so ids are never reused and history is kept), exits 5 `ask_timeout`, and records it. Orphan expiry (dead asker) does the same rename.
- `cpd answer <esc-id> <answer>`: run by the Lead.
  - Refuses `escalation_missing` or `escalation_answered` (exit 3).
  - Writes `.a.json` = `{v:1, id, answer, answeredAt}` and queue message `escalation-answered`.

### Pull Request
- `cpd pr open`:
  - Requires every Job `closed` or `cancelled`, and at least one `closed`.
  - Runs `git -C <leadWorktree> push -u origin cpd/<wf>`.
  - Runs `gh pr create --base <base> --head cpd/<wf> --title <workflow title> --body <summary>`. The summary is the Jobs table plus the Merged commits.
  - Sets the phase to `pr`, records `workflow.json.pr` and returns `{ok, pr}`.
- `cpd pr poll`:
  - Runs `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments` and `gh api repos/{owner}/{repo}/pulls/{n}/comments`, then diffs against `pr-state.json`.
  - New failing checks, reviews or comments: one `pr-feedback` message with them in `data`.
  - Merged: phase `done`, `touch <ledger>/MIGRATION-FREEZE`, message `workflow-done`.
  - Returns `{ok, changes: [...]}`.
- `cpd pr rebase`: the trial t3 "pr rebase" recipe for the clean case (`git merge-tree` check, then `wt step rebase <base>`, then `git push --force-with-lease`). On a conflict it changes nothing and returns `rebase_conflict` (exit 3) with the file list; the Lead decides.
  - It is also how Jobs merged in phase `pr` reach the PR: run it after their `job-merged` messages. If the base has not moved, the rebase is a no-op and only the push happens.

## 12. Attempt lifecycle (`cpd attempt run`)

Recipes: trial t3 (worktrunk), t1 (herdr and pi), t4/t5 (queue delivery). Every Ledger write holds `locks/ledger.lock`, and every step is logged to `logs/attempt-<id>.log`.

1. **Load:** the Attempt and Job from the Ledger; verify the Job fingerprint.
2. **Worktree:** `wt --yes -C <leadWorktree> switch --create <attempt-branch> --base cpd/<wf> --no-cd --format json`, which blocks while setup runs. Then assert `git merge-base` equals the Lead Branch HEAD.
   - On a non-zero exit: run `wt --yes -C <repo> remove <branch> -D -f --foreground`.
   - Then outcome `failed`, cause `setup_failed`: the Job becomes `failed` with message `job-failed`, and the ladder is not used.
3. **Spec:** render the Worker Spec (template `templates/worker.md`) to `specs/<attempt-id>.md`. For n > 1 it includes the previous Attempts' outcomes and feedback.
4. **Placement:** `herdr worktree open --cwd <repo> --path <worktree> --label "<wf> <job> a<n>" --no-focus`; the root pane comes from `herdr pane list --workspace <id>`.
5. **Start the Worker:** `herdr agent start <name> --kind pi --pane <pane> --timeout 60000 -- --approve --no-skills --exclude-tools ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox --model <model:level>`. Record `.result.argv`, `agent_session.value` and the agent name in Attempt metadata, plus provenance `cut`.
6. **Prompt:** `herdr agent prompt <name> "Read <spec path> and follow it exactly. Your attempt id is <attempt-id>." --wait`, with no herdr timeout (`cpd watch` enforces the Attempt timeout).
7. **Decide what happened.** Never trust herdr idle alone (t1 F7).
   - **Submitted:** go to 9.
   - **`metadata.timed_out` set:** outcome `failed`, cause `timed_out`; the Job becomes `failed` and the Lead gets `job-failed` (requirements: a timeout escalates to the Lead).
   - **Agent gone:** if the agent no longer exists, or the session's last assistant `stopReason` is not `stop`, outcome `failed`, cause `crashed`, then step 8.
   - **Ended without submitting:** prompt once more (`--wait`): "You ended your turn without running `cpd attempt submit <id>`. Finish the Spec and submit, or run `cpd ask <id> "<question>"` if you are blocked." If it is still not submitted, outcome `failed`, cause `no_submit`, then step 8.
8. **Failure handling:** close the Worker pane and keep the worktree.
   - If the ladder has a next Tier and the automatic Attempt count is below `1 + retryCap`: close the Attempt, set the Job back to `queued` (the next Attempt is created by `watch` with the next Tier), and send `attempt-retry`.
   - Otherwise the Job becomes `failed` with `job-failed`.
   - Collect metrics (§14) in both cases, then release the slot.
9. **Review:**
   - Write `reviews/<attempt-id>.diff` (`git diff cpd/<wf>...<branch>`), `.gate.txt` (gate output from submit) and the brief (template `templates/reviewer.md`) to `reviews/<attempt-id>.md`.
   - Split a pane from the Lead pane (`--no-focus`, cwd = Lead's cwd) and run `herdr agent start <rname> --kind pi --pane <p> --timeout 60000 -- --approve --no-skills --exclude-tools ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox --fork <lead sessionPath> --tools read,grep,find,ls --model <reviewTier model:level>`.
   - With `reviewMode: "fresh"`, drop `--fork <lead sessionPath>`. The brief then also points to the rendered plan (`<wf>/plan.md`) for the Workflow's context.
   - Prompt it to read the brief (`--wait`).
   - Parse the verdict from the **last assistant message** of the Reviewer's session: one fenced `json` block `{"verdict":"approve"|"reject","feedback":string}`.
   - If it is missing or invalid, re-prompt once. Still invalid: `review-needed`, and the Attempt waits for `cpd review decide`.
   - Close the Reviewer pane. Record the verdict as a comment and the Attempt's `outcome`.
10. **Rejected:** close the Attempt as `rejected` (cause `review_rejected`), then the ladder as in step 8.
11. **Approved: merge** under the per-repo merge lock, with the t3 "attempt merge" recipe and its classification.
    - **Success:** post-merge Gate `wt --yes -C <leadWorktree> hook pre-merge`. If it fails, `git reset --keep <pre>`, the Job becomes `failed` with `job-failed`.
    - **cas-lost:** retry up to 3 times.
    - **Conflict:** abort the rebase, outcome `failed`, cause `merge_conflict`, then step 8 (the next Attempt starts from the new Lead HEAD and its Spec shows the conflict).
    - **dirty-target:** `job-failed` with an explanation (the Lead worktree must stay clean).
12. **Merged:**
    - In the Ledger: close the Attempt as `approved`, close the Job with reason `merged` and `metadata.merged_sha`, record provenance `land`.
    - Record metrics.
    - Remove the Attempt worktree and branch (t3 "attempt remove" recipe, including `herdr workspace close`).
    - Release the slot and send `job-merged`.

## 13. Pull-request state

`pr-state.json` = `{v:1, number, lastPollAt, checks: {<name>: <conclusion>}, reviewIds: [], commentIds: []}`. A change is a check whose conclusion became failing, or a review or comment id not seen before.

## 14. Metrics

Parse the Worker's pi session JSONL (path from `agent_session.value`; never derived from cwd):
- `metrics_turns` = number of assistant messages;
- `metrics_tokens_in`, `metrics_tokens_out`, `metrics_cache_read`, `metrics_cache_write`, `metrics_cost_usd` = sums of `usage`;
- `metrics_duration_s` = last minus first entry timestamp;
- plus `model`, `thinking_level` and the final `stopReason`.

Recorded once per Attempt (write-once keys). Reviewer metrics are recorded on the Attempt as `review_metrics_*`.

## 15. pi adapter (`pi/` package, the v0 Lead adapter)

- **Package:** `pi/package.json` with `"keywords": ["pi-package"]` and `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`. Host packages are `peerDependencies: "*"`.
- **Activation:** only in TUI mode, and only when `HERDR_ENV=1` or `TMUX_PANE` is set (`pi/lib/activation.ts`).
- **Queue:**
  - At `session_start`, the queue dir is `$CPD_HOME/leads/<sessionId>/queue` (sessionId = pi's session id). Create it, drain it, then `fs.watch` it. Close the watcher on `session_shutdown`.
  - For each file: claim it by renaming, validate it (schema §10), then deliver with `pi.sendMessage({customType: "cpd", content: text, display: true, details: msg}, {triggerTurn, deliverAs: "followUp"})` per the table in §10, and delete it.
  - An invalid file is moved to `queue/rejected/` and reported to the Lead as an error message (fail loud).
- **Blocked state:** an `escalation` emits `pi.events.emit("herdr:blocked", {active: true, label: "cpd: Worker question"})`, and `escalation-answered` emits `{active: false}`, counted per id.
- **Tool `cpd_plan_review`** (no parameters), used by the Lead to request plan approval:
  - Read the attached wf from `leads/<sessionId>/workflow`, then run `cpd plan render --out <tmp>` via `pi.exec`.
  - Emit `plannotator:request` `{action: "plan-review", payload: {planContent, planFilePath}}`.
  - On `pending`, run `cpd plan decision --review-id <id> --pending`.
  - On `plannotator:review-result`, run `cpd plan decision … --approved | --rejected --feedback-file <f>` and deliver the decision and feedback to the Lead (`triggerTurn: true`).
  - At `session_start`, poll `review-status` for pending reviews (t4 F5).
- **Rename:** a `rename` message calls `pi.setSessionName(data.name)` and shows a one-line notice (`ctx.ui.notify`); it sends no message and triggers no turn.
- **Never** call `herdr agent prompt`, `pane run` or `send-text` toward the Lead.

## 16. Templates and the Lead skill

- `templates/worker.md`: Worker Spec, rendered per Attempt. It contains:
  - rules: work only in this worktree, commit your work, run the Gate before submitting, never edit `.config/wt.toml`, `verify.sh` or `.cpd/`, and do not ask the User anything;
  - the Job's goal, context, files to touch, acceptance commands and out-of-scope items;
  - previous Attempts' feedback;
  - how to finish (`cpd attempt submit <id>`) and how to escalate (`cpd ask <id> "<question>"`).
- `templates/reviewer.md`: the review brief.
  - Inputs: the Spec, the diff path, the Gate output, and the worktree path for reading files.
  - Rubric: approve only if every acceptance item is met, nothing out of scope changed, and touched files fit the Spec.
  - The final verdict block format.
- `pi/skills/cpd-lead/SKILL.md` (shipped in the `pi/` package, so `-e <repo>/pi` or the package in settings loads it with the extension): the Lead protocol for Context, Plan, Dispatch and PR using only `cpd` commands, `ask_user_question` (trial t5 rules) and `cpd_plan_review`. It covers how to react to each queue message kind, and the append-only rules (append, cancel or retry; never edit after lock).
