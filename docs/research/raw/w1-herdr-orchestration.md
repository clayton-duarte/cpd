# W1 herdr-side orchestration, workflow and task-state plugins: research report

Read at: 2026-10-12. Items reviewed: 17 listed (16 plugins/repos + `steveyegge/beads`), plus 12 additional finds. Items not reviewed in depth: `eighteyes/loud-lead` (the repo is empty: no commits, 0 files; `gh api` shows size 0, so nothing to read), `tuanhung303/herdr-swarm` (skim only), `caioniehues/herdmates` (skim only, Claude-Code-specific). `bd` itself was **not executed** (not installed; install is out of bounds), so every `bd` behaviour below is from docs and source, not from a run: "unverified by execution". npm downloads for `@beads/bd` were rate-limited by the npm API (error 1015): unknown.

All clones are in `/tmp/cpd-research/repos/<owner>__<repo>` (shallow, SHA recorded per item). Dates are the sandbox clock; "pushed" values come from the GitHub API.

## Summary

### Best candidate per need

| Need | Best item | Coverage | Notes |
|---|---|---|---|
| N1 spawn pi Worker, model+thinking per spawn | `herdr-projects` profiles (`profiles.rs:208-262` emits `--model X --thinking Y` for `pi`) | partial | Chosen by profile *name*, not free-form per spawn. 18 Tiers could be generated as profiles. `spoolway` does **not** pass effort to pi (see its item). `darjss/herdr-orchestrate` hard-codes 3 routes. |
| N2 non-clobbering completion notification | `herdr-projects` (`steps.rs:17-20, 358-372`: nudge only if the input box has looked empty 10 s, read from the screen, incl. `pi`) | partial | Only item that *measured* the problem: on herdr 0.9.1 `agent prompt` merges with half-typed text (`docs/herdr-notes.md`, Stage 2). Screen-scraping heuristic; falls back to `notification show`. `taskr` has a pi push shim (`adapters/pi/taskr-push.ts:146`) but behaviour vs. an editor draft is unverified. |
| N3 Escalation (Worker asks parent, waits) | `taskr` (`ask --blocking` + `wait --for answer`, ledger-backed, receipts) | partial | Works, but blocking wait is a tool call in the Worker and the model is claude/codex-centric; no tree-of-escalation rule enforced (parent is just `parent_id`). `husniadil/herdr-mail` has ask/reply obligations (0★, Claude-oriented). |
| N4 Reviewer fork, read-only | none | none | Nothing in this cluster forks a session. `spoolway`/`odysseus` launch a reviewer as a fresh agent with a prompt. |
| N5 Job graph, ready set, history | `beads` (`bd ready`, `bd dep`, `--claim`, `bd history`, Dolt) | full on graph/ready/claim; partial on append-only | Locked/append-only **not enforceable** (`bd delete`, `bd edit`, `bd compact` exist). Embedded Dolt is single-writer (file lock). `dagr` has the closest **vocabulary** (Task/Attempt/cause/events) but is a viewer, not a store. |
| N6 concurrency cap | `herdr-board` (`dispatch/pass.rs:33`, `max_concurrent`, real enforcement + tests); `spoolway` (`slots`/`concurrency`); `odysseus` (`max_parallel_runs`/`max_parallel_agents`) | partial (each is bound to its own dispatcher) | `herdr-projects` has `max_parallel_threads` as a **prose rule** for the coordinator (`skill/COORDINATOR.md:42`), not code. |
| N7 timeout per Attempt | `odysseus` (`agent_timeout` 45 m default, `config/mod.rs:149`); `sean1588` (`no_progress_timeout`, progress-based); `board` (per-column `timeout_minutes`) | partial | All bound to their own engines. `sean1588`'s *progress* (not position) timer is the best idea to copy. |
| N8 retry ladder (fresh session, different model/effort + feedback) | none | none | `odysseus` retries **the same live agent** with trimmed failure logs (`max_retries` 3); `spoolway` loops to an earlier step with static model/effort per step; `sean1588` has `retry_caps` with `changes_requested` loop; `board` retry re-attaches to the same conversation. No one escalates model/effort. |
| N9 worktree per Attempt from Lead Branch HEAD + per-repo setup | `herdr-projects` (`herdr worktree create --base`, `herdr.rs:372`); `StructuPath/herdr-swarm` (worktree fan-out from recorded base SHA, copy-on-write `node_modules`) | partial | `spoolway` cuts a dependent task from its first dependency's branch (`dispatcher.md:123`), `odysseus` has a "stacked" dependency mode: neither is "Lead Branch HEAD". |
| N10 merge approved Attempt into Lead Branch, serialised | `StructuPath/herdr-swarm` `harvest-step-merge.sh:65-93` (`merge --no-ff` + three-arg `update-ref` CAS, journaled, one slot at a time) | partial | Human-confirmed, not automatic. `spoolway`: "never merges branches". `sean1588`: `gh pr merge --squash` (squash drops commits; CPD wants commits kept). |
| N11 mechanical Gate | `odysseus` (`checks/`, retry with failure feedback); `spoolway` command steps (`pass`/`fail` by exit code, `timeout` 30 m) | partial | Both engine-bound. |
| N12 PR feedback loop to new Jobs | `beads` gates `gh:pr` + `CodeForBreakfast/beady-eye` (`bdi gates`: merge closes gate; failing checks, reviews, comments become comments on the bead) | partial | Produces bead comments, not new Jobs, and waking is Claude-Code-only. `herdr-projects` `pr-followup` routine prompts the *same* thread (`steps.rs:23` poll 120 s). `odysseus` `F` key turns CI/review into a follow-up on the same branch. |
| N13 per-Attempt metrics | `spoolway` (`usage.jsonl` ledger from transcripts incl. `~/.pi/agent/sessions`, `docs/cost.md:14-60`, eval) | partial | Only item that reads pi transcripts. `odysseus`, `agent-grid` read claude/codex only. `dagr stats` computes duration/rework from attempt timestamps. |
| N14 structured user questions, plan approval UI | none | none | `taskr ask --owner --question JSON` is AskUserQuestion-shaped (Claude-oriented). Nothing for pi. |
| N15 visibility | `dagr` (live DAG pane, attempts, causes, evidence); `herdr-beads` / `beady-eye` for bd | full for viewing | `dagr` needs a producer writing `run.json`; `herdr-beads` reads bd directly. |
| N17 agent-agnostic | `herdr-projects` (24 agent kinds, profiles, remote machines) | full | Strong candidate when Claude/Hermes are added (matches `requirements.md` §8). |
| N18 cheap aux agents | `herdr-projects` / `spoolway` per-step model | partial | Just a profile / pipeline step with `H` tier. |
| N19 building blocks | see "Borrow list" below | | |

### Overlaps and conflicts

- **Dispatchers that want to own the whole loop** (pick at most one, or none): `spoolway`, `herdr-board`, `odysseus`, `sean1588/herdr-orchestrator`, `taskr`+`husniadil/herdr-dispatch`. Each has its own state store (TOML task files, SQLite, JSON files, SQLite, SQLite) and its own worktree convention. Mixing two double-books worktrees and concurrency counters.
- `herdr-projects` and `taskr` both write **pane tokens** and want the sidebar (`herdr-projects` `agent.view.set` over the raw socket; `taskr` `taskr_state`/`taskr_campaign`/`taskr_parent`). `herdr-projects configure` rewrites the user's herdr sidebar config; `taskr` expects a separate sidebar plugin. Unverified whether they collide; likely visually.
- `herdr-projects` `[[startup]]` is global: its ticker runs in every herdr session once linked (`docs/herdr-notes.md`, "0.2.0 redesign"). `taskr` also has `[[startup]]` and `[[events]] pane.agent_detected`. Both start a long-lived daemon.
- `spoolway` hard-requires being launched in a herdr pane (`dispatcher.md`: refuses outside one). `herdr-board` and `taskr` run a **daemon** (independent of any pane). `odysseus` runs an engine on demand.
- `herdr-beads`/`beady-eye` expect `bd` writes by agents; embedded Dolt is **single-writer**, so many Workers calling `bd` concurrently hit "database is locked" (`docs/architecture/dolt.md:534-538`, `docs/reference/git-integration.md:208-210`). Server mode needs a `dolt sql-server`.
- Two plugins named `herdr-swarm` (StructuPath = fan-out/harvest plugin; tuanhung303 = bash skill, not a plugin) and two `herdr-orchestrator` (sean1588 = Go daemon, not a herdr plugin; kylezk777 = `orch` CLI, not a herdr plugin; `jpolec/herdr-plugin-odysseus` has plugin id `jpolec.herdr-orchestrator`). Do not confuse them.

### Recommendation for CPD v0

1. **Do not adopt any of these as the engine.** None provides Dependencies *and* Lead-Branch merge *and* fresh-session retry ladder *and* pi per-spawn model+effort. The closest are `spoolway` (static pipelines; deps via branch stacking; ledger reads pi transcripts; no pi effort; never merges) and `odysseus` (deps with a `merged` mode, timeout, retries, policy, draft PR; same-agent retries; usage only for claude/codex; 6★, 2 weeks old). Both would force us to bend CPD to their model. This agrees with `requirements.md` §1 ("adopt, don't build" only where behaviour matches).
2. **N5: beads is viable as the store, with two constraints** (verified by docs, not by running `bd`): (a) only the Lead process writes to `bd` (Workers never call it) to stay clear of the embedded single-writer lock; (b) "Locked" and append-only must be our own guard (wrapper + convention): `bd delete`, `bd edit`, `bd update` and `bd compact` (semantic summarisation of old closed issues) all exist, so never run `bd compact`/`prune`/`purge`. History comes from Dolt commits and `bd history`; the events journal is **off by default and auto-prunes** (7 days / 100k rows) so do not rely on it. Map **Merged = bd `close`** so `bd ready` matches CPD's "Dependency means Merged". Alternative worth weighing: a minimal SQLite ledger with triggers that make Attempts append-only (guess: ~100 lines, no 48 MB Dolt binary). `dagr`'s CONTRACT gives the Task/Attempt/cause/event vocabulary to copy either way.
3. **N2: the half-typed-input risk is real** on herdr 0.9.1 and `agent prompt` docs in 0.9.3 give no protection (`agent-automation.mdx:72`; unverified on 0.9.3). Borrow `herdr-projects`'s approach (wait for idle 60 s + input box empty 10 s via `agent read --source visible`, `prompt_box.rs` knows `pi`) or use a pi-side push (W2's packages; `taskr-push.ts` shows the shape). Default to `notification show` only for anything not provably safe.
4. **N6/N7 must be enforced by code, not by prose** (axiom). `herdr-projects`'s cap is prose only. Borrow `herdr-board`'s `dispatch_pass` shape (serialised pass; count active; claim before launch) and `sean1588`'s no-progress timer (progress events from herdr agent status reset the clock, then confirm against the pane's own output before failing).
5. **N10 is custom.** Borrow `StructuPath/herdr-swarm`'s serialised merge (`merge --no-ff` + `update-ref` compare-and-swap + journal, `harvest-step-merge.sh:65-93`). Keep commits, serial queue, Gate afterwards.
6. **N12:** consider `beads` `gh:pr` gates + `bdi gates` (webhook or 60 s poll) to *detect* PR events; creating the new Job from a failing check stays a Lead action. Unverified that `bdi` works without Claude Code (it is a Rust binary `bdi`; the wake plugin is Claude-only).
7. **N13:** copy `spoolway`'s approach (name the session at start, read the pi session transcript at settle, append one JSONL line). pi session files are under `~/.pi/agent/sessions` (`src/agent.rs:513`).
8. **N15 post-MVP:** `herdr-dagr` is an excellent viewer if the Lead (or an export from bd) writes `run.json` v3; `dagr check --strict` validates it. `herdr-beads` is the zero-glue option over bd.
9. **N17 later:** `herdr-projects` is the coordinator to evaluate when Claude/Hermes arrive (matches `requirements.md` §8). Its profiles ≈ Tiers; its PR follow-up and allow-lists are mature. Limits: one PR per thread, no Dependencies, no merge queue, single maintainer.
10. Skip for v0: `herdr-workflows` (linear YAML, no parallel/loops), `herdr-spreader` (static layouts), `tsk` (personal board, agent execution "coming soon"), `herdr-agent-grid` (no pi metrics), `herdmates` (Claude Code agent teams only), `kylezk777/herdr-orchestrator` (sequential phases, 4★, one day of history), `loud-lead` (empty repo), `tuanhung303/herdr-swarm` (bash skill, 1★).

### Borrow list (N19)

| Pattern | Where | Use |
|---|---|---|
| Input-box emptiness check per harness (ANSI screen parse incl. `pi`, fixtures per agent) | `herdr-projects` `src/prompt_box.rs`, `tests/fixtures/prompt_box/*.ansi` | N2 |
| Nudge gating: idle ≥60 s and box empty ≥10 s, one-line nudge, inbox files as source of truth ("missed prompt loses nothing") | `herdr-projects` `steps.rs`, `coordinator.rs:22-24`, `docs/operations.md` | N2 |
| Deterministic scheduler pass: claim capacity before launching, per-space FIFO | `herdr-board` `crates/board-daemon/src/dispatch/pass.rs` | N6 |
| Conservative restart reconciliation (Unknown ≠ Gone; subscribe before snapshot) | `herdr-board` `docs/design.md:286` | N7/robustness |
| No-progress timer + blocked timer + hard drive deadline (3 independent layers) | `sean1588` `examples/default-pipeline.yaml` (`policies:`) | N7 |
| Serialised merge with CAS ref update and journal | `StructuPath/herdr-swarm` `scripts/harvest-step-merge.sh` | N10 |
| Task/Attempt/cause/evidence/event schema + `check` linter + atomic `.tmp` → `mv` publish | `herdr-dagr` `CONTRACT.md`, `skills/dagr-producer/SKILL.md` | N5/N15 |
| Usage ledger from agent transcripts, session pinned at start | `spoolway` `docs/cost.md`, `src/agent.rs` | N13 |
| Loop budget per step (`loop: N` counts arrivals; cycles must carry a loop) | `spoolway` `docs/pipelines.md:154-196` | N8 |
| Fake herdr binary + backend trait for tests | `herdr-spreader` `tests/fixtures/fake-herdr.sh`, `src/backend/mod.rs`; `herdr-swarm` `tests/harness.mjs`; `herdr-board` `crates/board-core/src/client/fake.rs` | N19 |
| pi extension that forwards a child process's channel notifications via `pi.sendUserMessage(..., {deliverAs: "followUp"})`, kills the process group on `session_shutdown` | `taskr` `adapters/pi/taskr-push.ts:7,146` | N2/N3 |
| Manifest `[[startup]]` launching a detached single-instance daemon via a tiny `run.sh` | `taskr` `plugin/run.sh`, `husniadil/herdr-*` | N19 |
| herdr CLI quirks list (positionals before options; ids reused after server restart; `agent start` fails fast on trust dialog; `worktree open` needs `--cwd`) | `herdr-projects` `docs/herdr-notes.md` | N9/N19 |

## Items

### eliasstravik/herdr-projects
- Meta: ★608 · pushed 2026-09-28 · created 2026-09-18 · MIT · v0.2.34 (35 releases in 19 days) · no npm · SHA `4e4548c`
- What: a coordinator conversation plus parallel worker "threads" (agent in its own worktree/branch, or a tab), shared memory, a ticker, an overview of what needs you.
- Mechanism:
  - Manifest `herdr-plugin.toml`: `min_herdr_version 0.9.1`, `[[build]]` (release binary or `cargo build`), `[[startup]]` (ticker), 10 `[[actions]]`, 4 popup `[[panes]]`. No `[[events]]`; the ticker polls every 15 s instead.
  - herdr CLI used: `worktree create --cwd --branch --base --label --no-focus` (`src/herdr.rs:372-376`), `agent start/prompt/read/focus/list`, `agent prompt --wait --until working --until blocked` (`herdr.rs:456-458`), `pane report-metadata` tokens, `notification show`, `machine list`, `--machine`. Raw socket only for `agent.view.set` (`docs/herdr-notes.md`, Stage 4).
  - Model/effort: `profiles.rs:208-262`: `--model NAME` for every harness; `pi` effort → `--thinking {off,minimal,low,medium,high,xhigh,max}`. `is_model_name` accepts `provider/id:suffix` characters.
  - N2: `steps.rs:17-20, 358-372` (`NUDGE_QUIET_SECS=10`, `box_quiet`), `coordinator.rs:22-24` (`NUDGE_IDLE_SECS=60`), `prompt_box.rs:209-213` (pi editor between the last two rules), `:242` (`knows`: claude, codex, cursor, gemini, opencode, pi). Evidence for the underlying problem: `docs/herdr-notes.md` Stage 2: "A prompt sent while the user has half-typed text does not merge… **Does not hold.**" ⇒ `nudge` defaults to `false`; fallback `notification show`.
  - PR loop: `src/pr.rs`, `steps.rs:23` (`PR_INTERVAL_SECS=120`), routines `on = "pr"` with events opened/checks-failed/review/merged (`routine.rs:127`); default `routines/pr-followup.md` prompts the *thread* to fix checks/comments. Resolve on merge removes worktree (never forced) and branch.
  - Concurrency: `max_parallel_threads` exists as a setting (default 10) but only the coordinator skill reads it (`skill/COORDINATOR.md:42`; no enforcement in `src/` per grep).
  - Timeouts: none per thread. Metrics: none (only sidebar tokens and self-reported progress `report --percent`).
- Maturity: 329 `#[test]` + scenarios against a fake runner (`src/scenarios.rs`), `tests/cli.rs`; one workflow (`release.yml`, no test CI workflow in repo: unverified whether tests run on PR); 31 open issues (several about Codex delivery); 1 contributor; 35 releases in 19 days (high churn). Min herdr 0.9.1.
- CPD fit: N1 partial (profile name, not free-form), N2 partial (best available; heuristic), N9 partial (`--base REF` per thread; worktree placement is herdr's), N12 partial (poll + prompt the same thread), N15 full (sidebar rows, popup), N17 full (24 kinds, remote machines). N5/N6/N7/N8/N10/N11/N13 none. `--kind worktree|tab|checkout`.
- Gaps/risks: no Dependency graph (tasks are a TODO list); one PR per thread; coordinator is a "never does the work" agent (matches Lead) but routes via a skill and a binary named `hp` (conflicts conceptually with CPD's own Lead protocol); rewrites the user's herdr config on `configure`; globally linked `[[startup]]` runs in every session; coordinator answers permission prompts/trust screens through `thread keys` (soft safety, documented); hooks written into `~/.claude/settings.json` etc.
- Verdict: **BORROW** (N2 input-box detection, nudge gating, herdr CLI notes) and **evaluate as REUSE when Claude/Hermes Workers are added**. Not the v0 engine: no Dependencies, merge queue or retry ladder.

### aemrebarut/herdr-dagr
- Meta: ★96 · pushed 2026-08-23 · created 2026-08-14 · Apache-2.0 (+MIT file) · v0.3.1 (4 releases) · SHA `52991f9`
- What: a live DAG viewer for an agent swarm in a herdr split pane, rendering a producer-written `run.json` (contract v3) with attempts, gates, evidence and operator messages.
- Mechanism:
  - Manifest: `[[build]]` (install script), one `[[panes]]` (split, `dagr view`), `[[actions]]` open-dagr {right,down,up,left}. `min_herdr_version 0.8.0`.
  - Socket API: `events.subscribe` held connection for pane lifecycle (`src/herdr.rs:2-18, 237`), `pane.focus`, `agent.focus`, `agent.prompt` (`:493-540`, CLI fallback on Windows). Operator message flow: append raw message to `messages.jsonl`, then `agent.prompt` to the declared `run.orchestrator` pane (`CONTRACT.md` §9).
  - Contract (`CONTRACT.md`): Task (state projection over attempts: queued/working/review/blocked/done/failed/rejected/canceled/settled_unverified), Attempt (`T·aN`, `cause` = initial/sent_back/gate_failed/followup/superseded, `started_at/ended_at`, `model`, `outcome.evidence` verified/reported/heuristic/asserted, `liveness`), `deps`, gate fan-in `inputs`, append-only `events`. "Readiness is derived, not authored" (§3). Stance: "representation kernel, not an enforcement kernel" ⇒ no scheduler, no locking, never mutates run state.
  - `dagr check --strict --json` lints (E1xx/E2xx incl. dependency cycles, attempt/task state contradictions); `dagr stats` = WIP, rework rate, avg attempt duration, critical path (`src/stats.rs`).
- Maturity: 156 `#[test]`, tests in `tests/` (check CLI, skill examples, release readiness); CI on ubuntu+windows (`ci.yml`); 1 contributor; 4 releases; 0 open issues; compat herdr ≥0.8.0.
- CPD fit: N15 full (viewer, post-MVP backlog item in `requirements.md` §8). N5 vocabulary matches CPD's Job/Attempt/cause/Locked closely but it stores nothing: partial as a *schema to adopt*. N13 partial (`dagr stats` derives durations/rework from timestamps the producer writes). N8 vocabulary (`cause: sent_back(by, reason, ref)`).
- Gaps/risks: someone must write `run.json` (the Lead, or an export from the store) and keep it valid; no turn/token/cost fields except `model` and `progress`; pane is a read-only view.
- Verdict: **BORROW** the contract as the Job/Attempt/event vocabulary now; **REUSE** the pane for N15 when we get to a DAG view.

### nelsonPires5/herdr-board
- Meta: ★175 · pushed 2026-10-09 · created 2026-07-14 · **no license file** (GitHub: none) · v0.19.0 (22 releases) · SHA `754c836`
- What: kanban TUI + `boardd` daemon; a card is a prompt, a column is a pipeline stage; moving a card into an `auto` column starts an agent in a visible herdr pane.
- Mechanism:
  - Manifest: `[[build]]` ×2, overlay `[[panes]]` (`board tui`), action `open-board`. `min_herdr_version 0.9.0`; runtime requires herdr socket protocol 22.
  - Socket client (`crates/board-herdr/src/client.rs:44-63`): `workspace.create`, `agent.start/get/prompt/wait`, `pane.*`, `events.subscribe` (`docs/herdr.md:124`). No herdr worktree API use (own worktrees: none; "space" = workspace + cwd).
  - Harness: pi is the default; `--thinking` pushed through `harness/mod.rs:407, 535`; card carries harness/model/effort/permission (`docs/design.md:23`).
  - Concurrency: `config.max_concurrent` (default 3, `docs/configuration.md:13`), enforced in `dispatch_pass` (`dispatch/pass.rs:11-60`) with per-space FIFO; tests `dispatch/tests/concurrency.rs`.
  - Timeout: column `timeout_minutes` (`board-core/src/model.rs:78`), deadline/pause accounting (`:221-224`), idle watcher `tick_ms`.
  - History: Card ↔ Run separation, retries = new runs (`docs/design.md:25-27`); SQLite schema v15; "retry re-attaches to the same conversation" (`docs/configuration.md:122`).
- Maturity: ~1,200 tests (unit + 40 live E2E scenarios, Docker sandbox, `docs/testing.md`), CI + release workflows, 4 contributors, 13 open issues.
- CPD fit: N1 partial (per card model + thinking for pi), N6 full (global cap), N7 partial (per-column), N15 partial (board), N13 none (runs table has exit status/result summary, not turns/tokens). N5 none (no dependencies; stages only). N9/N10/N11 none.
- Gaps/risks: no license; daemon plus separate board DB; card = prompt, not Spec; no fan-in/Dependencies (as already noted in `requirements.md` §5); no completion nudge to a Lead (human-facing board).
- Verdict: **BORROW** dispatch-pass and restart-reconciliation patterns; **SKIP** as a dependency (no license, no graph).

### olafurns7/herdr-taskr
- Meta: ★0 · pushed 2026-10-12 · created 2026-10-02 · MIT · v0.18.0 (13 releases in 10 days) · SHA `ef9f291`
- What: a task ledger and hub for agent fleets: orchestrators register lanes, workers report/ask through it, a blocking `wait` wakes the orchestrator; TUI for the owner; multi-host via a hub.
- Mechanism:
  - Plugin manifest is in `plugin/herdr-plugin.toml`: `[[startup]]` `run.sh` (detached single-instance daemon) and `[[events]] on = "pane.agent_detected"` as watchdog; min herdr 0.9.0. It writes pane/workspace tokens `taskr_state`, `taskr_round`, `taskr_owner_ask`, `taskr_campaign`, `taskr_parent`.
  - Ledger: SQLite (`schema.sql`: `tasks` with `parent_id` only, **no dependency table**; `events` table `schema.sql:26-40` with `related_event_id`, `answered_by`, `event_key` unique). Go CLI (`taskr`) + Rust crates (`taskr-core`, `taskr-tui`).
  - Worker verbs (`SKILL.md`): `got`, `note`, `ready --report`, `ask "q" --blocking [--owner]`, `wait --for answer`, `done|fail`. Orchestrator: `new`, `launch --provider --model --effort` (records), `prompt --file` (delivered via herdr; 300 s receipt deadline → `prompt_outcome no_receipt`), `wait --for ready,ask,done,...`, `answer`, `ack`, `close --outcome accepted|reworked|rejected|abandoned` (`docs/cli.md`, `references/orchestrator.md`).
  - pi: `adapters/pi/taskr-push.ts`: starts `taskr mcp --channel`, turns each `notifications/claude/channel` into `pi.sendUserMessage(content, {deliverAs: "followUp"})` (`:146`), kills the process group on shutdown; flags `--taskr-bin/--taskr-root/--taskr-session`.
  - Capacity: `taskr slotr` shows another tool's (slotr's) pools (`docs/cli.md:123`; slotr itself is not in this repo: unverified).
- Maturity: ~485 Go tests, scenario/golden tests; **no CI directory in the repo** (`.github` absent); 1 contributor, 0 stars, 10 days old; README says "Early software; interfaces may change".
- CPD fit: N3 partial-to-full mechanism (blocking ask/answer with ack, idempotent keys, receipts), N2 partial (pi push shim exists; interaction with the User's editor draft unverified), N5 partial (parent/child, events, no deps/ready set), N13 none (usage unknown), N15 full for owner view.
- Gaps/risks: orchestrator `wait` is an agent-blocking call (the Lead would be blocked unless pushed via the shim); dense token-minimised output formats; hub/daemon complexity; claude/codex-centric.
- Verdict: **BORROW** (pi push shim, ask/answer/ack and receipt design) and watch; **SKIP** as a dependency (0★, no CI, no dependency graph).

### StructuPath/herdr-swarm
- Meta: ★9 · pushed 2026-10-04 · created 2026-07-22 · MIT · no release tag (package.json 0.5.0; manifest version 0.5.0) · SHA `f750bc7`
- What: worktree-per-agent fan-out, status pane (committed/uncommitted counts vs fork point), and a review-first harvest/merge pane. Race model: N agents on one task, pick a winner.
- Mechanism: manifest id `structupath.swarm`, `min_herdr_version 0.7.4`; 5 actions (fanout/status/harvest/abort/prune) + 2 split panes. Fan-out: `agent start --cwd` on 0.7.4; on 0.7.5+ `pane split` + `pane run` + `pane report-agent` (README "Requirements"). Slots get copy-on-write deps, own ports, run-unique branch from a recorded base SHA. Harvest: `git merge --no-ff` (`scripts/harvest-step-merge.sh:65-67`), three-arg `update-ref` compare-and-swap, journaled, drift-checked (`:72-93`, `scripts/preflight.sh:71-98`), conflict-resolver agent, `validate.sh` per slot.
- Maturity: Node tests under `tests/` (~19 files), CI workflow, 0 open issues, tiny community; sibling plugins (Guard, Conductor, Browser) exist (see Additional finds).
- CPD fit: N9 partial, N10 partial (best merge discipline seen, but human-triggered and one-shot), N11 partial (`validate.sh`). Dependencies: none (slots are independent). Pi: unverified (presets are your own argv).
- Gaps/risks: race-and-pick model; Node ≥20; 0.7.5+ path reports state itself (`pane report-agent`), so herdr stops reading the screen for those slots.
- Verdict: **BORROW** the CAS merge + journal pattern for N10 and the copy-on-write dependency seeding for N9; **SKIP** the plugin.

### sean1588/herdr-orchestrator
- Meta: ★7 · pushed 2026-10-12 · created 2026-06-29 · Apache-2.0 · v0.2.0 (2 releases) · SHA `1535a37`
- What: Go daemon `orchestratord` that turns labelled GitHub issues into merged PRs: triage → implement (agent in worktree) → PR → review agent → loop → squash merge once checks green. **Not a herdr plugin** (no `herdr-plugin.toml`); it drives herdr by CLI.
- Mechanism: one YAML pipeline (`examples/default-pipeline.yaml`): `max_concurrent_tasks: 4`, `retry_caps.changes_requested: 3`, `no_progress_timeout: 30m`, `blocked_timeout: 10m`, `drive_deadline`, per-state `timeout`, `dry_run`. Dependencies come from GitHub's `blockedBy` (`internal/github/gh.go:51`). herdr use: `git worktree add` itself, `pane run`, `agent wait --until idle/blocked`, `agent prompt --wait`, `agent explain` (`internal/exec/herdr.go:85-98, 176, 204, 227, 254`); state in SQLite (`modernc.org/sqlite`); MCP control surface; merge = `gh pr merge --squash` (`internal/github/merge.go:20`); validator refuses configs that can reach merge other than via the checks gate. Agents are `launch:` argv per role (claude/codex/grok; no per-spawn model).
- Maturity: ~360 Go tests, CI + release workflows; dogfooded on its own issues; 6 open issues; 1 contributor; tested on herdr 0.8.2–0.9.3 (README), the only item here with explicit 0.9.3 coverage.
- CPD fit: N6/N7 full in its engine, N11 partial, N12 partial (CI-green gate; review rounds), N5 partial (GitHub issue graph as the job graph), N10 not as wanted (squash per PR, no Lead Branch).
- Gaps/risks: unit is a GitHub issue→PR (CPD is many Jobs → one PR); squash drops commits; roles launch argv fixed; Claude Code is the only dogfooded agent.
- Verdict: **BORROW** the three-layer liveness bounds and "merge decided by GitHub signals" validator idea; **SKIP** otherwise.

### aorumbayev/herdr-workflows
- Meta: ★31 · pushed 2026-09-26 · created 2026-07-20 · MIT · v0.15.1 · SHA `140de99`
- What: linear YAML workflow runner (`run:` / `agent:` / `herdr:` / `workflow:` steps) with `hwf` CLI, picker/console panes, run history.
- Mechanism: manifest `launch` action + `picker`/`editor`/`console` panes, `min_herdr_version 0.8.2`. `docs/guide.md:3`: "There are no loops and no parallel groups." `agent:` step turn deadline default 30 m (`docs/reference.md:73`); `retry:` for commands/`herdr:` calls, "never agents" (`:116, 270`); `hwf run --detach --json`, `hwf runs list --json`, `hwf retry <id> --from-failed` (`docs/surfaces.md`).
- Maturity: ~870 Go tests, e2e suite, golangci, goreleaser, 0 open issues; 30 commits seen; docs site.
- CPD fit: none of N5/N6/N8/N10; N7 partial (30 m turn deadline = same default as CPD). Could script a Gate (`run: [verify]`), N11 partial at best.
- Verdict: **SKIP** (linear by design). Pattern note: its YAML schema + deadline defaults match CPD's 30 min Attempt timeout.

### marvingygas/spoolway
- Meta: ★8 · pushed 2026-10-12 · created 2026-08-29 · MIT · v0.10.0 (11 releases) · npm `spoolway` 1,586 downloads last month · SHA `a0ab061`
- What: a model-free dispatcher (Rust TUI/CLI) that drives Markdown task files through YAML pipelines of agent and command steps, one worktree per task; supports `claude`, `codex`, `pi`.
- Mechanism:
  - Manifest: 3 panes (init popup, dispatch tab, doctor popup), actions; `min_herdr_version 0.7.0`. The dispatcher refuses to run outside a herdr pane (`docs/dispatcher.md` "Running it"). herdr use is the CLI: `pane run`, `pane list/layout/process-info`, `agent list`, `workspace list`, `worktree remove --workspace --force` (`src/mux.rs:846-1805`). Worktrees are created by spoolway itself (`~/.spoolway/worktrees/<task>` branch `task/<id>`).
  - Task = Markdown with frontmatter (`id`, `title`, `group`, `pipeline`, `base`, `depends_on`, `starts_from`) (`README.md` "A task is what travels the line"). Ready when dependencies are archived (done) (`docs/dispatcher.md:80`); a dependent task is cut **from its first dependency's branch** (`:123`); stacks of PRs via `spoolway stack`; "spoolway never merges branches" (`docs/concepts.md:237`).
  - Caps: `[models."<glob>"].slots`, `agents.<profile>.concurrency` (`docs/configuration.md:678, 723`; `dispatcher.md:108`). Timeouts: command steps `timeout` default 30 m (`docs/pipelines.md:93`); agent lanes get reminders after `lane_quiet` 15 m and escalate after 3 answered reminders (`configuration.md:299-310`); no hard agent timeout (unverified beyond docs).
  - Retries: `on_fail`, `loop: N` arrivals per step (`pipelines.md:34-44, 91-92, 154-196`); same pipeline, static model per step; `unattended` mode with unblocker agent and `max_cost_usd`/`max_output_tokens` (`configuration.md`).
  - pi adapter (`src/agent.rs:470-499`): passes `--model`, `--no-approve`, `--session-id`; **effort not wired for pi** (docs/agents.md "Effort": `pi | not supported`; code comment: "`--thinking` takes a token budget, not a named level"). pi 1.1.0 documents `--thinking <off|minimal|low|medium|high|xhigh|max>` and `--model provider/id:thinking` (`pi docs/cli.md:65-70`) so that comment is stale for 1.1.0. Whether a `model:` value of `provider/id:level` passes through unmodified is unverified.
  - Metrics: usage ledger `usage.jsonl` read from pi/claude/codex transcripts per lane with tokens, cost, wall time, outcome, pipeline version (`docs/cost.md:14-60`, `docs/eval.md`); `spoolway-calibrate` skill reads them.
- Maturity: ~3,160 tests (`#[test]` count), CI (ci, verify, release, pages), 12 open issues, 2 contributors (author has 1,073 commits), 11 releases in 6 weeks, npm package.
- CPD fit: N5 partial (deps, states, `queue`/`blocked`/`paused`; history is files + ledger, not enforced append-only), N6 partial (per model/profile slots, no global cap seen: unverified), N7 partial (command steps only), N8 none (static pipeline; no effort/model escalation; pi effort missing), N9 partial (own worktree from dependency branch, not Lead Branch), N10 none (never merges), N11 full (command step), N13 full-ish (best ledger; pi transcripts), N15 partial (TUI board), N18 full (any step may use a cheap model).
- Gaps/risks: dispatcher is a TUI that must live in a herdr pane (not headless); bundled workflow DSL we would have to bend to CPD; Markdown task files are the store; two-person project; large surface (`cli-reference.md` 1,728 lines).
- Verdict: **BORROW** (usage ledger from pi transcripts, `loop:` budgets, unattended cost caps, per-model slots). Closest overall architecture to CPD but not adoptable without bending CPD; **SKIP** as engine.

### jeffarese/herdr-agent-grid
- Meta: ★7 · pushed 2026-10-07 · created 2026-10-04 · MIT · v2.0.2 · SHA `67860ba`
- What: Rust full-panel dashboard of herdr agents (status, latest call, tokens, API cost, subagents).
- Mechanism: popup pane via `plugin pane open`; `pane.read`, `agent.focus`, session-log parsing (`src/sessions.rs`, `costs.rs`). Metrics sources doc (`docs/metrics.md`): Claude and Codex session logs only; the only pi mention is an icon (`src/visuals.rs:107`).
- Maturity: 2 unit tests; CI `test.yml`; 1 open issue; 3 days old at creation-to-push.
- CPD fit: N15 partial; N13 none for pi (Claude/Codex only).
- Verdict: **SKIP** (W5 territory; no pi cost).

### tuanhung303/herdr-swarm
- Meta: ★1 · pushed 2026-08-03 · created 2026-08-03 · MIT · no release · SHA `1f59f87`
- What: bash scripts + skill to spawn grids of peer agents (Hermes, Codex, Cursor, OpenCode, Claude, AGY) in a `swarm:<topic>` tab, with a `comm.md` mailbox, `swarmctl.sh wait/collect`. Not a herdr plugin (no manifest). Needs `HERDR_ENV=1`.
- Maturity: 19 files, bash tests against `fake-herdr.sh`; 1 star.
- CPD fit: N17 curiosity (Hermes support) only; no pi.
- Verdict: **SKIP**.

### eighteyes/loud-lead
- Meta: ★0 · pushed 2026-10-09 · created 2026-10-09 · no license · GitHub description: "Lead a visible herdr team of one-job Claude Code help sessions; self-improving from a shared lessons log." · repo is **empty** (git clone warns "empty repository"; API size 0).
- Verdict: **SKIP** (nothing to read; unverified).

### caioniehues/herdmates
- Meta: ★26 · pushed 2026-08-21 · created 2026-07-15 · MIT · v1.0.0 (manifest 3.2.0) · SHA `f149455`
- What: hosts Claude Code **native agent teams** in herdr: a fake `tmux` shim (`teammux`) translates Claude's tmux verbs into `herdr pane` CLI calls; hook-driven signal engine, durable recorder, sidebar tokens.
- Mechanism: depends on Claude Code `teammateMode: tmux`, team files and hooks; Rust, ~320 tests.
- CPD fit: N17 (a Claude-Code-native path, nothing for pi); native team `blockedBy` DAG is Anthropic's, not ours.
- Verdict: **SKIP** for v0; note as the Claude-native alternative if Claude Workers arrive.

### kylezk777/herdr-orchestrator
- Meta: ★4 · pushed 2026-07-26 · created 2026-07-26 · MIT · no release · SHA `445d756`
- What: `orch` Rust CLI (not a herdr plugin) with file-based tasks: planner/orchestrator/operator roles, sequential phases with hard LGTM gates, `allowed_paths` scope checks, decisions append-only/superseded (`README.md` "Gate rules").
- Maturity: ~133 tests; 1 day of history; Chinese+English docs.
- CPD fit: N11/N5 ideas only (scope check on reviewed diff, append-only decision log); phases are sequential, not a DAG.
- Verdict: **SKIP** (BORROW idea: scope-check the diff against the Spec's "files it touches" at review time).

### smarzban/tsk
- Meta: ★197 · pushed 2026-10-12 · created 2026-08-19 · MIT · v0.11.6 · SHA `c992b6f`
- What: shared task board TUI/CLI for you and your agents (queue, notes, steps), herdr plugin (`herdr-tsk`) with a board pane and quick capture; skill for pi, omp, claude, cursor, grok, codex, opencode.
- Mechanism: manifest `panes` (split `./target/release/tsk`), action `open-board`; statuses open/ready/started/blocked/review/done (`CONTEXT.md:7`); "Coming soon: agent execution in Herdr worktrees" (`README.md`). ~540 tests, 10 open issues.
- CPD fit: no dependency graph found in CONTEXT.md/README (unverified in code), no spawn; the "NEEDS YOU" idea is N14-ish for a human.
- Verdict: **SKIP**.

### miiraheart/herdr-beads
- Meta: ★37 · pushed 2026-10-08 · created 2026-07-23 · MIT · no release (manifest 0.1.0) · SHA `72f59e1`
- What: List/Table/Kanban view over `bd` issues, docked sidebar or popup.
- Mechanism: shells out to `bd list --json` (`src/bd/mod.rs:104-106`), follows `bd events tail --follow` when the events journal is enabled (`src/bd/live.rs:399-470`; the journal is off by default, so otherwise it polls), writes via `bd update/close/create/note/comment`; `[[events]] on = "tab.created"` (auto-dock opt-in). Resolves the repo from the focused pane's cwd via `herdr pane list`. `min_herdr_version 0.7.0`.
- Maturity: 20 commits; selftest + fixtures (`tests/fixtures/*.json`); CI; 2 open issues.
- CPD fit: N15 full over a bd store (post-MVP "beads board" in `requirements.md` §8). Display only, no Job semantics.
- Verdict: **REUSE** for N15 if beads is chosen (low risk: read-mostly).

### steveyegge/beads (`bd`; repo redirects to `gastownhall/beads`)
- Meta: ★27,828 · pushed 2026-10-12 · created 2025-10-12 · MIT · v1.3.1 stable (2026-09-30), v1.3.2-rc.2 prerelease (2026-10-10) · npm `@beads/bd` downloads: unknown (API rate-limited) · SHA `a2d43e6` · 4,803 commits by the owner, 776/616/514 by next three; ~2,160 Go test files; Bazel + GitHub workflows · darwin_arm64 release asset 48 MB (`beads_1.3.1_darwin_arm64.tar.gz`).
- What: a distributed, dependency-aware issue graph for agents, stored in Dolt.
- Evidence by CPD question (docs/source only; `bd` not run):
  - **Dependencies / ready set:** `bd dep add child parent`, types `blocks` (default), `parent-child`, `conditional-blocks`, `waits-for` (blocking) vs `related`/`tracks`/`discovered-from`/... (`docs/core-concepts/dependencies.md`). `bd ready` = no open blocking deps, with `--explain`, `--priority`, `--label`, `--unassigned`, `--json` (`docs/cli-reference/ready.md`). `bd blocked`, `bd dep tree`, `bd graph`. Ready means blockers are **closed**; so "Merged = close" must be our convention.
  - **Claim:** `bd update <id> --claim` is atomic (sets assignee + in_progress; idempotent for the same actor; typed `ClaimConflictError` otherwise) (`docs/cli-reference/update.md:29`, `claim.go`, `issueops/claim.go`).
  - **History/audit:** Dolt auto-commit after writes (embedded default on), `bd history <id>` (`docs/CLI_REFERENCE.md:1622`), `bd audit` appends to `.beads/interactions.jsonl` ("Entries are append-only", `docs/cli-reference/audit.md`), optional **events journal** (`bd events tail`, off by default; floors 7 days / 100k rows with auto-prune on: `docs/reference/events-journal.md`).
  - **Custom fields:** free JSON `metadata` with `--set-metadata k=v`; documented execution-hint convention `execution_suggested_model`, `execution_reasoning_effort`, `execution_agent_type`, `execution_parallel_group` (`docs/core-concepts/metadata.md`), advisory not binding. Custom statuses via `bd config set status.custom "name:category,..."` with categories active/wip/done/frozen controlling `bd ready` (`docs/cli-reference/statuses.md`). Attempts could be child issues or metadata/comments (our modelling choice; no first-class Attempt type).
  - **Enforceability of append-only/Locked:** none. `bd edit`, `bd update`, `bd delete`, `bd compact` (semantic summarisation of old closed issues), `bd prune/purge` exist (`docs/cli-reference/`, `README.md`). Hooks `.beads/hooks/on_update` run after the mutation, async, fire-and-forget: cannot veto (`events-journal.md` comparison table). We would need a wrapper and a rule "never call compact/prune". Dolt history remains underneath, which gives forensic recovery.
  - **Concurrency / footprint:** embedded Dolt is in-process, **single writer via file lock** ("database is locked"; `docs/architecture/dolt.md:534-538`); concurrent writers need server mode (`bd init --server`, external `dolt sql-server`); all worktrees of a repo share one `.beads` (`docs/reference/worktrees.md`, `git-integration.md:208`). Go module pulls `dolthub/dolt/go`, `go-mysql-server`, `driver/v2` (`go.mod:17, 149-154`); 48 MB compressed binary. Cross-machine sync via `bd dolt push/pull` to `refs/dolt/data`.
  - **Gates:** `bd gate` types human/timer/`gh:run`/`gh:pr`/bead (`docs/cli-reference/gate.md`) bridge PR/CI events into the graph; `bdi gates` (beady-eye) is the poller/webhook that closes them.
  - **Integrations:** `bd setup` for Claude/Codex/Cursor/etc., `bd prime` for agent context; no pi-specific integration under `integrations/`, but pi reads `AGENTS.md`.
- Maturity: very high (project age 1 year, daily releases, large community), but fast-moving (schema migrations: "exactly one designated clone runs `bd migrate schema`" per README) and the repo recently moved org (`steveyegge` → `gastownhall`).
- CPD fit: N5 full for graph/ready/claim/audit, partial for append-only/Locked; N12 partial via gates; N15 via herdr-beads/beady-eye/dagr export.
- Gaps/risks: binary size and Dolt complexity for a "small state store"; single-writer constraint; append-only not enforceable; semantics of "closed" vs "Merged"; schema/CLI churn (rc releases weekly).
- Verdict: **REUSE with a Lead-only writer** (or swap for a thin SQLite ledger if the Dolt footprint is unwelcome: guess, not measured). Run a spike before committing: unverified items are lock behaviour under two quick successive `bd` calls and `bd history` output shape.

### yuk1ty/herdr-spreader
- Meta: ★139 · pushed 2026-08-16 · created 2026-07-03 · MIT · v0.2.1 · SHA `206ae55`
- What: apply a tmuxinator-style YAML layout (workspaces/tabs/panes/commands/`wait_for`) via the herdr CLI.
- Mechanism: manifest `apply` action, `min_herdr_version 0.7.0`; `src/backend/mod.rs` trait with CLI backend and `tests/fixtures/fake-herdr.sh` integration test.
- CPD fit: none (static layouts). Pattern value: strict unknown-key rejection, dry-run plan, fake herdr in tests (N19).
- Verdict: **SKIP** (BORROW test-harness pattern).

## Additional finds

Method: `/tmp/herdr-index.json`, ≥5★, orchestration/workflow/task keywords, minus the other clusters' areas (pi↔herdr subagents, pi workflow engines, review/diff/worktree helpers, observability/notify/safety). 75 candidates matched the keyword filter; those relevant to CPD needs are below. Excluded as other clusters: `osolmaz/pi-workflows` (W3), `devashish2203/herdr-worktrunk`, `tdi/*` worktree plugins, `lamngockhuong/herdr-worktree-setup`, `wyattjoh/*`, `tomasvarga/*`, `plannotator/*` (W4), `ribbons-digital/pi-herd`, `July24/pier`, `jeffory/herdr-walkietalkie` (W2), `hhdebb/herdr-radar`, `Davidcreador/herdr-token-dashboard`, `eliasstravik/herdr-agent-progress`, `madarco/agentbox-herdr-plugin`, `e2b-dev/herdr-e2b-sandbox`, `AltanS/collie`, `powerfooI/roamgate` (W5). Left unreviewed: `a2u/herdr-jira`, `talent-factory/herdr-linear`, `JacquesvanWyk/herdr-linear`, `ogulcancelik/herdr-plugin-github-start` (issue→agent starters, no graph), `ivanarama/PromptPilot` (queue for Claude Code, web/Telegram), `simoncrypta/bercail`, `GavinTomlins/herdr-oh-my-agent`, `flowy11/agent-panel`, `marcvermeeren/chatter`, `agentwireprotocol/awp`.

### jpolec/herdr-plugin-odysseus (plugin id `jpolec.herdr-orchestrator`)
- Meta: ★6 · pushed 2026-09-25 · created 2026-09-23 · MIT · no release (manifest 0.4.0) · SHA `e739fa5`
- What: governed workflow layer: task → isolated worktree → agent → checks (retry with feedback) → independent review → policy check → approval → draft PR → hash-chained audit.
- Mechanism: manifest `[[startup]]` + actions (open, new-task, approvals, install-cli), `min_herdr_version 0.9.0`; file store with locks (`src/store`), engine/scheduler (`src/engine/scheduler.rs`), runners incl. `pane`, `headless`, `fake`, per-agent profiles with `model`/`effort` (`src/runners/profiles.rs:39-44, 147-158`), 24 agent kinds list (`:58`).
  - Dependencies: `depends_on` on tasks (`src/model.rs:262`); `dependency_state` waits until the dependency's head is an ancestor of the base (mode `merged`) or starts from its branch (stacked) (`src/epic/engine.rs:529-560`; `config/mod.rs:409`). ADR→tasks decomposition with acceptance criteria.
  - Limits: `scheduler.max_parallel_runs: 4`, `max_parallel_agents: 8` (`config/mod.rs:107-117`); `agent_timeout` 45 m, `limits.max_retries` 3 (`:127, 133, 145-149`); watchdog for stuck agents.
  - Retry: failing check sends trimmed log to the **same live agent**; reviewer gate sends findings back; tournament mode ×N.
  - Metrics: tokens per task and step for Claude/Codex from their session logs (README); no pi (unverified).
- Maturity: 115 `#[test]`, CI workflow, 0 open issues, **0 releases**, 1 contributor, 2 weeks old, JSON schemas for config/policy/workflow.
- CPD fit: N5 partial (deps with merged semantics), N6 full, N7 full, N8 partial (same-session retry, not fresh+escalation), N9 partial (worktree per task), N11 full, N12 partial (follow-up key `F`), N10 none (draft PR per task, not Lead Branch).
- Verdict: **BORROW** (policy/approval design, `dependency_state` rules, audit chain); watch. Closest in scope to CPD's Dispatch, but a different retry and PR model and too young.

### CodeForBreakfast/beady-eye (`bdi`)
- Meta: ★16 · pushed 2026-10-12 · created 2026-09-06 · Apache-2.0 · v0.26.1-rc.1 (≥26 releases) · SHA `19bff58`
- What: one viewer/watcher over many beads trackers: draws trees of work, shows the herdr pane of the agent that claimed a bead, `bdi watch` notifies subscribers when a bead changes, `bdi gates` settles `gh:pr`/`gh:issue`/`gh:release` gates (merge closes gate; failing checks, conflicts, reviews, comments become bead comments; webhook via `--listen`).
- Mechanism: manifest pane `eye` (split) + action `open`, `min_herdr_version 0.9.1`; `herdr/fetch.sh` downloads a binary; Rust; `plugin/` (Bun/TypeScript) is the Claude Code wake plugin (`plugin/bun.lock`), `docs/agents.md` for agents. `README.md:320-380`.
- Maturity: ~76 test files (`tests/*.rs`), CI, 3 open issues, mutation testing config.
- CPD fit: N12 partial (the PR-signal mechanism), N15 full over bd, N5 view only.
- Gaps/risks: wake-up is Claude-Code-only (unverified for pi); comments rather than new Jobs; requires its own watcher process.
- Verdict: **REUSE** candidate for N12 detection and N15 if beads is chosen; confirm pi wake path in a spike.

### darjss/herdr-orchestrate
- Meta: ★7 · pushed 2026-07-13 · created 2026-07-10 · MIT · v0.1.2 · SHA `281a462`
- What: pi-native orchestration: pi package + herdr plugin; `orch` CLI with run board, durable prompts/reports, isolated worktrees, three hard-coded model routes (`default`, `fast`, `explore`) with optional `--thinking`, background waiting (`orch wait` through pi's background-command tool so pi can end its turn and wake on settle, `README.md:63-75`).
- Mechanism: manifest actions doctor/status/reconcile (`orch ...`), skills `orchestrate`, `orch-implementation-worker`, `orch-research-plan-worker`; TypeScript (vite, bundled `dist/`).
- Maturity: 29 files, no tests found (unverified), 0 open issues.
- CPD fit: N1 partial (pi + thinking; routes fixed to specific providers: `openai-codex/gpt-5.6-*`, `opencode-go/deepseek-v4-flash`, not our `github-copilot/claude-*`), N2 partial (background wait trick; W2 territory).
- Verdict: **BORROW** (the "wait through pi's background command" idea); **SKIP** otherwise.

### husniadil/{herdr-tasks, herdr-dispatch, herdr-mail, herdr-sched} (one design family, Go, "shared plugin contract" v0.11.0)
- Meta: tasks ★3 (v0.10.1, SHA `305c9ac`), dispatch ★1 (v0.10.7, `852f941`), mail ★0 (v0.6.4, `5d32a38`), sched ★5 (v0.2.4, `c68ad65`); all MIT, created 2026-08-21..24, pushed 2026-08-30/31.
- What: `htask` board with claims + renewable leases, evidence-backed review by someone other than the author, human decision gate (`README.md`); `hdis` dispatcher: a worker pane per ready task, `max_workers`, claim timeout, start timeout; `hmail`: notify / ask / reply with tracked obligation; `hsched`: cron and triggers firing actions as `cron:<id>` principals.
- CPD fit: closest *design* match for Dispatch (ready → worker → review gate → done), but **claude/codex only** in dispatch (`provider = "claude"|"codex"`, `--agent`), tasks have no explicit Dependency model seen in README (`htask list --ready` exists; unverified), and the stars/age are minimal.
- Verdict: **SKIP** for v0; read `herdr-tasks` for lease/evidence/review-by-other-session design and `herdr-mail` for ask/reply obligations.

### bon5co/bermuda
- Meta: ★12 · pushed 2026-10-12 · created 2026-08-29 · MIT · v3.6.1 · SHA `1831624`
- What: harness for Claude Code on herdr: **flows** (steps run as separate agents so none can be skipped), **overwatch** (decides retry/jump back/stop), **jobs** on a clock, **claims** (leases), threads, checklists, A.LOG, forum, memory. Claude Code only ("Orchestration under Claude Code").
- CPD fit: N8 idea (overwatch decides retry/jump), N7 none; Claude-only.
- Verdict: **SKIP** (BORROW the idea of an overwatch agent for the retry decision).

### Northern-Lighthouse/herdr-fleet
- Meta: ★7 · pushed 2026-08-14 · created 2026-08-14 · MIT · no release · SHA `2cbefee` · 9 files
- What: tailnet fleet manager; `fleet dispatch` places agents on the machine with most RAM/load headroom and refuses with per-machine reasons; diskless shallow clones.
- CPD fit: N6 (capacity-aware, across machines) as a post-MVP idea; per-machine cap is simpler (`requirements.md` §6).
- Verdict: **SKIP** (idea: capacity from RAM headroom rather than fixed count).

### nidhi-singh02/agent-router
- Meta: ★113 · pushed 2026-09-27 · created 2026-09-17 · MIT · v0.1.0 · SHA `fb24d06`
- What: CLI picks Cursor/Claude/Codex/OpenCode + model/effort by quota and a remote ranking service (TypeSafe API key mandatory, "no fallback"), then launches in a herdr pane. No pi.
- Verdict: **SKIP** (sends task text to a third-party API; pi unsupported).

### DnzzL/herdr-automations, husniadil/herdr-sched, Phoobobo/herdr-workboard, KokiKono/herdr-kanban, detailles/AgentRadio
- `DnzzL/herdr-automations` (★16, v0.8.2, MIT, SHA `2bae3f9`): cron prompt + model + fresh git worktree per run, Go; schedules, not Jobs. SKIP.
- `husniadil/herdr-sched`: see family above. SKIP.
- `Phoobobo/herdr-workboard` (★7, no license): board = workspace, column = tab, task = pane; no deps. SKIP.
- `KokiKono/herdr-kanban` (★5, no license, SQLite, tabs per task): SKIP.
- `detailles/AgentRadio` (★5, v0.7.1, MIT, Python stdlib): local message bus (join, DM, pushed replies into panes, offline delivery). N3 alternative to taskr/mail; W2 overlap; unreviewed in depth.
