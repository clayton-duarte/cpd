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
