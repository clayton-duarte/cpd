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

### 3.4 Dialogs and UI requests

Sources: `a-dialogs.md`, `b-extui.md`, `c-tools.md`. Abbreviations: `HT` = pi-durable `dist/harness/types.d.ts`; `R` = pi-coding-agent `dist/modes/rpc/rpc-mode.js`; `D` = pi-coding-agent `docs/rpc-extension-ui.md`.
pi-durable 1.3.0 has no UI API: a grep of its README and `dist` for `ui_request`, `ctx.ui` and `hasUI` finds nothing, and its README says a UI reads committed state with `viewState()` (README l.380; read in `a-dialogs.md`). So every dialog below is host plus client work.

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G39 | `ask_user_question` as a tool: 1 to 4 questions of 2 to 4 options, validation, the G2 and G4 guards, the answer text and `details` | `pi/ask-user-question/tool/types.ts:41-157`, `validate-questionnaire.ts:19-55`, `ask-user-question.ts:322,339-345`, `askguard.ts:4-103`, `adapter.ts:83-90` | partial: a tool is a `defineTool` whose `execute` may await (README Tools l.155-170; `HT:238-271`); a guard maps to `beforeTool` (`HT:714`). The tool itself is not shipped | host: port the tool to a `defineTool` | M (422 lines in `tool/` plus 104 in `askguard.ts`, both measured) | measured |
| G40 | `ask_user_question` at walker parity: one `select` per single-choice question with a "Type something." row, one `input` for multi-select | `pi/ask-user-question/rpc-fallback.ts:88-202` (select `:123`, input `:140,171`) | absent: no dialog request (`a-dialogs.md` section 5) | client: render `select` and `input` requests | S (203 lines of `rpc-fallback.ts`, measured) | measured |
| G41 | `ask_user_question` full questionnaire: tab bar, option list, preview pane, submit tab, notes, global note, external editor, collapse key | `pi/ask-user-question/ask-user-question.ts:241-257,362-382` (`ctx.ui.custom`); TUI code in `view/`, `state/`, `selectors/` | absent: `ctx.ui.custom` has no RPC form (`R:152-155`; `D:16`) | client: a `questionnaire` request kind (see the closing table). Open: whether the first cut ships G40 only | L (guess; the TS view, state and selectors are 4,362 lines, measured) | inferred |
| G42 | `ask_user_question` with no UI: the reconciler removes the tool, `execute` returns `no_ui`, a missing `custom` returns `no_custom_ui` | `pi/ask-user-question/reconcile.ts:25-47`, `ask-user-question.ts:70-73,89,270-275,336` | absent: the host knows whether a client is attached; pi-durable does not | host: no client attached, so the call fails with `no_ui` as today. Open: or the tool waits for a client (G50 shows a tool can wait) | S (guess) | inferred |
| G43 | Plan review flow: tool `cpd_plan_review`, summary message, pending entry, `cpd plan decide`, supersede, decided-elsewhere | `pi/lib/review.ts:51-159,179-195`, `pi/lib/plan-review.ts:10-63` | partial: an app entry `entry:{kind,data}` and `submit` write store state (`HT:23-36`; README Busy Conversations); `customType` and `display` have no equivalent (inferred) | host: the flow and the `cpd plan` calls | S (262 lines: `review.ts` 198 plus `plan-review.ts` 64, measured) | measured |
| G44 | Plan review dialog and review chat: `select` Approve or Change, on Change an `editor` "What should change?", Esc leaves it pending with a `notify` | `pi/lib/review-chat.ts:8-67` (select `:50`, editor `:58`, notify `:53`) | absent: no UI | client: a select and an editor dialog | S (67 lines, measured) | measured |
| G45 | `/plan-review`: reopen the waiting review dialog (Plan Lead only) | `pi/lib/plan-review-command.ts:15-33`, registered `pi/lib/startup.ts:16` | absent: no command surface | client: send `command` `plan-review`; host: resend the pending dialog | S (36 lines, measured) | measured |
| G46 | `cpd_secret_approve`: a `select` "Allow once" or "Deny" inside the tool; denial and approval texts | `pi/lib/secret.ts:20-55` (select `:49`, `no_ui` `:47`), `pi/lib/state.ts:56-63` (`ownPrompt`) | partial: the tool may await a human answer (G50); no dialog | host: the tool awaits a `select` request; client: renders it as an approval | S (58 lines, measured) | measured |
| G47 | `cpd_host_approve` (Manager only): a `select` "Allow once" or "Deny" before a host repair | `pi/lib/hostapprove.ts:36-57` (select `:54`, `no_ui` `:52`) | partial: same as G46 | host: as G46; client: as G46 | S (60 lines, measured) | measured |
| G48 | The `ctx.ui` members cpd uses: `select`, `editor`, `notify`, `input`, `setStatus`; `confirm` is used by none | `pi/lib/review-chat.ts:50,58`, `pi/lib/secret.ts:49`, `pi/lib/handle.ts:129`, `pi/compact-tools/extensions/compact-tools-silent.ts:476,498` | absent: no UI context; pi's RPC mode serialises 9 of 29 members as `extension_ui_request` (`R:83-225`; `D:12-25`) | host: reuse pi's request and response shapes as they are (inferred); client: render them | S (guess) | inferred |
| G49 | `ctx.ui` members with no RPC equivalent: `custom`, `onTerminalInput`, `setWorkingIndicator`, `setWorkingMessage`, `setWorkingVisible`, `setHiddenThinkingLabel`, factory `setWidget`, `setFooter`, `setHeader`, theme calls | `pi/ask-user-question/ask-user-question.ts:241`, `pi/compact-tools/extensions/compact-tools-progress.ts:99-139`, `pi/lib/pad.ts:82` | none-needed: pi's RPC mode makes them no-ops (`R:97-221`); with no pi in the client there is nothing to patch | none: the client draws its own working pill and footer (section 3.5) | S (guess) | read |
| G50 | A tool that waits for a human answer | `pi/ask-user-question/ask-user-question.ts:336-391`, `pi/lib/secret.ts:49` | offered: `execute` may await; no tool timeout field (`HT:238-271`). Measured: a tool awaiting a 2 s timer returned its answer and the run went on. Pending state: `tool_execution_start` in `watchEvents` and `pi.live.tools` `status:"running"`, no "waiting for human" state. A restart marks it interrupted and does not rerun it unless `replay:"safe"` (measured; `tool.js:101-118`) | host: choose `replay` per tool. Open: `safe` re-asks the question after a restart, `unsafe` tells the model it was interrupted | S (guess) | measured |
| G51 | Dialog persistence: cpd stores the pending review with `appendEntry` and reopens it at `session_start`; the ask, secret and host dialogs are not stored | `pi/lib/review.ts:67,86,123`, `docs/commands.md:3976-3977` | partial: app entries are stored; there is no pending-dialog notion, and `watch` replays nothing to a late client (README l.402) | host: hold the map of open requests by id and resend them to a client that attaches or reconnects | M (guess) | inferred |
| G52 | Dialog cancel: a newer review or a decision elsewhere closes the open select | `pi/lib/review-chat.ts:16-31,47,59` (`AbortSignal`) | absent: pi's RPC sends no cancel to the client (`R:57-60`); an `editor` has no signal (`R:174-192`) | host: a `cancel` message by request id; client: close the dialog | S (guess) | inferred |

Smallest set of UI request kinds over the socket (inferred from the rows above; `questionnaire` is the only one beyond pi's own shapes, and it is needed only for G41):

| Kind | Payload | Response | Used by |
|---|---|---|---|
| `select` | `id`, `title`, `options`, `timeout?` | `value` or `cancelled` | G44, G46, G47, G40 |
| `input` | `id`, `title`, `placeholder?` | `value` or `cancelled` | G40 |
| `editor` | `id`, `title`, `prefill?` | `value` or `cancelled` | G44 |
| `cancel` (host to client) | `id` | none | G52 |
| `notify` | `message`, `level?` | none | G44, G45 |
| `questionnaire` | `id`, `questions` with `options` and `preview?` | `answers`, `cancelled`, `globalNote?` | G41 |
| `command` (client to host) | `name`, `args` | none | G45 |
| `message` (host to client) | `customType`, `content`, `display`, `details` | none | G43 |

Reading: `confirm` and `custom` are carried by no feature. Of 14 rows, 0 are offered, 1 is none-needed (G49).

### 3.5 Rendering

Sources: `a-render.md`. `EXT` = `pi/compact-tools/extensions/compact-tools-`. Line counts are measured (`git show origin/main:<file> | wc -l`, non-test) and are the TypeScript counterpart; the Rust size is a judgement, so every Size here says guess and cites the counterpart. All rows are built in the client. pi-durable offers data only: the `pi.tool-result` entry (`dist/entries.js:20`), the `pi.live` and `pi.usage` docs (README l.378-402), and no render. Mention highlight is G38 (3.3) and is not repeated.

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G53 | Tool rows: head `╭─[pill] title`, body rails, closing `╰` row, pill colour by state, running loader row, `ask_user_question` in magenta | `` `EXT`.ts:187,285,296,361,567 ``, `compact-tools.ts:255,272,276`, `compact-tools-custom.ts:51` | partial: data only (entry plus `pi.live`) | client: draw from entry and live doc | M (guess; TS 996 + 295 + 574 + 85 lines) | measured |
| G54 | Per-tool argument summaries and the shell "calm label" parse | `EXT`-invocation.ts:81,329,395, `EXT`-shell.ts:29,245 | partial: data only (tool args) | client: show the command; classify nothing | S (guess; TS 531 + 277 lines) | measured |
| G55 | Tool grouping: `Run date + 16 more steps` head, count, running hint, failures | `EXT`-grouping.ts:44,133,150,157,199,270 | partial: data only | client: the compact group head and count | S (guess; TS 487 lines) | measured |
| G56 | Nested call summary under a parent tool | `EXT`-nested.ts:17,87 | partial: child conversation found through the call's `details` (README l.560) | client: a count line | S (guess; TS 168 lines) | measured |
| G57 | Thinking fold: one pill `Thinking…`, folded, body on rails, time `HH:MM`, Ctrl+T | `EXT`-thinking.ts:52-56,62,124,184,253,348 | partial: data only (thinking blocks of the assistant entry) | client: two states, fold and time | S (guess; TS 385 lines) | measured |
| G58 | User and assistant balloons: rounded box, tail, time, text wrapped to width minus 8 or 7 | `EXT`-balloon.ts:10,14,29, `EXT`-silent.ts:72,114,302 | partial: data only (`pi.user` entry `dist/entries.js:11`) | client: balloon widget | S (guess; TS 49 + 501 lines) | measured |
| G59 | Markdown in balloons: headings, code, lists, tables, syntax colours | pi-tui `Markdown` (`EXT`-silent.ts:12), `pi/themes/dango.json` | none-needed: input text is data; the renderer is not | client: a markdown to ratatui renderer, new work | M (guess; no TS counterpart of ours) | inferred |
| G60 | cpd message fold (`cpd`, context, decision, summary, skills): pill, first-sentence summary, `Message from <name> (<id>) (<HH:MM>)` | `pi/lib/render.ts:23,37-99,143` | partial: an app entry carries `content` and `details` (README l.416); the mapping from pi's `customType` is inferred | client: fold widget | S (guess; TS 238 lines) | measured |
| G61 | Compaction fold: `[compaction]` pill, `Compacted from N tokens` | `EXT`-compaction.ts:31,71,84 | partial: `pi.compaction` entry (`dist/entries.js:30`); whether it carries `tokensBefore` was not found | client: fold widget | S (guess; TS 91 lines) | measured |
| G62 | Editor box: rounded, border colour by working, focused, unfocused, `↑ n more` labels | `EXT`-editor.ts:88,137,145-151,163 | none-needed: no UI | client: box and colours over its own editor (G88) | S (guess; TS 231 lines) | measured |
| G63 | Header working pill in the editor's top border: moon frames, words from run events; animation clock | `EXT`-editor.ts:68, `EXT`-progress.ts:38,132, `EXT`-loader.ts:8, `EXT`-clock.ts:7 | partial: run state from `pi.live` and events `run_start`, `tool_execution_start` (`events.d.ts`) | client: pill and a tick | S (guess; TS 146 + 43 + 55 lines) | measured |
| G64 | Pills and the Dango palette: 16 indexed colours, pill spec JSON | `pi/lib/pill.ts:41`, `pi/themes/dango.json`, `pill-spec.json` | none-needed: static JSON | client: map to indexed colours; reuse the JSON as is | S (guess; TS 53 lines) | measured |
| G65 | Footer: cwd, branch, session name, tokens, cost, context, statuses (pi's own) | pi-coding-agent `dist/modes/interactive/components/footer.js:130-168,253` | partial: `pi.usage` (`dist/harness/usage.js:4`); branch and cwd are client facts | client: footer line | S (guess) | inferred |
| G66 | Chat padding: one blank cell around the root | `pi/lib/pad.ts:62-80` | none-needed | client: a margin | S (guess; TS 83 lines) | measured |
| G67 | Anchored scrolling: keep the reader's place when rows resize | `EXT`-viewport.ts:79,130,200,212-216 | none-needed: no UI | client: anchor by entry id and offset, not by line content (inferred) | S (guess; TS 234 lines) | measured |
| G68 | Fold state and toggles: Ctrl+O, Ctrl+T, click on a row | `EXT`-config.ts:35, `pi/lib/render.ts:95`, `EXT`-compaction.ts:17,60 | none-needed: client state | client: expanded flag per row | S (guess; TS 195 lines) | measured |

May simplify (inferred in `a-render.md`): per-tool argument summaries and the shell parse (G54); the failure line; the thinking hint and the 3-step Ctrl+T cycle (two states); the input cursor (ratatui's own; blink optional); nested summaries (G56); viewport anchoring by entry id (G67); silent mode.
Can drop (inferred): the claude and codex grouping vocabularies (1,187 code lines, not the default: cpd sets `style: compact`); the eased dot ramp of `compact-tools-activity.ts:100` (contract 6.9 already lists it as removed, yet it is still in the tree); the hint patch `pi/lib/hint.ts`; render caching (`EXT`-layout.ts:24,59,111); config, `/compact-tools`, double-load and the hook utility (they exist because the renderer patches pi); status pills (cockpit, not chat).
What depends on pi-tui and cannot be exported as data (read, `a-render.md` section 3; pi-tui's source is not under `node_modules`, so this is from cpd's imports): the `Editor` text buffer and key handling, the alt-screen driver and layout (`TuiAltScreen`, `ScrollView`), `Markdown`, `MouseRegion`, ANSI width and wrap helpers, the key map, and the pi component classes that cpd patches (`AssistantMessageComponent`, `ToolExecutionComponent` and others). With no pi in the client those hooks have no target; only the drawing rules carry over. Everything else (row text, times, counts, status, token numbers, mention ids, palette, pill spec, glyphs) is data from entries, `pi.live` and `pi.usage`.
Open: whether the client reuses a ratatui editor crate or writes one (G88).

### 3.6 Slash commands, keys and input forms

Sources: `b-commands.md`, `c-env.md`. `IM` = pi-coding-agent `dist/modes/interactive/interactive-mode.js`, `SC` = `dist/core/slash-commands.js`. pi's table `BUILTIN_SLASH_COMMANDS` has 23 entries (`SC:2-27`, read). Three hidden commands are dispatched but not in the table: `/debug`, `/arminsayshi`, `/dementedelves` (`IM:2663-2673`). Of the commands cpd names (`/reload`, `/plan-review`, `/mcp login`), `/plan-review` is G45 and `/mcp login` is G33 (3.3); both are not repeated here. Commands cpd does not name in `pi/skills`, `pi/lib` or `docs/commands.md` are "not named in cpd" (a grep of those paths only; a User can still type them in a pane, inferred).

The 23 built-in commands (read, `SC` order not kept; grouped as in `b-commands.md`):

| Group | Commands | Row |
|---|---|---|
| Session and tree | `/tree`, `/fork`, `/clone`, `/resume`, `/new`, `/name`, `/session`, `/import` | G74, G75, G77 |
| Context | `/compact` | G71 |
| Model and thinking | `/model`, `/thinking`, `/scoped-models`, `/login`, `/logout` | G72, G73, G78 |
| Extension and tool control | `/reload`, `/settings`, `/trust`, `/hotkeys`, `/changelog`, `/quit` | G76, G78, G79 |
| Export and report | `/copy`, `/export`, `/share`, `/bug` | G79 |

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G69 | Command dispatch, autocomplete and extension commands (they run while streaming) | `IM:467,2542-2683,2707`; command `pi/lib/plan-review-command.ts:15` | absent: no command surface | client: parse `/name args`, complete names | S (guess) | read |
| G70 | `/skill:<name>` and prompt templates by name | `IM:535`; pi `docs/slash-commands.md:56-58` | absent: no skill notion (`c-env.md` row 7); see G35 | client: expand `/skill:name` into the input text (inferred) | S (guess) | read |
| G71 | `/compact [instructions]`: manual compaction | `IM:2652,5941`; not named in cpd | offered: `compact(instructions, ctx)` (`HT:610`; README Compaction) | client: the command; host: call it (see G91) | S (guess) | read |
| G72 | `/model [provider/model]`: selector or argument | `IM:2558,4320,4454`; not named in cpd (`pi/lib/models-list.ts` lists models, a different thing) | offered: `configure({model})` (`HT:378-396`; README Per-Conversation Agent) | client: selector; host: model list from pi's `ModelRuntime` as the Worker does (`pi/durable-worker/main.ts:24-31`) | S (guess) | read |
| G73 | `/thinking [level]` | `IM:2564,4282`; not named in cpd | offered: `configure({thinkingLevel})`; levels `off` to `max` | client: the command; host: `configure` | S (guess) | read |
| G74 | `/fork`: new session from an earlier user message (pi's; cpd's `cpd session fork`, `docs/commands.md:4691`, is a different command) | `IM:2616,4593`; not named in cpd | offered: `fork(at, opts, ctx)` (`HT:622`); see G89 | client: message selector; host: `fork` | S (guess) | read |
| G75 | `/tree`: switch branches in the same session file | `IM:2626,4641`; not named in cpd | partial: see G90 | client: tree selector | M (guess) | inferred |
| G76 | `/reload`: reload extensions, skills, prompts, themes (and the `cpd-reload-after-install` command that cpd sends itself) | `pi/lib/reload-command.ts:7-23`, `pi/lib/reload.ts:17`, `docs/commands.md:3986` | partial: `registry.install(ext)` (README Reload); see G31 | client: `/reload` sends a reload request; host: re-install | S (guess) | read |
| G77 | `/clone`, `/resume`, `/new`, `/name`, `/session`, `/import` | `IM:2575,2596,2601,2621,2647,2678`; not named in cpd | none-needed: a cpd Session is cpd's own object (`cpd session ...`), not a pi file | none: cpd's commands stay. Open: whether `/new` means pi-durable `reset` (G94) | S (guess) | inferred |
| G78 | `/scoped-models`, `/login`, `/logout`, `/settings`, `/trust` | `IM:2548,2553,2631,2636,2642`; not named in cpd | absent: settings and provider auth are pi's files; pi-durable takes a plain `settings` object (`c-env.md` row 2) | none: the User keeps the shell `pi` for sign-in. Open: confirm | S (guess) | inferred |
| G79 | `/hotkeys`, `/changelog`, `/quit`, `/copy`, `/export`, `/share`, `/bug` and the hidden three | `IM:2583-2683`; not named in cpd | none-needed | client: `/quit`, `/copy`, `/hotkeys` only (inferred) | S (guess) | inferred |
| G80 | Commands the doc and the source disagree on: `/llama` is in `docs/slash-commands.md:17` but is an extension command (`dist/extensions/llama/index.js:163`); `/mcp` is in neither the table nor `slash-commands.md`, only `docs/mcp.md:28,88,90` (source `dist/extensions/mcp/index.js:1121`) | `docs/commands.md:5848` names `/mcp login <name>` (G33); `/llama` is not named in cpd | absent | none: `/llama` is not used by a Lead; `/mcp` is G33 | S (guess) | read |
| G81 | Keys: Esc abort and double-Esc, Ctrl+C, Ctrl+D, Ctrl+Z, Ctrl+L, Ctrl+P, Shift+Tab, Ctrl+T, Ctrl+O, Ctrl+X, Alt+Enter, Alt+Up; user-configurable in `keybindings.json` | `IM:2418-2479` (`pi-coding-agent/docs/keybindings.md:124-167`); not named in cpd | none-needed: no UI | client: a key map and a config file | S (guess) | read |
| G82 | Images in a submit: the model can take images; the Worker passes text only | `pi/durable-worker/harness.ts:31`, `docs/contract.md:3709` (text only); pi `IM:2500-2534`, `dist/utils/clipboard-image.js` | offered: `submit` takes `content: string or (TextContent or ImageContent)[]`, `ImageContent = {type:"image", data, mimeType}` (`HT:13,17-35`; pi-ai `dist/types.d.ts:260-264,357-361`). Resizing an image the User submits directly: not found | host: accept base64 and `mimeType`, build `ImageContent[]`; client: read the file or clipboard | S (guess) | read |
| G83 | Paste: Ctrl+V of files, image or text; right-click paste; bracketed paste; drag | `IM:2486-2534`; pi `docs/usage.md:19`; the editor is pi-tui (not read) | none-needed: the client's own input | client: bracketed paste and clipboard | S (guess) | inferred |
| G84 | `!cmd` and `!!cmd`: run a shell command, output into the context or excluded | `IM:2690-2702,5869`; not named in cpd | partial: `submit` write appends an entry without asking the model (`HT:307`); `ExecutionEnv` runs a command (`c-env.md` row 2) | host: run through the env and write the entry (inferred); client: bash input mode | M (guess) | inferred |
| G85 | `@file`: fuzzy file search and path completion | `IM:543`; pi-tui `autocomplete.js:90-115` | absent | client: file completion over the Lead's cwd | S (guess) | read |
| G86 | External editor: Ctrl+G opens `$VISUAL` or `$EDITOR` | `IM:3771-3786`; not named in cpd | none-needed | client: spawn and read back | S (guess) | read |
| G87 | Queued input: Enter steers, Alt+Enter queues a follow-up, Alt+Up and Esc bring queued messages back, input queued during compaction | `IM:2704-2723,3655-3693,3893`; not named in cpd | partial: `whenBusy` (see G95); `Submission.abort` withdraws a queued one (`HT:44-49`); no "restore to editor" | client: the keys and the editor restore; host: list and withdraw queued submissions | S (guess) | read |
| G88 | Editor behaviour: multi-line, history, cursor, wrap, autocomplete | pi-tui `Editor` (`EXT`-editor.ts:145-151) | none-needed: no UI | client: write one or take a ratatui crate (Open) | M (guess) | inferred |

### 3.7 Conversation operations

Sources: `c-conv.md`, `b-commands.md`. Of the operations below only submit with `requestId`, `watch`, `watchEvents` and usage were run (`c-conv.md`); `steer`, `followUp`, `compact`, `reset`, `fork`, `configure` and `abort` were read only, so each such row says unverified.

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G89 | Fork from an earlier entry | pi `/fork` `IM:2616,4593` (not named in cpd) | offered: `fork(entryId, {ownership, agent?, init?}, ctx)` returns a `Conversation` seeing entries up to the entry; `context(ctx,{at})` previews it (`HT:622`; README More Conversations and Forks). Unverified: not run | host: `fork`; client: message selector (G74) | S (guess) | read |
| G90 | The session tree: branches in one file, switch between them | pi `/tree` `IM:2626,4641`; double-Esc `IM:2418-2447` | partial: `ConversationRecord.parent` (`HT:267`) and fork-aware paged `entries(query, limit, cursor, ctx)`; no switch of the leaf inside one conversation found | host: serve the entry tree from `entries`; client: tree selector (G75). Open: a branch is a fork (a new conversation) or not supported | M (guess) | inferred |
| G91 | Compaction, manual: `compact(instructions)` | pi `/compact` `IM:2652,5941` (not named in cpd) | offered: `compact(instructions, ctx)` returns a `TaskId`; the summary is placed at once when idle, else at the next boundary; appends `pi.compaction`; Esc cancels (`HT:610`; README Compaction). Unverified: not run | host: `compact` and the wait on the task | S (guess) | read |
| G92 | Auto-compaction and cpd's reserve ratio | `pi/lib/compaction-ratio.ts` (`docs/commands.md:4405`: hooks `getCompactionReserveTokens`) | offered: `settings.compaction {enabled, reserveTokens, keepRecentTokens, backgroundTokens}` (`HT:476-497`); a context overflow compacts and retries once | host: the ratio as a live `settings` getter | S (guess) | read |
| G93 | The `beforeCompact` hook (decline or supply a summary) | not used by cpd today (`c-tools.md` finds only `beforeTool` in `pi/durable-worker/extension.ts:15`) | offered: `beforeCompact({reason, entries, messages, firstKept, instructions?})` returns `{decline:true}` or `{summary}` (`HT:728`) | none: no use found | S (guess) | read |
| G94 | Reset and handoff | pi `/new` `IM:2647` (not named in cpd) | offered: `reset(handoff, ctx)` writes `pi.reset`; the model no longer sees older entries, which stay in storage; a tool can ask for it with `control:{handoff}` (`HT:605,91`). Unverified: not run | host: `reset`. Open: whether `/new` is `reset` (G77) | S (guess) | read |
| G95 | Steer versus follow-up when busy | pi `IM:2715-2723`; Events `pi/lib/delivery.ts:10-20` (G02) | offered: `whenBusy` `"followUp"` (default), `"steer"` or `"reject"`; settings `steeringMode` and `followUpMode` `"all"` or `"one-at-a-time"` (`HT:23-36,484-485`; README Busy Conversations). Unverified: not run | host: pass `whenBusy` from the client's key (G87) | S (guess) | read |
| G96 | Abort | pi Esc `IM:2418-2447` | offered: `abort(ctx, {background?})` withdraws queued inputs, aborts the tasks and the owned children (`HT:627`). Whether it fires a running tool's `abortSignal` was not tested | client: Esc; host: `abort`. Open: restore the withdrawn text to the editor as pi does | S (guess) | read |
| G97 | Model and thinking switching at runtime | pi `/model` and `/thinking` (G72, G73); Ctrl+L, Ctrl+P, Shift+Tab | offered: `configure({model, thinkingLevel, ...}, ctx)` in one commit; applies from the next request (`HT:593,378-396`). Whether a level is clamped to the model: unverified | host: `configure` | S (guess) | read |
| G98 | Usage and cost | pi footer `footer.js:166-168`, `/session` `IM:2601` | offered: `docs["pi.usage"]` per conversation, `harness.usage(ctx)` summed, events `usage_changed` (`harness/usage.d.ts:6-11`; README Usage and Cost). Measured: cost is inside each `Usage` | host: forward the doc; client: footer (G65) | S (guess) | measured |
| G99 | Context-window usage (the footer's fill) | pi footer `footer.js:130-168` | absent: not found as a field of `UsageState` or the snapshot | host: compute from the last assistant `usage` and the model's window (inferred) | S (guess) | inferred |

### 3.8 Code mode and tool search

Sources: `c-env.md`. Today the Worker runner installs neither (grep of `codemode` in `pi/durable-worker/` finds nothing; `c-env.md` rows 5 and 6). Whether pi's interactive Lead uses code mode: the finding did not read the cpd pi extensions and marks it unverified. The Spec says the `codemode` tool is in the Lead's tool list; that comes from the Spec, not from a finding, so it is unverified here.

| ID | Feature | Used today (file:line) | pi-durable 1.3.0 offers | cpd builds | Size | Basis |
|---|---|---|---|---|---|---|
| G100 | Code mode: a `codemode` tool that runs a script calling `tools.<name>()` in a sandbox; unverified for the Lead | none in `pi/durable-worker/`; the Lead's use is unverified | offered, optional: `createCodemodeExtension({sandbox, namespaces?})` from `@earendil-works/pi-durable/codemode`, QuickJS sandbox, survives a restart through a journal (README Code Mode l.259-272; `dist/codemode/codemode.d.ts:47-48,154-171`) | host: install it with a `CodemodeSandbox`, namespaces for `mcp__<server>__` tools. Open: whether Leads get it | S (guess) | read |
| G101 | Tool search: `deferred: true` tools stay out of the prompt until `tool_search` loads them; aimed at large MCP sets | none: MCP tools are installed direct and all offered (`pi/durable-worker/mcp.ts:31-39`; `docs/contract.md:3762`) | offered: `createToolSearchExtension()` adds `tool_search` (limit 8); loaded tools go to `modelTools` and survive compaction (README Tools l.253; `dist/tool-search/tool-search.d.ts:8-19`; `HT:264,362`) | host: mark MCP tools `deferred` and install it if the Lead's tool list is large. Open: pi's own MCP exposure modes are not mapped (unverified) | S (guess) | read |

### 3.9 Parity summary

Counted over every gap row of section 3 (3.1 to 3.8) with:

```sh
timeout 30 awk -F'|' '/^\| G[0-9]+ /{n++; split($5,a," "); split($6,b," "); split($7,c," "); gsub(/:/,"",a[1]); gsub(/:/,"",b[1]); s[a[1]]++; w[b[1]]++; z[c[1]]++} END{print n; for(k in s) print "offers",k,s[k]; for(k in w) print "builds",k,w[k]; for(k in z) print "size",k,z[k]}' docs/research/durable-leads.md
```

COUNTS_PLACEHOLDER
