# Leads and Managers on pi-durable with a cpd chat client: P0 findings

## 1. Scope, sources and labels

Goal (the User's requirement of 2026-10-30 22:52 EDT, records hs-n586 and hs-d752):

- Leads and Managers, today pi sessions in tmux panes driven by the pi adapter `pi/lib/*`, run on pi-durable 1.3.0 as Workers already do (contract 6.12), reversibly.
- pi-durable has no interface, so cpd builds its own ratatui chat client; one pi-durable storage has one owning process, so a Node durable session host owns the Session's store and serves clients over a local socket.
- This is P0 only: findings, no product code. Dropping tmux is out of scope (P3). A Session's runner is fixed at creation; the default stays `pi`.

The doc answers four requirements: the gap list (section 3), the host and protocol (4 and 5), reversibility (6) and the cut into P1 (durable session host) and P2 (`cpd chat`) (7). Sections 4 to 8 and the Summary are written by later Jobs.

Pinned versions: pi-durable 1.3.0, pi-coding-agent 1.3.0, cpd origin/main at commit `943a11337`. Every `file:line` is of that commit unless it names a package.

Labels, on every claim:

| Label | Meaning |
|---|---|
| read | a `file:line` or a README section cite |
| measured | the command and the number |
| inferred | a judgement, stated as one; a size is `guess` unless derived from a cited line count |

Gap-table legend (the same header in every gap section; IDs G01, G02, ... run on across sections in file order):

- `pi-durable 1.3.0 offers` starts with `offered`, `partial`, `absent` or `none-needed` (no equivalent need), then the README section or `.d.ts` file:line.
- `cpd builds` starts with `host` (the Node durable session host), `client` (the Rust ratatui client `cpd chat`), `engine` (the Rust engine or core) or `none`.
- `Size`: S under 300 non-test lines, M 300 to 1,000, L over 1,000, then the line count it derives from or `guess`. `Basis`: read, measured or inferred. A finding marked unverified keeps that word in its row.

Evidence: the finding files are at `/home/clayton/.cpd/run/durable-leads/explore/`, one per question; they are a machine-local source, as in `docs/research/chat-render-cost.md`.

## 2. What exists today

Sources: the Manager's evaluation of 2026-10-30 22:52 EDT (`/tmp/cpd-building-cpd/durable-leads-eval.md`, copied and tightened here because `/tmp` is not durable); `a-dialogs.md`; `c-conv.md`.

What exists (read in the evaluation; line counts are the evaluation's, not remeasured, except the two rows that say measured):

| Piece | Today | Size (non-test lines) |
|---|---|---|
| Durable Worker runner | `pi/durable-worker/` (contract 6.12): headless Node, pi-durable 1.3.0 SQLite store per Attempt, Event delivery as `followUp` inputs, Worker guards (`workerVerdict`), MCP, crash resume, end report | ~580 |
| Runner seam | `attempts.runner` `pi`/`durable`, `worker_runner` config and status-line pill toggle (pro-d165, pro-d199) | done |
| pi adapter for Leads and Managers | `pi/lib/*`: delivery, turn-start context, guards (main, secret, noblock, ask, test, loop), plan review, review chat, MCP, reload, render (compact-tools, balloon, pad, pill, cursor) | ~5,700 |
| Dialogs and ask tool | `pi/ask-user-question` and the dialog modules of `pi/lib` | 5826 and 587 (measured by `a-dialogs.md`, totals of its tables) |
| Cockpit | `tui/src` (already ratatui: tree, plan, widgets, dialogs) | ~24,000 |
| tmux host | `core/src/host/tmux*`, `core/src/session/*` (chat slot, parked windows, moves), engine restart and ensure | ~9,000 and many tests |

What pi-durable 1.3.0 gives a UI (read, evaluation): everything as committed state: `viewState()`, `watch()` (Chord ops) and `watchEvents()` (pi-style `message_update` and `tool_execution_*` deltas over a snapshot), `submit` with `followUp`, `steer` and `write`, `abort`, `compact`, `reset`, `fork`, and a per-conversation agent (model, thinking). Constraints: README says "Experimental. The API changes without notice"; one process owns a storage, so a Rust UI cannot open the store and must talk to the Node process that owns it.

No storage lock (measured, `c-conv.md` "Storage: two processes, one SQLite file"): two processes opened one SQLite file with `Harness.open(await openNodeSqliteStorage(file), ...)`; the second open succeeded silently, process A's view went stale, and A's next write failed with `Error: ID 11 already belongs to entry`; `PRAGMA integrity_check` returned `ok`. README says "there is no cross-process locking" (read, `c-conv.md`). So the host must hold its own lock; pi-durable offers none (inferred from the grep of `storage/sqlite/*.js` and `storage/jsonl/*.js`, which `c-conv.md` reports found only `BEGIN IMMEDIATE` and a busy timeout).

Shape (read, evaluation):

```
cpd engine ── starts/keeps ──► durable session host (Node, one per Session; owns <session>.sqlite)
                                   │ unix socket: events stream (watchEvents), submit/steer/abort,
                                   │ UI requests (ask_user_question, plan review, approvals), commands
cpd chat <session> (ratatui) ◄────┘  any number of clients attach/detach; the agent never depends on one
```

The agent no longer lives in a terminal: closing the UI does nothing to the Session (what tmux gives today, done by the host instead).

## 3. Gap list

### 3.1 Runtime and Event delivery

Sources: `a-hooks.md`, `d-worker.md`, `c-conv.md`.

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G01 | Delivery of an Event while idle: stored custom message (`customType`, `details`), starts a turn | `pi/lib/handle.ts:94`, `pi/lib/delivery.ts:25-30` | partial: `submit` starts a run when idle; `entry:{kind,data}` stores an app kind while the model sees `content` (README Busy Conversations; `harness/types.d.ts:23-36`). That `customType` and `details` map onto it is inferred | host: map Event to `submit` with `requestId` `event:<id>` | S (guess) | inferred |
| G02 | Delivery of an Event while busy: steer (pr-feedback, escalation, Plan Lead) or follow-up | `pi/lib/delivery.ts:10-17,20`, `pi/lib/handle.ts:103,133,138,179` | offered: `submit({whenBusy:"steer"})` and default follow-up (README Busy Conversations; `harness/types.d.ts:23-36`) | host: the per-kind rule table | S (guess) | read |
| G03 | Silent queue: store a message without a turn; hold for the next turn while the first message is pending or the loop guard is tripped | `pi/lib/delivery.ts:23,25`, `pi/lib/host.ts:114` | partial: `submit` type `write` appends an entry without asking the model (README Busy Conversations; `types.d.ts:307`); a hold-for-next-turn (`nextTurn`) is not found | host: its own gate over the submission queue | S (guess) | read |
| G04 | Adapter errors as a chat message (Manager) or a ledger report (Lead) | `pi/lib/host.ts:82-115`, `pi/lib/delivery.ts:33-38` | partial: write entry as G03; `cpd session report` is a CLI call, already in the Worker runner (`pi/durable-worker/mcp.ts:51`) | host | S (guess) | read |
| G05 | Hidden first message of a by-user Session (section 15.7a), persisted with `details`, counted by the cursor | `pi/lib/first-message.ts:12-39`, `pi/lib/startup.ts:149-150` | partial: `GenerationTask.beforeRequest` replaces the messages of one request (README Hooks, `types.d.ts:690`); a hidden persisted message is not a named feature, so the host would store it as an entry through G03 (inferred). The Worker's first message is a spec line, not this (`pi/durable-worker/run.ts:105`) | host: `cpd session first-message` call, dedupe by entry | S (guess) | inferred |
| G06 | Context updates at turn start (`cpd-context`): visible persisted message, read of the cursor, `cpd` reads | `pi/lib/turns.ts:66-73` (hook :69), `pi/lib/updates.ts:266-325` | partial: `beforeRequest` can alter messages (`types.d.ts:690`), but it fires per request attempt including recovery (so it over-counts) and the entry must still be committed for the cursor (inferred). No named turn-start hook (`a-guards.md`) | host: submit-time hook in the host before each run | M (guess; `updates.ts` is 330 lines, cited `updates.ts:5-335`) | inferred |
| G07 | Agent state for the cockpit: idle, working, blocked, waiting, `reply` flag, reported by `cpd session state` | `pi/lib/agent-state.ts:20-91`, `pi/lib/state.ts:2,101-112`, `pi/lib/reply.ts:12-21` | partial: `viewState`/`watch` give `docs["pi.live"]` with `run` and `tools[].status` (README Watching; measured in `c-conv.md` and `c-tools.md`); no event that a human question is open (no `ui_prompt_*` analogue, inferred from the README sections read, grep `ask`/`prompt` not done in `d-worker.md`). The Worker runner sends only working, idle, stopped (`pi/durable-worker/run.ts:88,95,104,155`) | host: derive blocked and waiting from its own UI requests; `reply` flag new | M (guess) | inferred |
| G08 | Resume cursor: highest delivered Event id among prior messages with `details.delivered_to` | `pi/lib/startup.ts:145-146`, `pi/lib/cursor.ts:8-16` | partial: stored entries are readable fork-aware (`viewState` entries, README Watching) and `requestId` dedupes a resubmit, also after restart (measured in `c-conv.md`: second submit returned the same id 8). The Worker's cursor is in memory from `registered_event` (`pi/durable-worker/run.ts:111`) | host: persist the cursor in an entry or store doc | S (guess) | measured |
| G09 | Start-up replay: Events since the cursor after follower start | `pi/lib/startup.ts:145-146,203`, `pi/lib/follower.ts:98` | offered: `resume()` continues unfinished runs (README Persist and Resume; `harness/types.d.ts:646`); the follower is pure cpd code the Worker runner reuses (`pi/durable-worker/run.ts:111-136`) | none: reuse | S (guess) | read |
| G10 | Start-up and shutdown: start hook with a reason, `stopped` on exit with a bounded wait (about 2 s) | `pi/lib/adapter.ts:45-64`, `pi/lib/startup.ts:221-236`, `pi/lib/follower.ts:14` | partial: no session-start or shutdown hook is named; the Worker runner exits 143 on SIGTERM and never closes the harness (a call with no end had not returned from `Harness.close()` after 3 s, spiked, `contract.md:3749,3758`; `pi/durable-worker/run.ts:49,147-156`) | host: reuse the exit-143 convention | S (guess) | read |
| G11 | Turn-end reminder (Lead with a Plan): hidden message at the next run start | `pi/lib/turns.ts:82-110`, `pi/lib/reminder.ts:5,67-68` | partial: `GenerationTask.onYield` may `continue` the run with another user message (README Hooks); the reminder would be a user message, not hidden (inferred) | host | S (guess) | inferred |
| G12 | Policy prompt section (section 15.10) | `pi/lib/agent-state.ts:30-41`, `pi/lib/policy.ts:4-12` | offered: `section(name, fn)` (README System Prompt); the text is constant per run. Already done by the Worker (`pi/durable-worker/extension.ts:6-17`) | none: reuse | S (guess) | read |
| G13 | Time-zone prompt section, rebuilt each turn | `pi/lib/agent-state.ts:44-47`, `pi/lib/time.ts:51-56` | offered: `section(name, fn)`; README warns a section that changes every call defeats prompt caching (README System Prompt, l.284). What `zoneLine` prints is unverified. The Worker does it (`pi/durable-worker/extension.ts:6-17`) | none: reuse | S (guess) | read |
| G14 | Compaction ratio (section 15.19): reserve tokens from a ratio, by a prototype patch | `pi/lib/compaction-ratio.ts:30-69`, `pi/lib/adapter.ts:29` | partial: settings `compaction:{enabled,reserveTokens,keepRecentTokens,backgroundTokens}` and `beforeCompact` (README Compaction, `types.d.ts:445,728`); a ratio setting is not found, unverified beyond the README lines read | host: compute reserve from the window | S (guess) | read |
| G15 | Session-name sync, both ways | `pi/lib/startup.ts:142`, `pi/lib/handle.ts:124-130`, `pi/lib/agent-state.ts:84-91` | absent: no session display name in the sections read (inferred) | host: store name in its own doc; client shows it | S (guess) | inferred |
| G16 | One owner per storage; second open | none (pi sessions are files) | absent: no lock, no error on a second open (README "no cross-process locking"; measured in `c-conv.md`: ID 11 error, integrity ok) | host: its own lock | S (guess) | measured |
| G17 | Runner parts reusable as is: `harness.ts`, `mcp.ts`, `pi-mcp.ts` | `pi/durable-worker/harness.ts:12-37`, `pi/durable-worker/mcp.ts:15-117`, `pi/durable-worker/pi-mcp.ts:20-55` | offered: `Harness.open`, `resume`, `root` (README Quick Start; `harness/harness.d.ts:15`) | none | S (38 + 118 + 56 lines, read in `d-worker.md`) | read |
| G18 | Runner parts to generalise: `config.ts` (Attempt, spec), `deliver.ts` (`attempt-nudge`), `extension.ts` (Worker guard), `main.ts`, `run.ts` (end report, spec first message) | `pi/durable-worker/config.ts:5-39`, `deliver.ts:6-14`, `extension.ts:6-17`, `main.ts:22-52`, `run.ts:40-157` | none-needed: these are cpd code; the store path becomes per Session | host: a Session-keyed variant | M (guess; the five files total 39+15+17+52+157 = 280 lines read) | read |
| G19 | Worker-only parts: end report, terminate-time report, spec first message, `workerVerdict` as a whole | `pi/durable-worker/report.ts:27-51`, `pi/durable-worker/run.ts:94,105,147-153`, `pi/durable-worker/guards.ts:17-36` | none-needed | none: drop | S (51 lines of `report.ts`) | read |
| G20 | Socket server for a client: submit, view, watch, interrupt, abort | none (pi draws in-process) | partial: `viewState`, `watch`, `watchEvents`, `abort`, `submit` exist (README Watching, Agent Events; `harness/events.d.ts:184`), with no resume cursor and a fresh snapshot after 100 frames behind (measured shapes in `c-conv.md`); the `Conversation` of `pi/durable-worker/harness.ts` exposes only `submit/install/entries/closed`; no socket anywhere in pi-durable | host: the socket protocol | L (guess) | read |
| G21 | Liveness and restart of a paneless Lead or Manager: engine step E2 exempts only a durable Worker; a Lead host has no Attempt driver, `worker_pid` or `MAX_RESTARTS` | `docs/contract.md:1132`, `core/src/driver/runner.rs:34,116-122,126-153` | none-needed | engine: Session-level pid and restart bound | M (guess) | read |

Reading: of 21 rows, 5 are offered, 11 partial, 2 absent and 3 none-needed (counted in the table).
No hook gives a turn start with its origin or a human-question signal, so G01, G05, G06, G07 and G11 are host code on top of `submit` and `beforeRequest` (inferred).
The cost is the socket server and its protocol (G20, L, guess), because pi-durable has no resume cursor and no socket. Open: does the host hold its lock by a lock file or by the socket bind?

### 3.2 Guards

Sources: `a-guards.md`, `c-tools.md`. The Worker runner ports its guards in one `beforeTool` hook, `hook(ToolTask, { beforeTool: (call) => workerVerdict(call, where) })` (`pi/durable-worker/extension.ts:15`, `pi/durable-worker/guards.ts:17-36`). `beforeTool` can block (first `block` wins, a throw blocks) or rewrite arguments (`harness/types.d.ts:714-717`; README Hooks). The adapter runs all but loop in one `tool_call` handler (`pi/lib/adapter.ts:67-102`).

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G22 | Guard main (section 15.16): no change to a primary checkout by edit, write or bash | `pi/lib/adapter.ts:71-77`, `pi/lib/mainguard.ts:10-28,117-123` | offered: `beforeTool` (README Hooks; `harness/types.d.ts:714`). Already ported by the Worker: `pi/durable-worker/guards.ts:22-29`. That pi-durable's built-in tools are named `edit`, `write`, `bash` is unverified | none: reuse; the host passes the conversation cwd | S (guess) | read |
| G23 | Guard secret (section 15.17): refuse raw `op` and `cpd secret approve` in bash | `pi/lib/adapter.ts:78-79`, `pi/lib/secretguard.ts:5-16` | offered: `beforeTool`. Ported by the Worker: `pi/durable-worker/guards.ts:30-31` | none: reuse | S (guess) | read |
| G24 | Guard noblock (section 15.11): a Lead or Manager bash may not wait over 30 s, and a missing `timeout` is set to 30 | `pi/lib/adapter.ts:94-98`, `pi/lib/noblock.ts:6,42-82` | offered: `beforeTool` can block and rewrite `arguments` (`harness/types.d.ts:714-717`), so the default-timeout rewrite fits (inferred). Not ported: `pi/durable-worker/guards.ts:1-5` has no `noblock`. Whether pi-durable's `bash` has a `timeout` input is unverified | host: add to a Lead verdict | S (guess; the rule is `noblock.ts:42-82`, 41 lines) | inferred |
| G25 | Guard ask G2: an `ask_user_question` call carries at most 1500 characters | `pi/lib/adapter.ts:83-90`, `pi/lib/askguard.ts:4,29-32` | offered: `beforeTool`. Not ported (no `askguard` import, `guards.ts:1-5`; the Worker has no `ask_user_question`) | host | S (guess) | read |
| G26 | Guard ask G4 (Lead and Manager): every label cited in the call (A1, B2-B4) must be stated in the call or the visible chat so far | `pi/lib/adapter.ts:86`, `pi/lib/askguard.ts:86-104` | partial: needs conversation history; `beforeTool` gets `(call, api, context)` and `call.parent` (`harness/types.d.ts:705-714`), but whether `Context` exposes entries in the shape G4 reads (`askguard.ts:91-98`) was not read, unverified. `viewState` entries (README Watching) are the other source (inferred) | host: keep the visible chat in the host and pass it in | S (guess) | inferred |
| G27 | Guard test (section 15.12): a Worker may not run an unfiltered `cargo test` or `npm test` | `pi/lib/adapter.ts:100-101`, `pi/lib/testguard.ts:47-83` | offered: `beforeTool`. Ported by the Worker, applied unconditionally: `pi/durable-worker/guards.ts:34-35`. A Lead host must not apply it (`docs/commands.md:4216`) | host: a role switch around the shared verdict | S (guess) | read |
| G28 | Guard loop (section 15.15): more than 20 self-started turns in 600 s trips; deliveries are held; reason `loop-guard`; entry `cpd-loop-guard` | `pi/lib/loop-guard.ts:14-59`, `pi/lib/adapter.ts:42` | absent: the guard runs on pi `before_agent_start`, not `tool_call`; no named pi-durable hook is a turn start with who started it. `beforeRequest` fires per request attempt and over-counts retries (`harness/types.d.ts:690`); `afterResponse`, `afterTools`, `onYield` are the other generation hooks (`types.d.ts:696-702`). The host, which submits the input, may count there (inferred). State persists via "Your Own State" (README, inferred fit) | host: window, trip, gate and entry | M (guess) | inferred |
| G29 | Guard host-approve (section 15.18): refuse raw `cpd host approve`; the model uses the tool `cpd_host_approve` | `pi/lib/adapter.ts:80-81`, `pi/lib/hostapprove.ts:9-17,28-59` | offered: `beforeTool` for the guard; ported by the Worker: `pi/durable-worker/guards.ts:32-33`. The tool is not in the runner: it opens `host.ctx.ui.select` (`hostapprove.ts:51-54`) | host: the guard (reuse). client: the approval dialog. Same for `cpd_secret_approve` (`pi/lib/secret.ts:46-49`) | M (guess) | read |
| G30 | Guard explore (section 15.22): scout and research Workers, keyed on `CPD_JOB_KIND` | `pi/lib/adapter.ts:69-70`, `pi/lib/explore.ts:94-173` | offered: `beforeTool`. Ported: `pi/durable-worker/guards.ts:18-21`. Never applies to a Lead or Manager (`explore.ts:171`) | none: leave out | S (guess) | read |

Reading: of 9 rows, 7 are offered, 1 partial (G26) and 1 absent (G28) (counted in the table).
The Worker already ports main, secret, host-approve (guard only), test and explore; a Lead host adds noblock, ask G2 and G4, loop and a role switch.
The loop guard costs most (G28, M, guess): no named pi-durable hook gives a turn start with its origin; ask G4 needs history `beforeTool` may not carry (unverified).

### 3.3 Operations

Sources: `a-ops.md`, `c-env.md`. The durable Worker runner has none of reload, `/mcp login`, skill notice, retier and mentions (grep `reload\|skill\|mention\|retier` in `pi/durable-worker` finds only `thinkingLevel`, read in `a-ops.md`); it has MCP attach and replay.

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G31 | Reload after an install (section 15.6): poll the adapter path every 5 s, validate the package, reload only when idle, no pending message, state idle, editor empty | `pi/lib/reload.ts:14,70-76,97-132,173-300`, `pi/lib/reload-command.ts:7-23`, `pi/lib/startup.ts:204-216` | partial: `registry.install(ext)` with an installed name replaces it in place; a running call keeps the old code (README Reload). No file watch, validation or idle guard. A changed file needs a cache-busting import URL (inferred, not in README). Worker: none | host: watch, validate, guard, `install` | M (guess; the poll alone is `reload.ts:173-300`, 128 lines) | read |
| G32 | MCP attach and replay: Event `mcp-attached` registers a server; replay at every start | `pi/lib/mcp.ts:14-53`, `pi/lib/handle.ts:114-123`, `pi/lib/startup.ts:195` | partial: tools via `defineExtension` and `conversation.install` (README Extensions); no MCP client in pi-durable. The Worker runner has it: `pi/durable-worker/mcp.ts:43-117`, replay `:92-109`, wired `run.ts:107-109`, via pi's client `pi/durable-worker/pi-mcp.ts:20-55` | none: reuse the Worker's handler as is | S (118 lines, `mcp.ts`) | read |
| G33 | `/mcp login`: pi's own command and manager UI; sign-in picked up on the next turn | pi's `extensions/mcp/index.js:939-969,1121` (pi-coding-agent); cpd only mounts it, `pi/lib/mcp.ts:9` | absent: no UI. The Worker fails a `needs-auth` server with "run pi mcp login" (`pi/durable-worker/pi-mcp.ts:32-38`), told as a follow-up (`mcp.ts:48-59`) | client: a `/mcp login` command (Open: whether it relays to a host that runs the flow, or the User keeps using the shell `pi mcp login`) | M (guess) | read |
| G34 | Skill-changed notice (section 15.23): sha256 of the skill files against a baseline entry; message at the next turn | `pi/lib/skills.ts:18-107`, `pi/lib/startup.ts:197-198` | absent: no skill notion in pi-durable (grep `skill` in `dist` finds only a test helper; README has 0 hits, `c-env.md`). `appendEntry` and `before_agent_start` have no shown equivalent (inferred). A Worker runs without skills (`docs/commands.md:4469`) | host: digest and baseline in its own doc | S (guess) | read |
| G35 | Skills in the prompt: name, description and path of `cpd-lead` or `cpd-manager` in the system prompt; the model `read`s `SKILL.md` | pi's skill loader (`docs/skills.md` in pi-coding-agent); cpd role skills `pi/lib/skills.ts:18-20` | absent: the host supplies a prompt section. pi exports `formatSkillsForPrompt` and `loadSkills` (`PC/dist/core/skills.d.ts:47,62`, exported `PC/dist/index.d.ts:23`); a section is `section(key, fn)` (README System Prompt). `/skill:name` is a client command the host expands (inferred) | host: one section. client: `/skill` | S (guess) | read |
| G36 | Project context files (AGENTS.md and the like) in the prompt | pi's loader `PC/dist/core/resource-loader.d.ts:63-69` | absent in pi-durable; the Worker reads them once at start with `loadProjectContextFiles` (`pi/durable-worker/main.ts:32`, `run.ts:36-37,68`) into the section `cpd-context-files` (`extension.ts:8`) | none: reuse; a re-read on reload is the host's choice (inferred) | S (guess) | read |
| G37 | Retier at idle (section 10.15): `session-retiered` Event, switch model and thinking when idle and nothing queued; `switched` or `switch-failed` back to cpd | `pi/lib/retier.ts:6-73`, `pi/lib/agent-state.ts:80`, `pi/lib/handle.ts:110-113` | partial: `configure({model, thinkingLevel})` in one commit, applied from the next request (README Per-Conversation Agent; `harness/types.d.ts:378-396`, read only, not run). Idle detection is not offered; whether durable clamps a thinking level is unverified (`c-conv.md`). The Worker drops the Event (`pi/durable-worker/deliver.ts`, `contract.md:3709`) | host: idle test, `configure`, report to cpd | S (guess) | read |
| G38 | Mention highlighting: Lead and Manager ids in answers, magenta italic outside code | `pi/lib/mentions.ts:8-66`, `pi/lib/mentions-refresh.ts:9-26`, `pi/compact-tools/extensions/compact-tools-silent.ts:44-47,350-351` | none-needed: pi-durable has no render or UI | client: paint at render; id set from `cpd` lists | S (guess) | read |

Reading: of 8 rows, 0 are offered, 3 partial (G31, G32, G37) and 4 absent (G33, G34, G35, G36), 1 none-needed (G38) (counted in the table).
Only MCP attach (G32) is already built for pi-durable; everything else here is host or client work with no pi-durable support.
Reload (G31) costs most (M, guess): watch, validation and the idle guard all move to the host.
