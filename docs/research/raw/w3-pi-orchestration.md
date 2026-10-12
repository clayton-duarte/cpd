# W3 pi orchestration, workflow engines and agent coordination: research report

Read at: 2026-10-12 (pi 1.1.0 installed locally). Items reviewed: 14 of 14 listed (incl. @quintinshaw/pi-dynamic-workflows, which replaced the 404 repo), plus 5 additional finds (mu, taskplane, pi-crew, pi-worker-graph, pi-claude-bridge) and a not-reviewed list. Items not reviewed: none of the listed ones. Per-item caveats are inline.

Corrections to the cluster file (verified):
- `QuintinShaw/pi-dynamic-workflows` returns 404 on GitHub (repo and user gone). The package exists on npm as **`@quintinshaw/pi-dynamic-workflows` 3.14.0** (≈24k downloads/mo). I read it from the npm tarball (no tests shipped). The repo I cloned instead, `Michaelliv/pi-dynamic-workflows`, is the 1.0.1 prototype it descends from.
- npm `pi-workflows` (24 downloads, no repo) is **not** osolmaz's package. The real package is `@osolmaz/pi-workflows` (6,225/mo).
- `pi-intercom` has no `repository` field in npm. The code is `nicobailon/pi-intercom` (same maintainer, `nicopreme`; matching name and version 0.17.0). I read the GitHub clone and the npm tarball exists with the same file list.
- `kontextmind/kxm` publishes as `@kontextmind/kxm`; `Akagilnc/ak-pi-workflow-roles` as `@akagilnc/pi-workflow-roles`.

## Summary

### Best candidate per need

| Need | Best item | Coverage | Notes |
|---|---|---|---|
| N1 spawn Worker, model+thinking per spawn | nicobailon/pi-subagents (in-process / detached SDK runner); tintinweb/pi-subagents via `pi.events` RPC | partial | Both take `provider/id:thinking` per call. Neither spawns into a herdr pane for local work. Herdr-pane spawn exists only in pi-subagents for remote saved machines, and in `@piewf/herdr` (every workflow agent in a pane). mu spawns a pi in a herdr pane but cannot pass `--model` (see mu; contradicts herdr docs, unverified). |
| N2 non-blocking completion, no input clobber | nicobailon/pi-subagents `parent-wake.ts`; pi-intercom `idle-wake.ts` | full (by code; not runtime-tested) | Both append the notice with `pi.sendMessage` and wake an idle parent with `pi.sendUserMessage(..., {deliverAs:"steer"})`. Neither writes the editor. They share a `Symbol.for("pi.idle-wake.v1")` reservation so two extensions do not double-wake. |
| N3 Escalation (child asks parent, waits) | pi-subagents `contact_supervisor` (native) and pi-intercom `ask`; gentle-shell `subagent_parent_message kind:"query"` | full | pi-subagents: blocking `need_decision` and `interview_request`, nested A→B→C scoping. pi-intercom `ask`: blocks, 10 min default timeout. |
| N4 Reviewer = fork of Lead, read-only | nicobailon/pi-subagents (`context:"fork"` + custom agent with `tools` allowlist) | partial | Fork = branched child session of the persisted parent; signed thinking blocks stripped. The builtin `reviewer` may make "small fixes", so CPD must define its own read-only agent. pi-goal-x and pi-background-tasks give read-only reviewers but with a fresh or projected context, not a true fork. |
| N5 job graph, history, queryable | mu (SQLite DAG, append-only ops log); `pi-extensible-workflows` (journal) | partial | mu is a close match for N5 (see Additional finds). Workflow engines hold the graph in a script, not a queryable store. |
| N6 concurrency cap per machine | none | none | All caps are per run or per session: pi-subagents `globalConcurrencyLimit` (20/run), tintinweb `maxConcurrent` (10/session), piewf `concurrency` (1–16/session), mu `MU_DELEGATE_MAX` (16/pi session), taskplane `maxLanes` (3/batch). CPD must add a machine-wide cap (guess: a lock or counter outside any single pi process). |
| N7 timeout per Attempt | pi-subagents `timeoutMs` (default 30 min, plus `checkpointBeforeDeadlineMs` steer); piewf per-agent `timeoutMs`; @quintinshaw `agentTimeoutMs` | full | tintinweb has only max-turns, no wall clock. |
| N8 retry ladder (fresh session, different model, feedback) | @quintinshaw `gate(thunk, validator, {attempts})` + `retry` + per-call `model`; piewf `retries` + `reviewLoop` starter | partial | Expressible in a script; no engine ships a "model ladder" as config. |
| N9 worktree per Attempt from Lead Branch HEAD + setup | tintinweb (`worktree add --detach … HEAD`, commits preserved on a branch); pi-subagents (`baseRef`, setup hook, patch/handoff manifest) | partial | tintinweb keeps commits, no setup hook. pi-subagents has a `worktreeSetupHook` but captures a patch and removes the worktree, so commits are not the artefact. |
| N10 merge queue, keep commits | taskplane (merge agent, temp branch, `--ff-only` into orch branch) | partial | Nothing else merges. Others stop at "branch exists, you merge". |
| N11 mechanical Gate | pi-subagents `options.gate` (host-run command, typed JSON gate); tintinweb `agent({gate:"npm test"})` runs in the child worktree | full | Gate runs inside the engine, not the repo's `verify` unless you pass it as the command. |
| N12 PR feedback loop | none (osolmaz `autoimplement` does CI/PR-comment handling as an agent workflow; kxm has signed webhook workflows) | partial | Neither fits CPD without heavy adaptation. |
| N13 metrics per Attempt | pi-subagents (status.json usage, usageBudget); piewf (tokens, cost, duration, launches); tintinweb `usage.ts`; pi example `index.ts:366-376` | full | Pi `usage.cost.total` is the common source. Most engines drop cacheRead from display totals. |
| N14 structured questions / plan approval UI | pi-goal-x (`goal-questionnaire.ts`, TUI dialog); pi-intercom `interview_request`; piewf `checkpoint()` | partial | |
| N15 visibility | pi-subagents FleetView + herdr metadata bridge; piewf Trajectory; osolmaz `piw` pane; mu dashboard | full | |
| N17 agent-agnostic | agent-comms (pi, Claude Code, Codex, opencode bridges); kxm (Claude Code + pi); ak-role engines; pi-subagents external CLI profiles on herdr machines | partial | pi-subagents can run Claude Code/Codex/Cursor profiles only on herdr saved machines. |
| N18 cheap aux agents | all engines (per-call model); gentle-shell profiles; pi-advisor (direct model call, no session) | full | |
| N19 building blocks | pi-subagents `parent-wake.ts`, `herdr-pi-bridge.ts`, `herdr-connection.ts`; mu `docs/architecture/control-socket.md`; tintinweb `cross-extension-rpc.ts`; pi-intercom `herdr-location.ts` | n/a | See recommendation. |

### Overlaps and conflicts
- **Same tool names / double orchestrators.** nicobailon registers `subagent`, `subagent_supervisor`, `contact_supervisor`, `bg_wait`; tintinweb registers `Agent`, `get_subagent_result`, `steer_subagent`, `SubagentWorkflow`; piewf registers `subagents_run/inspect/steer/stop/retry` and `workflow_*`; gentle-shell registers `subagent_run/result/continue/parent_message`. tintinweb disables its own `SubagentWorkflow` if another `Workflow`/`SubagentWorkflow` tool exists (README). Loading two of these gives the model two overlapping orchestrators. Pick one.
- **Same event: idle wake.** pi-subagents and pi-intercom both wake an idle pi via `sendUserMessage` and deliberately share one reservation key (`parent-wake.ts:11-14`, `idle-wake.ts`). Any custom extension that also wakes the Lead should join that key. Cause: pi#5581 (a `sendMessage({triggerTurn:true})` turn skips `before_agent_start`).
- **pi-subagents depends on pi-intercom?** No: v0.77 ships a native `contact_supervisor` and `subagent_supervisor`; generic `intercom` is optional.
- **Herdr bridges.** pi-subagents publishes `herdr:busy` pane metadata and owns an optional inspector plugin; `@piewf/herdr` and osolmaz also touch herdr panes; pi-intercom reads `herdr api snapshot`. They do not conflict (different calls), but all assume `HERDR_ENV=1` and `HERDR_PANE_ID`.
- **Compatibility with pi 1.1.0.** `pi-background-tasks` peer range is `^0.81–0.84` (will not satisfy 1.1.0); Michaelliv/pi-dynamic-workflows `^0.78`. tintinweb requires `>=1.1.0`; pi-advisor `^1.0.0`; the rest use `*` or `>=0.8x/0.99`.

### Recommendation for CPD v0
1. **Do not adopt a workflow engine as the control plane.** CPD's Plan is an LLM-authored DAG that the User approves, then is Locked, with Jobs appended later, retries as Attempts, and a serialized merge queue. JS-script engines (pi-subagents, tintinweb, piewf, @quintinshaw) express a DAG as program text that the model writes per run. That fits fan-out, not an auditable, append-only, queryable Job graph.
2. **Borrow the mu data model for N5** (tasks + `blocks` edges, `OPEN/IN_PROGRESS/CLOSED` + substates, append-only notes and ops log, derived ready set) or build the same on SQLite. mu itself is 4★, 1 contributor, and says it is "not a model router" and "not a workflow engine"; read it as a design reference first. Verify before reuse: its herdr backend cannot pass `--model` (see mu).
3. **Reuse for Worker feedback and Escalation (N3): pi-intercom or the native `contact_supervisor` of nicobailon/pi-subagents.** Both are tested, maintained (pushed 2026-10-10 / 2026-10-12), and block the child on a reply. If CPD does not adopt pi-subagents, vendoring is not needed: pi-intercom installs alone and also resolves herdr location.
4. **For N2 copy the idle-wake pattern** (`parent-wake.ts`: append notice with `triggerTurn:false`, then `sendUserMessage(wakeText, {deliverAs:"steer"})`, shared `Symbol.for("pi.idle-wake.v1")` reservation, hold after user abort). Do not re-derive it.
5. **For N1 + herdr, borrow mu's pi control socket** (`hello/status/send/wait/abort/interrupt/fresh/command` over a unix socket, backed by `pi.sendUserMessage` and `agent_settled`) rather than screen-scraping. It gives exact "this run settled with this lastText" semantics that herdr `agent wait` explicitly does not (herdr "does not track individual turns").
6. **Spawn model/thinking per Worker via argv, not via an override.** herdr 0.9.3 docs say `herdr agent start <name> --kind pi --pane ID -- <agent-args>` passes args to the executable. mu's source claims `-- --model m` was not honoured on 0.8.0 and that 0.9.0 has no override flag. These two statements disagree; unverified. Needs a one-off test in a scratch herdr session before design lock. Fallback: `pane run <pane> "pi --model … "` as pi-intercom does (loses herdr agent classification).
7. **Take tintinweb's worktree semantics for N9** (`git worktree add --detach <path> HEAD`; commit with `--no-verify`; branch left behind; agent's own commits preserved; fail loud if worktree cannot be created) as the spec to copy, and add per-repo setup yourself. nicobailon's worktree layer returns patches and deletes the tree, which conflicts with "keep commits".
8. **N10 and N6 have no off-the-shelf answer.** Read taskplane `extensions/taskplane/merge.ts` (temp merge worktree, `merge --ff-only`) for the merge queue; write the machine-wide cap ourselves.
9. **N4: use nicobailon `context:"fork"` as the reference implementation**, or replicate: branch the Lead's session file, strip signed thinking blocks, pass a tool allowlist. Do not rely on read-only-by-prompt (pi-goal-x's auditor gets `bash` in its tool list and is read-only only by instruction; `goal-auditor.ts:230,359`).
10. **Skip:** pi-dynamic-workflows (Michaelliv), pi-background-tasks, pi-advisor, ak-pi-workflow-roles, kxm, agent-comms as the CPD substrate. Borrow one idea each where noted.

## Items

### nicobailon/pi-subagents (npm: pi-subagents)
- Meta: ★3909 · pushed 2026-10-12 · created 2026-01-07 · MIT · v0.77.0 · 608,963/mo · SHA 8fcc3c787a6e7d9da0f21bafb604fe937084c21b
- What: Full delegation layer for pi: foreground and detached-background child sessions, scripted workflows, missions, supervisor channel, acceptance gates, worktrees, FleetView.
- Execution model: foreground children are SDK sessions inside the parent pi process; background children are SDK sessions inside a detached Node runner (README "How it works"; `src/runs/background/subagent-runner.ts`). Not panes. External-CLI children (native pi, Claude Code, Codex, Cursor) can run in a herdr pane **only on a herdr saved machine**: `machine` option, `src/runs/shared/herdr-placed-run.ts:112-165` calls socket methods `workspace.create`, `tab.create`, `agent.start`, `agent.prompt`, `pane.read`, `pane.close`; docs `docs/agents.md:262`. `cwd` is the remote path; local worktrees are rejected for placed runs (`agents.md:334`).
- DAG / dependencies / parallelism: JS statement-body scripts in a restricted sandbox (`runs.run`, `runs.all`, `runs.lanes`, `runs.steer`, `Promise.race`), plus chains and dynamic fan-out. `docs/workflows.md:48-110`. Limits: `globalConcurrencyLimit` default 20 per run (`docs/configuration.md:430`), `maxSubagentSpawnsPerRun` 64, `maxSubagentSpawnsPerSession`.
- Persistence / resume: async runs write `status.json`, events, handoff manifests; missions are durable records (`docs/missions.md`); after `/reload` a rerun of the same script+args reuses finished children (`workflows.md:32`). Run history is on disk and queryable via `subagent({action:"status"})`; not an append-only graph store (inference).
- Worktree: `worktree:true` per `runs.run`; branches from clean HEAD or `baseRef`, journals ownership, captures a patch and handoff manifest, then removes cleanly captured worktrees and branches (`workflows.md:444-460`); setup hook (`docs/configuration.md` "worktreeSetupHook"). Source checkout must be clean.
- Review / approval gates: `options.acceptance` levels (attested/verified), `options.gate` host-run command with optional typed JSON, `lane.recordMerge` attestation (`tool-reference.md:79,115`). No merge executed by the extension (it records evidence).
- Per-step model+thinking: `model: "provider/id:high"`, `agentOverrides`, `subagents.defaultThinking`, `maxThinking`, `docs/models.md:50-127`.
- Metrics: usage in status, `usageBudget {tokens,costUsd}`, `toolBudget`, FleetView token columns (`observability.md:108`, `types.ts:1255-1270`).
- Parent notification (N2): `src/shared/parent-wake.ts:37-75` — append notices with `triggerTurn:false`, then `pi.sendUserMessage(PARENT_WAKE_TEXT,{deliverAs:"steer"})` when idle; shared reservation (`:11-14`); user abort holds wakes (`:19-24`).
- Escalation (N3): `contact_supervisor` (child) and `subagent_supervisor` (parent) native; `docs/workflows.md:495-529`; scoped to the parent pi session id; nested chains supported; a detached child pauses its workflow.
- Herdr: `src/extension/index.ts` + `docs/extension-api.md:541-548` publish active-run counts and `herdr:busy` through pane metadata; `src/extension/herdr-pi-bridge.ts:99-102` is a bridge in the placed child that accepts `prompt/steer/follow-up/abort/supervisor-reply` frames over a private unix socket; `project.open/status/close` opens a peer pi pane in another repo (`extension-api.md:562-592`, public API `pi-subagents/project-panes`).
- Maturity: 309 unit + 46 integration test files; CI on Ubuntu and Windows (`.github/workflows/test.yml`); releases every few days (v0.77.0); one dominant maintainer; 5 open issues. Pi peer `pi-ai >=0.86.1`. Changelog shows heavy churn (hard cutovers by policy: `VISION.md` "Compatibility is explicit"), which is a pinning risk.
- CPD fit: N1 partial (model+thinking yes; panes only remote), N2 full, N3 full, N4 partial (`context:"fork"` + custom read-only agent), N6 partial (per run), N7 full, N8 partial (retry loops are script-level), N9 partial (patches), N11 full, N13 full, N15 full, N17 partial (remote only), N18 full, N19 high value.
- Gaps/risks: huge surface (≈260k lines incl. tests; `subagent-executor.ts` 8k lines); the model-facing tool is a large schema (it lazily activates via `subagents_enable` to save cache); parent-driven orchestration means the Lead model writes JS scripts, which conflicts with "Lead never writes code" only loosely but couples the Job graph to prompt text; no merge queue; worktree output is a patch, not commits; no machine-wide cap; model-led flow not enforced by a state machine.
- Verdict: **BORROW**. Take `parent-wake.ts`, the supervisor channel design, `context:"fork"` semantics and the herdr bridge/connection code as patterns. If CPD wants a quick v0 without a custom engine, installing it as-is for Worker/Reviewer spawn is viable but fights the Job-graph model.

### tintinweb/pi-subagents (npm: @tintinweb/pi-subagents)
- Meta: ★1293 · pushed 2026-10-12 · created 2026-03-05 · MIT · v0.21.0 · 34,125/mo · SHA e819a82ae364a2b0432881b89b0c784b413a88c1
- What: Claude-Code-style `Agent` tool with background agents, FleetView, scripted `SubagentWorkflow`, cron schedules, nested agents and a cross-extension RPC over `pi.events`.
- Execution model: in-process SDK sessions in the parent (`src/agent-runner.ts`, `src/agent-manager.ts`); no panes, no separate processes (workflow scripts run in a `node:vm` worker thread).
- DAG: `agent()`, `parallel()`, `pipeline()`, `phase()`, `gate:` and `resume:` inside a script (README "Scripted workflows"); workflow fan-out bounded at `max(1, min(16, cpus-2))`.
- Persistence: schedules persisted (`.pi/subagent-schedules/`, PID-locked); agent transcript files; workflow runs inspectable in `/agents`. No durable job store (inference from README + file list).
- Worktree: `src/worktree.ts:106-113` `git worktree add --detach <tmp path> HEAD`; on completion `add -A` + `commit --no-verify` (`:141-145`), branch `pi-agent-<id>` created; the agent's own commits are preserved and leftovers committed on top (README "Worktree Isolation"); strict failure when no git or no HEAD. Worktree dir removed after completion; only the branch survives. No setup hook (grep found none).
- Review/approval: `gate: "<cmd>"` runs inside the child's worktree.
- Per-step model+thinking: `Agent({model, thinking})`; fuzzy names ("haiku"); `invocation-config.ts:103-148`; RPC `options.model` accepts a `provider/modelId` string.
- Concurrency (N6): `maxConcurrent` default 10 background slots, optional `maxConcurrentForeground` (`src/agent-manager.ts:6-7`); queueing automatic; per session, not machine.
- Timeout (N7): none per agent; `max_turns` with a grace window (`agent-runner.ts:382-413`). No wall-clock timeout (grep `timeout` found only internal races).
- Metrics: `src/usage.ts` accumulates input/output/cacheWrite/cacheRead/cost from `message_end`; `cost` is `usage.cost.total`; survives compaction.
- Notification (N2): `src/index.ts:470-475` `pi.sendMessage(..., {deliverAs:"steer", triggerTurn:true})`; this path skips `before_agent_start` for an idle parent (pi#5581, per nicobailon's comments) — unverified whether that matters for CPD.
- Cross-extension RPC: `src/cross-extension-rpc.ts:105,168,192` handles `subagents:rpc:ping|spawn|stop|consume` over `pi.events`; replies on `…:reply:<requestId>`; lifecycle events `subagents:created|started|completed|failed|steered|compacted`. `options.cwd` and `options.model` are accepted. This is the cleanest hook for another pi extension (e.g. a CPD extension) to spawn workers.
- Escalation (N3): none (no child→parent query tool; grep for escalate/ask_parent/contact found nothing).
- Maturity: vitest suite (many files), biome lint, CI `ci.yml`; v0.21.0; 109 open issues; one author (263 commits vs 3 for the next). Requires pi >=1.1.0 (matches our version).
- CPD fit: N1 partial (in-process, not a pane), N6 partial, N9 partial (best semantics, no setup), N11 partial (`gate`), N13 full, N17 skip. N3 gap.
- Gaps/risks: in-process means the Lead session and all Workers share one Node process and one pi version; a crash of the Lead kills Workers; no herdr integration at all (no `herdr` grep hits in `src`).
- Verdict: **BORROW**. Worktree commit-preserving logic and the RPC-over-`pi.events` pattern are the useful parts.

### osolmaz/pi-workflows (npm: @osolmaz/pi-workflows)
- Meta: ★329 · pushed 2026-10-08 · created 2026-07-18 · MIT · v0.17.7 · 6,225/mo (not the unrelated npm `pi-workflows`) · SHA 2cb8ab3a9c2e52d0c193d572728a9ccd7afdb77a
- What: Durable TypeScript workflow-graph engine (nodes `agent`, `compute`, `action`, `shell`, `notify`, `checkpoint`, `decision`) with a global SQLite-backed server, a Rust TUI viewer `piw` and a herdr plugin.
- Execution model: **agent steps run in the user's own origin pi conversation**, one step message at a time (README "Node types"; `docs/WORKFLOW_STEP_MESSAGES.md`). Headless `pi --mode rpc` child only for resource-manager workflows without an origin session (`src/server/rpc-executor.ts:57`), and that executor cannot enforce tool allowlists (`docs/WORKFLOWS.md:216`). A built-in `pi-agent-group` fans out up to 8 read-only agents in-process (`src/builtins/pi-agent-group.ts:21-22`: MAX_AGENTS 8, concurrency 8, timeout 15 min default, tools limited to read/grep/find/ls).
- DAG: named nodes plus `edges`, `includeWorkflow`, `controlLoop`, `decisionEdge` with compile-time case checking; one active workflow per session.
- Persistence/resume: `~/.pi/agent/workflows/state.sqlite`, one server process owns it; pause/resume at durable boundaries; safe replay of idempotent effects (`README` "Always-on workflows"; `docs/SQLITE_STATE.md`).
- Worktree/review: `autoimplement` builtin prepares a branch or worktree, runs "pi-reviewer", tracks P0–P2 findings, handles PR comments/CI and finalizes the PR (README Examples; `src/builtins/autoimplement.workflow.ts:319`). This is a single-conversation agent workflow, not a swarm.
- Per-step model: none found (grep `model` in `docs/WORKFLOWS.md` returned nothing relevant); steps use the origin session's model. Unverified for builtin group.
- Metrics: per-node trace and updates (progress counts/ETA), no per-attempt cost seen.
- Herdr: `herdr-plugin.toml` (id `osolmaz.pi-workflows`, min herdr 0.7.0, one `[[panes]]` entry running `plugins/herdr/viewer.mjs`). Only a viewer pane via `src/extension/herdr-viewer.ts`, `src/herdr/client.ts`. Not for Workers.
- Maturity: 124 test files, CI + publish workflows, 662 commits by one author. Needs pi >=0.99.
- CPD fit: N5 partial (durable store, but it is its own graph), N14 partial (protected human decisions, `/workflow answer`, Telegram channel), N15 partial (piw/Herdr viewer). Not a fit for N1/N3/N4.
- Gaps/risks: workflow occupies the interactive conversation (conflicts with "Lead never writes code" and with the User typing to the Lead); out-of-process server + SQLite + Rust binary is a heavy install (`cargo install pi-workflows`).
- Verdict: **BORROW** the SQLite-run-state design and "protected human decision" idea; skip as a runtime.

### vekexasia/pi-extensible-workflows (npm: pi-extensible-workflows; herdr package @piewf/herdr in `packages/extensions/herdr`)
- Meta: ★246 · pushed 2026-10-12 · created 2026-07-12 · MIT (package.json `license`; no LICENSE file in repo root, GitHub shows none) · v6.2.0 · 5,313/mo core, 2,949/mo `@piewf/herdr` · SHA f5e5123c57e5e23057f4d30c2a45a3bc62d5b118
- What: Deterministic, resumable, journaled workflow scripts (`agent`, `parallel`, `pipeline`, `withWorktree`, `checkpoint`, `shell`, `agent.create`/`handle.send`) plus standalone `subagents_*` tools, a `reviewLoop` starter and an optional Herdr package.
- Execution model: SDK sessions created by the host (`createLocalPiSession`), workflow script in a separate sandbox; with `@piewf/herdr` and `extensionSettings.herdr.enableFullyInspectableMode: true`, **every workflow agent launches in a dedicated Herdr workspace/tab/pane** (`packages/extensions/herdr/index.ts:364` closes tab/pane; `packages/core/src/herdr.ts:11-15` runs the `herdr` CLI via `execFile`; README of the herdr package). Live handoff moves a running session into a pane and returns ownership on `agent_settled` via a generated bridge.
- DAG: script-based; `parallel("name", {key: () => agent(...)})`, structural keys make replay deterministic; concurrency 1–16 (default 8) in settings; per-call `retries`, `timeoutMs`, `outputSchema`, `worktree` on standalone runs (`docs/llm.md` "Standalone Subagents").
- Persistence/resume: journal replay after cold recovery; run states `queued … budget_exhausted … interrupted`; `workflow_resume`, `workflow_retry` with `expectedState`; retention settings. Strongest resume design among the script engines.
- Worktree: `withWorktree(name, cb)` gives each named scope a deterministic owned branch+worktree; caller branch unchanged; a later run can reuse scopes via `parentRunId`. Merge is left to a downstream agent ("Merge these verified worktrees"). Base ref = current HEAD (inference from docs; not read in code).
- Review/approval: `checkpoint({name,prompt,context})` for approval, journaled; `reviewLoop` (dev→reviewer with `outputSchema {pass, findings}`, maxIterations 5; `packages/core/starter/review-loop.ts`).
- Per-step model: `model:"openai-codex/gpt-5.6-sol:high"` or alias, `modelAliases` in settings; dynamic aliases resolved once per launch.
- Metrics/budget: tokens, costUsd, durationMs, agentLaunches per run with hard/soft limits, human-approved budget raises, Trajectory web UI with Gantt (docs developers "budget").
- Maturity: 62 core test files plus herdr and cli tests, `.github/workflows`, eslint, docs site, 850 commits by one main author (14 by another). Needs Node 22.19, pi >=0.99.
- CPD fit: N1 partial (model+thinking per agent; herdr pane per agent in fully-inspectable mode), N7 full, N8 partial (`retries`, `reviewLoop`), N13 full, N14 partial (`checkpoint`), N15 full, N19 high (an extension API for setup hooks, transports, `agentAttemptActions`).
- Gaps/risks: workflows are scripts the Lead writes; the herdr transport sits behind a plugin-specific mode and its interruption story is weak ("Herdr's Pi screen detection cannot distinguish an aborted turn from a completed turn"); no escalation (child→parent) tool; no merge queue; trusted-code model (extensions run with host privileges).
- Verdict: **BORROW**. Reference for journaled resume, `withWorktree` naming, budget model and the herdr transport; if CPD ever wants a script-engine path this is the strongest candidate.

### QuintinShaw/pi-dynamic-workflows (npm: @quintinshaw/pi-dynamic-workflows; GitHub repo 404)
- Meta: ★unknown (repo and user not found on GitHub as of 2026-10-12) · npm modified 2026-10-06 · created 2026-05-30 · MIT · v3.14.0 · 23,981/mo · read from npm tarball `quintinshaw-pi-dynamic-workflows-3.14.0.tgz` (no commit SHA; no tests in tarball)
- What: Claude-Code-style dynamic workflow tool (`agent`, `parallel`, `pipeline`, `phase`, `verify`, `judgePanel`, `loopUntilDry`, `retry`, `gate`, `checkpoint`) with tiered model routing, journaled resume and worktree isolation.
- Mechanism: in-process SDK sessions; `node:vm` sandbox. Tiers `small/medium/big` each map to one `provider/model:thinking` (`src/model-tier-config.ts`). `agent()` options include `model`, `tier`, `schema`, `isolation:"worktree"`. Tool input: `concurrency` (≤16), `maxAgents` (≤1000), `agentRetries`, `agentTimeoutMs`, `tokenBudget`, `resumeFromRunId`, `resumeMode` (README generated capability table).
- Worktree: `src/worktree.ts:74-77`: `git worktree add -b pi/wf/<slug>-<uuid> <repo>/.pi/worktrees/<id> HEAD`; kept for merge by default (`keepWorktree:false` deletes).
- `gate(thunk, validator, {attempts})` passes validator feedback into the next attempt (README signature). This is the closest shipped analogue to "retry with reviewer feedback" (N8), though same model unless the script changes it.
- Metrics: real per-session tokens and cost, estimates flagged when a provider omits usage.
- Maturity: unverifiable repo (missing), npm-only; frequent releases (3.14.0); relies on npm provenance only. Provenance risk: if the repo is deleted, the maintainer cannot be contacted through issues.
- CPD fit: N8 partial, N7 full (`agentTimeoutMs`), N13 full, N1 partial.
- Verdict: **SKIP** as a dependency (source repo gone, in-process, script-driven). **BORROW** the tier+gate API shape.

### Michaelliv/pi-dynamic-workflows (npm: pi-dynamic-workflows; the repo I cloned instead of #5)
- Meta: ★1230 · pushed 2026-05-31 · created 2026-05-28 · MIT per npm (no LICENSE file; GitHub shows none) · v1.0.1 · 1,379/mo · SHA 31b2aca0f1cb195aafbfc5e3ee2b8c83ad3f21a2
- What: 1.1k-line prototype of the `workflow` tool with `agent`, `parallel`, `pipeline`, `phase`, `budget`; README says it does not implement persisted or resumable runs.
- Mechanism: `src/agent.ts:61` `createAgentSession` in memory; `node:vm`; `model` option parsed (`src/workflow.ts:423`) but passed as text hint (`:449`) — unverified whether it actually switches models.
- Maturity: stale (4.5 months), peer `^0.78` (incompatible with pi 1.1.0), forks exist (`@maplezzk/pi-dynamic-workflows` 1.5.0).
- Verdict: **SKIP**: superseded by tintinweb/QuintinShaw/mu-style engines.

### ismailsaleekh/pi-background-tasks (npm: pi-background-tasks)
- Meta: ★35 · pushed 2026-09-30 · created 2026-05-26 · ISC · v2.6.9 · 97,554/mo · SHA 4aceb55fc7dfa3e99b8e7b275f4910af4362a776
- What: Background shell jobs (`bg_run`, `/bg`, `bg_logs`, `bg_kill`), one read-only `bg_delegate` child, multi-model "Fusion" (3 candidates, blind evaluator, merger), an attested direct pi run and a bundled Anthropic OAuth attribution/prompt-sanitization layer.
- Mechanism: `bg_run` shells with durable output files and notifications (`src/extension.ts:260` `pi.sendMessage`). `bg_delegate` launches an inspect-only child (read, grep, find, ls) with a **frozen projection of the visible conversation**, hash-verified result via `bg_result` (README "Pick the right workflow"). `bg_run_pi_attested` spawns `pi --mode json`. Not an orchestrator: no DAG, no worktrees, no merges.
- Maturity: CI + release-certification workflows; docs generated; peer range `^0.81.1 … ^0.84.0` which **does not include pi 1.1.0** (package.json). The attribution/sanitization layer rewrites provider requests for Anthropic subscription OAuth; compatibility with Anthropic terms: unverified, and a policy risk for CPD.
- CPD fit: N4 idea only (read-only child with frozen conversation projection, no tools beyond read); N7/N11 shell-job notifications.
- Verdict: **SKIP**. High downloads are from the shell-jobs feature, not orchestration.

### Gentleman-Programming/gentle-shell (npm: gentle-pi)
- Meta: ★1291 · pushed 2026-10-12 · created 2026-05-10 · MIT · v4.1.0 · 35,595/mo · SHA b19d7c505b4ed49859c6a650c2a1d2f10b6e20db (shallow clone; GitHub contributors API lists upstream pi-mono authors such as badlogic 3104 and mitsuhiko 95 commits, so the repo history was seeded from pi-mono; unverified why)
- What: An opinionated pi distribution ("shell"): focused agents, ODD workflow, profile-based model routing, review integration, themes, memory, TUI changes view.
- Execution model: **each subagent is its own `pi --mode rpc` process** (`lib/agents-runner.ts:16,306`), with `--model <ref>:<thinking>` (`:317-318`), session dir per child, live card, results as `gentle-agents.result` messages that start a turn when the parent is idle ("nothing polls", `docs/gentle-shell.md:239`). History persisted in `~/.pi/agent/gentle-agents/tasks/` (200 retained). No herdr coupling for Workers; `extensions/gentle-herdr-activity.ts` only publishes activity to herdr pane metadata.
- Concurrency: `max_concurrency` default 5 (`lib/agents-config.ts:109,281`); stall timeouts 4 min / 30 min tool (`:107-108`). Wall-clock per-task timeout: not found.
- Escalation (N3): child tool `subagent_parent_message` with `kind:"notification"|"query"`; a query blocks for the reply (`extensions/gentle-agents.ts:315-336`). Also `orchestrator_consult/send_message/list` for cross-session.
- Review: native review integration (`docs/review-integration.md`), cross-model writer/reviewer routing; writer profile recorded on review receipts (`gentle-agents.ts:350`).
- Per-step model+thinking: named profiles, repo-pinnable (README "Profiles and model routing"). Metrics: model, tokens, cost, elapsed in live card and overlay.
- Maturity: 355 test files, very active (686 open issues), strong brand; pi >=0.99.1.
- CPD fit: N1 (argv model+thinking, process per child) partial, N3 full, N7 partial, N13 full. But its subagent tools are tied to its permission modes, ODD and "agents-home" layout.
- Gaps/risks: takes over the whole pi shell (themes, permission modes, prompts, sound notifications, telemetry doc). Adopting it means adopting the distribution. DAG: none (parent decides). No worktrees by default for writers (guess: README says "disjoint edit surfaces").
- Verdict: **SKIP** as a dependency. **BORROW**: process-per-child RPC runner and `subagent_parent_message` query pattern.

### tmonk/pi-goal-x (npm: pi-goal-x)
- Meta: ★95 · pushed 2026-10-02 · created 2026-05-14 · MIT · v0.32.3 · 130,230/mo · SHA 64c5ace87f3400c0b55ef87f2d2912167f53dbaa
- What: `/goal` with persisted task tree, completion requirements, auto-continuation and an optional independent completion auditor.
- Mechanism: single-session loop (`goal-state.ts:276-283` `pi.sendMessage(..., {triggerTurn:true, deliverAs:"followUp"})`; `goal-drafting.ts:182` `sendUserMessage`). Auditor is an in-process `createAgentSession` (`goal-auditor.ts:333`) with tools `["read","grep","find","ls","bash"]` (`:359`) and a prompt saying it is read-only (`:230`); model/thinking chosen in `/goal-settings`. Questionnaire dialog `goal-questionnaire.ts` (options, recommended, custom answer) for guided goal creation.
- Persistence: goals, tasks, evidence stored across sessions (`storage/`, `goal-ledger.ts`).
- Maturity: 87 test files, 43k lines, no herdr. pi >=0.83.
- CPD fit: N14 partial (questionnaire UI is a copyable pattern), N4 idea (separate-model auditor after completion). No spawn, no DAG, no worktrees.
- Gaps/risks: auditor read-only by prompt, not by tool allowlist (`bash` present).
- Verdict: **BORROW** the questionnaire TUI and auditor settings; skip otherwise.

### philipbrembeck/pi-advisor (npm: pi-advisor-flow)
- Meta: ★47 · pushed 2026-10-11 · created 2026-07-17 · MIT · v0.13.1 · 64,227/mo · SHA 1caef6f0ec0f595e40824dc7e218fd4e33efa637
- What: `ask_advisor` tool: a stronger model gives second opinions to an executor model, with gates (before plans, after repeated failures, before completion), loop detection, usage ledger and optional herdr surfacing.
- Mechanism: direct provider stream calls through `ctx.modelRegistry.streamSimple` (`src/model-stream.ts:21-47`), not a pi session; reconstructs filtered conversation context with redaction; per-response token/cost recorded and normalized into `/cost`. Herdr hits are only in tools/ui files (status surfacing; `grep -rn herdr` found them in `src/tools/*.ts`, `src/ui/*.ts`).
- Maturity: bun test suite, bench dir (Harbor trials), oxlint, knip, preview channel; pi ^1.0.0.
- CPD fit: N8 idea (escalate difficulty to a stronger model with a budget) and N18 (cheap aux calls without a session); N13 pattern (per-call ledger). Not a spawn/orchestration tool.
- Verdict: **BORROW** the ledger/redaction pattern; skip otherwise.

### Akagilnc/ak-pi-workflow-roles (npm: @akagilnc/pi-workflow-roles)
- Meta: ★5 · pushed 2026-10-11 · created 2026-07-27 · Apache-2.0 · v0.1.0 in package.json (npm modified recently; version shown by `npm view` 0.1.5837) · 42,943/mo · SHA 64750122c851c203a315443a26029e5f74659e33
- What: A personal "imperial court" role system as a CLI (`ak-role judge|coder|reviewer|fixer|merger|notary|…`), roles with souls, terminating submission tools and a receipts ledger; 5.8k commits from one author; docs in Chinese/English.
- Mechanism: each role call is one run with `--model provider/model[:thinking]` per invocation, `ak-role config set-engine <role> claude-code|cursor`, `set-auto-resume-limit` (README); ACP host in `src/acp-host/`. Roles have their own submission gate and "Unfinished" terminal state (ADR 0050).
- Maturity: 114k lines, huge ADR trail, 1 maintainer, 5★; download count (43k) is disproportionate to stars: unverified cause.
- CPD fit: N17 idea (engine abstraction), N8 idea (auto-resume with limits). Terminology and workflow are idiosyncratic and unrelated to CPD's Job/Attempt model.
- Verdict: **SKIP**.

### nicobailon/pi-intercom (npm: pi-intercom; repo not in npm metadata)
- Meta: ★534 · pushed 2026-10-10 · created 2026-03-12 · MIT · v0.17.0 · 27,160/mo · SHA 14731a4873b500e19d000bf131142e430e4b7928
- What: 1:1 messaging between pi sessions on one machine through a local broker, with `intercom` tool (`list/send/ask/reply/pending/cancel/handover/status`), `contact_supervisor` bridge for pi-subagents children, and herdr awareness.
- Mechanism: local broker over unix socket (`broker/broker.ts`, `broker/client.ts`, auto-spawn `broker/spawn.ts`); inbound messages are appended with `pi.sendMessage` (`index.ts:1295-1305`) and an idle session is woken with `pi.sendUserMessage("New intercom message above.", {deliverAs:"steer"})` (`index.ts:1307-1312`) using the shared idle-wake reservation (`idle-wake.ts`). `ask` blocks until reply (10 min default, `PI_INTERCOM_ASK_TIMEOUT_MS`), one pending ask per session (README "intercom actions").
- Herdr: `herdr api snapshot` joins pi sessions to workspace/tab/pane (`herdr-location.ts:14-50`, location never cached); `openProjectPane` does `herdr pane split --current --direction right --cwd <root> [--focus]` then `herdr pane run <pane> pi` (`project-agent.ts:233-241`) — plain `pi`, no model/thinking flags (`PI_INTERCOM_PI_BIN`/`PI_BIN` override the binary name only). Cross-machine relay over herdr saved machines (README line 599). 
- Maturity: 25 `*.test.ts` files, no `.github/workflows` directory in the clone (no CI seen), CHANGELOG present, 1 open issue.
- CPD fit: N3 full (Worker → parent `ask`, blocking, reply returns as tool result), N2 full (inbound wake pattern), N15 partial (herdr location in session list), N1 partial (spawn pane but no model flag), N17 no.
- Gaps/risks: an ask requires the recipient to be connected; "disconnected target fails immediately" so a Lead that restarted loses the question; no persistence of unanswered asks across Lead restart (README says broker keeps queued messages for sends, not for asks; unverified for restart); name-based addressing.
- Verdict: **REUSE** for N3 (or use pi-subagents' native equivalent); **BORROW** `idle-wake.ts` and `herdr-location.ts`.

### ExaDev/agent-comms (npm: agent-comms)
- Meta: ★29 · pushed 2026-10-10 · created 2026-04-26 · MIT · v8.13.20 · 46,698/mo · SHA 0faf73b095815bf68e6749fa5696b7e457a7484f
- What: Cross-harness chat mesh (rooms, DMs, presence, visibility) over localhost TCP with bridges for pi, Claude Code, Codex, opencode and MCP.
- Mechanism: pi bridge `src/bridges/pi/extension.ts` (330 lines) registers one `agent_comms` tool (`register|update|whoami|create_room|list_rooms|join_room|leave_room|send|dm|list_agents|read_room|invite|decline_invite|kick|destroy_room|web_url`, lines 210-262) and wakes the model with `pi.sendMessage(..., {triggerTurn:true, deliverAs})` (`:98-113`). No blocking ask/reply primitive.
- Maturity: stryker, playwright, vitest, semantic-release (≈294 bot commits); heavy dependency set (React 19, Mantine, openpgp, orpc beta, wire-mesh-core); 1 human maintainer.
- CPD fit: N17 partial (only cross-harness messaging candidate with real bridges), N3 no (no blocking ask).
- Verdict: **SKIP** for v0 (revisit for Claude Code/Hermes Workers: it is the only option here with bridges for them).

### kontextmind/kxm (npm: @kontextmind/kxm)
- Meta: ★2 · pushed 2026-10-12 · created 2026-08-25 · MIT · v0.7.1 on npm (GitHub tag v0.7.182) · 29,416/mo for `@kontextmind/kxm` (11/mo for unscoped `kxm`) · SHA e511785e9ced06d1e984638254ee1b79ec02abe1
- What: Local hub (HTTP+SSE, SQLite) for authenticated peer messaging between agents (Claude Code plugin, pi extension, CLI) plus a Runtime for YAML (`kxm.workflow.v1`) workflows with webhooks, quorum gates and a trust diff for `.kxm/*.yaml`.
- Mechanism: messages move `queued→delivered→replied` with idempotency keys; **workers are long-lived `pi --mode rpc` processes** under a supervisor with restart backoff, fallback model list (≤8), `--tools` allowlist and `--session-isolation workflow` (`docs/guides/pi-workers.md`). Model is fixed per worker process, not per Job. Peer `await` with fan-out to 1–3 peers.
- Maturity: CI, e2e, nightly, smoke; many docs; 2★, 1 open issue, GitHub tag v0.7.182 vs npm 0.7.1 (auto-release churn; exact cadence unverified).
- CPD fit: N17 partial (Claude Code + pi on one plane), N3 partial (await a peer), N12 partial (signed Jira/GitHub/CI webhooks resume parked stages). Conflicts with CPD's "fresh session per Attempt" (long-lived workers).
- Verdict: **SKIP**. **BORROW**: "a quorum gate counts only replies the hub routed" as an idea for N10/N12 provenance.

### pi's own example: `examples/extensions/subagent/` (pi 1.1.0)
- Meta: ships with `@earendil-works/pi-coding-agent` 1.1.0 · `index.ts` 1038 lines, `agents.ts` 157 lines · path read: `~/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/pi-coding-agent/examples/extensions/subagent/`
- What: One `subagent` tool (single, parallel, chain) that spawns `pi` subprocesses with an agent's system prompt, model and tool list.
- Mechanism: `index.ts:300-307` builds `pi --mode json -p --no-session [--model M] [--thinking L] [--tools a,b]`; `:346` `spawn`; usage parsed from JSON stream events (turns, input/output/cache/cost, `:366-376`); abort kills the process (`:410-419`). `MAX_PARALLEL_TASKS 8`, `MAX_CONCURRENCY 4` hard-coded (`:33-34`, `mapWithConcurrencyLimit` `:219`, `:645`). Agents are markdown with frontmatter (`agents.ts`: `tools`, `model`), project agents need trust (README "Security Model").
- CPD fit: shows the minimal contract for a Worker: argv model+thinking+tools, usage from `--mode json`, kill on abort. Foreground-blocking only (no background, no escalation, no timeouts, no worktrees).
- Verdict: **BORROW** as the reference for N1/N13 argv and JSON-stream usage; it is the authoritative pi-owned pattern.

## Additional finds

### mu-crew/mu (npm: @mu-crew/mu)
- Meta: ★4 · pushed 2026-10-11 · created 2026-05-07 · MIT (npm and README; GitHub shows none) · v3.12.0 · 4,670/mo · SHA d53d5e3b0cf0edc3aadabbd515d27724ce5628de · 1 contributor, 297 test files, CI only `release.yml` (no test workflow seen).
- What: "Control plane for a crew of agents": task DAG in SQLite, per-agent VCS workspaces (jj/sl/git worktrees), agents as panes in tmux **or herdr**, pi extension with a control socket, `mu_delegate` for one-off helper agents. README: "Not a workflow engine… Not a model router… Not a daemon."
- Mechanism (herdr): topology mapped workspace=workstream, tab=window, pane=agent (`docs/architecture/mux.md`); all herdr calls via CLI `herdr(args)` (`src/mux/herdr.ts`), spawn is `herdr agent start <name> --kind <cli> --pane <id>` (`src/mux/herdr.ts:1038`) with **no `--` args**; `--command`/`MU_<CLI>_COMMAND` overrides are refused on herdr (`HerdrCommandOverrideError`, `herdr.ts:176-215`, citing herdr 0.8.0 "argv:['pi']" evidence). `docs/guide/delegate.md` states the `model` option is "tmux only: herdr refuses a command override". **herdr 0.9.3 docs** (`cli-reference.mdx:355`) say "arguments after `--` are passed to that executable". The two sources disagree; unverified; test in a scratch herdr.
- DAG (N5): one edge type `blocks`; `OPEN→IN_PROGRESS→CLOSED` with substates `todo|parked|triage|active|done|rejected|wontfix|duplicate|superseded`; ready set derived; append-only per-task notes; append-only ops log with `mu undo <group>` and `mu rebuild`; all reads have `--json` (`docs/architecture/dag.md`, `README`). Claim protocol sets `owner`.
- Control socket (N1/N2/N3 adjacent): per pi agent unix socket protocol v1 JSON lines with `hello, status, send, wait, abort, interrupt, fresh, command` (`docs/architecture/control-socket.md:21-47`), backed by `pi.sendUserMessage` and `agent_settled`; `fresh` = new session + prompt. Remote agents over ssh.
- Delegation: `mu_delegate` returns immediately, answer arrives as a follow-up message when pi would otherwise stop, never mid-turn; `timeout` default 3600 s; `MU_DELEGATE_MAX` default 16 with queue up to 4×cap (`docs/guide/delegate.md`).
- Merge: orchestrator's job; `mu task wait` prints a cherry-pick command; docs advise "merge one task, run tests on the merged tree, then dispatch next" (`docs/guide/dispatch.md:143`). No automated merge queue.
- Escalation: child stalls in `needs_input` → `mu task wait` exits 7 after 5 s (pi worker via control socket); no blocking ask tool.
- Metrics: not seen.
- CPD fit: N5 partial/high, N15 partial (dashboard), N1 partial, N9 partial (workspaces), N6 partial (per session cap).
- Gaps/risks: 4★, single maintainer, hard coupling to its own CLI as the state API; herdr backend cannot choose a model (see above). The Job graph is orchestrator-driven (an LLM following recipes) rather than an engine-enforced state machine.
- Verdict: **BORROW** the DAG schema, ops log and control socket; this is the closest design relative of CPD in the whole cluster. Check with W1 (probably covered under a herdr-orchestration item) before duplicating.

### HenryLach/taskplane (npm: taskplane)
- Meta: ★219 · pushed 2026-09-19 · created 2026-03-12 · MIT · v0.30.6 · 1,809/mo · SHA dfa342bebb71d8be4fe327b38bd2a291504aca65
- What: Batch orchestrator for pi: tasks defined in `PROMPT.md`/`STATUS.md`, dependency-graph waves and lanes, git worktree per lane, worker, reviewer and merger agents, supervisor agent, mailbox, web dashboard.
- Mechanism: each agent is `node pi-cli --mode rpc --no-session --model M --thinking L` (`extensions/taskplane/agent-host.ts:452-469`; "No terminal-session backend", `:13`); lane config `maxLanes: 3` (`config-schema.ts:696`); merge agent timeout 90 min (`:729`); merge into a dedicated orch branch via temp worktree, `git merge --ff-only` (`merge.ts:1686,2535-2550`); checkpoint commits at step boundaries; reviewer on a different model recommended; state persisted (`persistence.ts`, `resume.ts`).
- CPD fit: N5 partial (file-based STATUS.md + persistence), N6 partial (`maxLanes`), N7 partial, N8 partial, N9 partial (worktree per lane, polyrepo), N10 **partial/best seen** (merge agent + ff-only into orch branch), N15 full (dashboard). Escalation: file mailbox, not blocking.
- Gaps/risks: 46 open issues; one maintainer (1,950 commits); heavy opinionated task format; no herdr; panes not used.
- Verdict: **BORROW** merge.ts/orch-branch design for N10; read before building the merge queue.

### baphuongna/pi-crew (npm: pi-crew)
- Meta: ★56 · pushed 2026-10-12 · created 2026-04-26 · MIT · v0.11.18 · 7,648/mo · SHA d29df7dd895cd32788a56d16628311a3d89254d2
- What: Single `team` tool: runs as real child pi processes with durable on-disk state, opt-in worktree isolation, steering, resume, scheduling. README warns it is AI-written, unaudited, built for one workflow.
- Herdr: mentions only in UI/settings/doctor files (`src/ui/agents-jobs-browser.ts`, `src/extension/team-tool/doctor.ts`); not read in depth.
- Verdict: **SKIP** (self-declared unaudited; not read in depth).

### JavePreem/pi-worker-graph (npm: pi-worker-graph, prerelease 0.1.0-dev.2)
- Meta: ★0 · pushed 2026-10-05 · created 2026-09-07 · MIT · SHA 787b44df16a3dfbe2c5b0dffdd29552879b36bf5
- What: DAG-first runtime: graph primitives, structured worker-report contract, bounded runner, one-shot pi subprocess adapter, per-node check and work-review-repair cycle, per-attempt token/cost accounting, run-scoped coordination events; workers share one checkout; orchestrator tool inactive by default (README).
- Verdict: **BORROW** concept only (design is closest to CPD's Attempt/Job model on paper); too early (0★, prerelease, 0 issues) to depend on. Only the README was read.

### elidickinson/pi-claude-bridge (npm: pi-claude-bridge)
- Meta: ★627 · pushed 2026-10-07 · MIT · v0.9.2 · 83,001/mo · SHA b2e7255ece0b8bfd3a6429fd5c147fd59984c785
- What: Uses Claude Code through the Agent SDK as a pi model provider and an `AskClaude` tool (README). Relevant to N17/N18 only as "Claude as a model", not Claude Code as a Worker. README flags unsettled Anthropic billing for Agent SDK tools.
- Verdict: **SKIP** (not read beyond README).

### Not reviewed (seen in npm search; belong to other clusters or not CPD-relevant)
- Herdr-specific, W2 scope: `pi-herdr-agents` (giuseppecrj, 5,794/mo), `@howaboua/pi-shepherdr` (3,908/mo), `@andrewjacop/pi-herdr` (2,778/mo), `pi-harness-runtime` (31,882/mo), `xz-pi-side-agents-herdr`, `pi-cmux` (cmux, not herdr).
- Orchestration-flavoured, unread: `@tinoy/pi-fleet` (1,472/mo, "long-lived worker sessions driven by one coordinating session"), `pi-agents-team` (background RPC worker agents), `@narumitw/pi-subagents`, `@cr1ms0n/pi-subagent` (fork of Luke Parke's pi-subagent), `@blackbelt-technology/pi-dashboard-subagents`, `pi-workflow-engine` (timbrinded, ★12, "Dynamax" fan-out), `pi-ptc-subagents`, `@bunshin-cc/bunshin` (shared memory, Claude/Codex/pi), `@tintinweb/pi-tasks` (task tracking), `@pi-unipi/workflow`, `@alexeiled/pi-plan-exec`, `@agwab/pi-workflow`, `@agimon-ai/doompi-*` (a monorepo with `doompi-task`, `doompi-team`, `doompi-git`, `doompi-workflow`; 11–25k/mo each; not opened).
- Observability packages with large downloads, not orchestrators: `@langfuse/pi-observability-plugin` (323k/mo), `@langchain/langsmith-pi-extension`, `@braintrust/pi-extension`, `@raindrop-ai/pi-agent`. These are a lead for N13 (export Attempt metrics to a trace backend); not read.
