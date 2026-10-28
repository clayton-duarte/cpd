# ratatui chat — parity audit, runtime half (tools, commands, event loop, storage, hosting)

Status: research (Job `parity-runtime`, Plan `ratatui-chat-research`, decision pro-d166). Nothing in the product is changed.
Written 2026-10-28 EDT. Evidence file for the synthesis Job; the other half is `ratatui-chat-parity-core.md`.

Labels: **read** = I opened the file and line; **ran** = I ran the command (it is in the file); **unverified** = inferred, with why.
Sources: cpd at this worktree (cut from main, HEAD `d943c49`, **ran** `git show HEAD --stat`, committed 2026-10-28 14:56 EDT); pi repo `earendil-works/pi` cloned at tag `v1.4.0` into `/tmp/pra1` (path `src/packages/…`, shown below as `pi:packages/…`); installed pi 1.3.0 at `~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent`. Both the clone and `/tmp/pra1` were deleted at the end (**ran** `rm -rf /tmp/pra1`, see §9).

## 0. Result in six lines

1. pi-durable 1.4.0 is a library (a `Harness` over SQLite/JSONL storage), not a program: **read** `pi:packages/durable/README.md:1-30`. It has no TUI, no slash commands, no `ctx.ui`, no `pi.events`, no extension loader for pi extensions. Everything in `pi/lib/*` that calls `pi.on`, `pi.registerCommand`, `pi.registerTool` or `ctx.ui` has to be re-hosted on durable's own extension API (`defineExtension`, hooks, tasks), or moved into the Rust cockpit.
2. The blocking-tool pattern exists in durable only in a weak form: a **tool task whose `execute()` awaits** (replay policy decides what a crash does) or a **task that parks in `waiting`/`sleeping`** (`docs/spec.md:1762-1769`). Neither is a "pending question the UI can list". The cockpit would answer by writing a document or submission into the same storage; durable has no example of it (§2).
3. The cpd driver's `judge` (A5) and `metrics::parse_file` (A9) read pi's JSONL session file and its `stopReason` values. pi-durable stores `pi.assistant` entries whose `model[0]` is the same `AssistantMessage` shape (**read** `pi:packages/durable/src/entries.ts:17`, `pi:packages/ai/src/types.ts:578-600`), but in SQLite, so the readers are rewritten, not reused (§5, §6).
4. pi-durable has a fork primitive: `Conversation.fork(entryId, options, context)` (**read** `pi:packages/durable/src/harness/types.ts:763`; README `:631-634`). The statement in `docs/research/pi-durable.md:134` "Pi Durable cannot fork a pi session file" is still true of a *pi JSONL file* (it cannot import one) but is no longer a reason review Jobs cannot fork: a durable Lead conversation forks natively (§3).
5. Hosting: one Node process per Session is the only model that keeps cpd's per-Session lifecycle (`session run`, driver, engine liveness) unchanged; storage is "one process owns a storage at a time" (**read** README `:790`). One shared host would be a rewrite of liveness, stop and crash rules (§7).
6. Size: ~5.5 k lines of non-test adapter TypeScript exist today (**ran** §8); I estimate 2.5–3.5 k lines of TypeScript to re-host what must be rebuilt, plus 2–3.5 k lines of Rust, in 9–12 PRs (§8). Every figure has its basis there.

## 1. Version note: cpd runs pi 1.3.0, this audit read 1.4.0

- **ran** `grep -m1 '"version"' …/pi-coding-agent/package.json` → `"version": "1.3.0"`; the clone's `packages/coding-agent/package.json` is `1.4.0`; `npm view @earendil-works/pi-durable version dist-tags --json` → `1.4.0`, `latest: 1.4.0`.
- **ran** the three events the adapter depends on that are pi additions (`ui_prompt_start`, `agent_settled`, `session_info_changed`): `grep -c <event> dist/core/extensions/types.d.ts` in 1.3.0 → `2` each, and all three are in 1.4.0 at `pi:packages/coding-agent/src/core/extensions/types.ts:1043`, `:1034`, `:765`. No difference met for them.
- **ran** the built-in slash command lists, `diff <(grep -o 'name: "[a-z-]*"' 1.3.0 slash-commands.js | sort) <(… 1.4.0 slash-commands.ts | sort)` → empty (`same-commands`). No difference met.
- I did not diff the rest of the two versions: **unverified** beyond those two checks.

## 2. (h) Interactive blocking tools

### 2.1 How they work today

| | Where cpd implements it | How the answer reaches the model |
|---|---|---|
| `ask_user_question` | vendored fork `pi/ask-user-question/ask-user-question.ts:377-455` (`registerAskUserQuestionTool`); refusal rules `pi/lib/askguard.ts:1-30` (G2 limit 1500 chars; G4); tool is allow-listed for Leads at `core/src/host/pi/launch.rs:15` | **read** `execute()` mounts a `QuestionnaireSession` in pi's editor slot via `ctx.ui.custom` (`:430-446`), awaits it, and returns `buildQuestionnaireResponse(...)` as the **tool result** of the same tool call (`:446-453`). While it waits, the adapter reports a blocked reason via `ui_prompt_start`/`_end` (`pi/lib/agent-state.ts:68-69`) and `tool_result` → `facts.release()` (`:71-73`). |
| `cpd_plan_review` | `pi/lib/review.ts:238-267` (`registerTool`) | **read** one dialog (Approve / Change …) in chat via `pi/lib/review-chat.ts`; the decision is a tool result and a cpd call; optional plannotator link. Reopened by `/plan-review` (`pi/lib/plan-review-command.ts:15`). |
| plannotator | npm package `@plannotator/pi-extension` (a pi extension, loaded through `lead_pi_packages`, `core/src/config/file.rs:54`); cpd side: `pi/lib/plannotator.ts:1-60` (request over `pi.events.emit("plannotator:request", …)`, 15 s answer timeout), `pntrack.ts` (registry), `pnguard.ts:19-40` (G1–G3: PLANNOTATOR_REMOTE=1, non-loopback host, port range) | **read** the tool calls `plannotator`, `plannotator_submit_plan`, `plannotator_mark_done`, `plannotator_inbox` are the package's; cpd only guards them in `tool_call` (`pi/lib/adapter.ts:59`) and polls after `tool_result` (`:95-102`). The User answers in a browser page; the package's tool result carries the verdict. |
| `cpd_secret_approve`, `cpd_host_approve` | `pi/lib/secret.ts:12-58`, `pi/lib/hostapprove.ts:28-60` | **read** a one-dialog tool (`ctx.ui`); Allow/Deny is the tool result. Not in the brief's list; found by `git grep -n 'registerTool(' -- pi`. |

The pattern is the same for all: **a tool call that awaits the User inside `execute()`, with the dialog drawn by the pi TUI in the same process**. The wait has no bound in cpd code (the questionnaire) or a 15 s bound for the plannotator *request*, not for the review itself. **read** the above; the unbounded questionnaire is **unverified** against the vendored `QuestionnaireSession` internals (I did not read `view/*`).

### 2.2 What pi-coding-agent provides (and why the pattern is tied to it)

- **read** `ExtensionContext.hasUI`, `ctx.ui.select/confirm/input/editor/custom`, and the events `ui_prompt_start` / `ui_prompt_end` are pi's (`pi:packages/coding-agent/src/core/extensions/types.ts:1043`; `ui_prompt_end` next to it).
- **read** In RPC mode pi forwards `select`, `confirm`, `input`, `editor` as `extension_ui_request` messages (`pi:packages/coding-agent/src/modes/rpc/rpc-types.ts:253-256`) and `custom()` does nothing there (`pi:packages/coding-agent/src/modes/rpc/rpc-mode.ts:228`). The vendored tool already has a dialog-walker for RPC (`pi/ask-user-question/ask-user-question.ts:419-421`, `rpc-fallback.ts`, 203 lines, **ran** `wc -l`).

### 2.3 What pi-durable provides

**ran** `grep -rniE 'pending|prompt|signal|waiting' pi:packages/durable/src --include=*.ts -l` and `grep -rniE 'approval|human|ask the user|question' durable/README.md durable/docs/spec.md`: no primitive for "ask a human". The hits are the hook example `beforeTool → { block: "Needs approval" }` (README `:235`; spec `:2795`), a note that humans are external effects that must not be awaited on the Session line (spec `:1701`, `:5522`), and nothing else. `packages/chord/src` has no such primitive either (**ran** `grep -rniE 'approval|approve|confirm|human|interactive|question' chord/src -l` → only `loader.ts`, unrelated: **unverified**, I did not open it).

What durable gives that a blocking tool is built from:

| Piece | Source |
|---|---|
| A tool call is its own durable task; its intent is committed before `execute()` runs; after a crash it reruns only if the tool says `replay: "safe"`, otherwise the model gets an `interrupted` error result | **read** README `:277` |
| A task can park: `waiting` on child tasks (`on`, `policy`) or `sleeping` until a clock time, without an invocation; a host wakeup calls `resume()` | **read** spec `:1762-1769`, `:1941`; `resume()` at spec `:676`, `:744` |
| A tool can end the run without another model request: `control: { terminate: true }` | **read** README `:277`; `pi:packages/durable/src/harness/generation.ts:678-713` |
| Typed documents beside the transcript, changed in commits, observable with `watchDoc` | **read** README `:98`, `:749`; `harness.watchDoc` at `pi:packages/durable/src/harness/tool.ts:389` |
| `submit({ type: "input", whenBusy: "steer" })` and `Submission.wait()` | **read** README `:506-517`, `pi:packages/durable/docs/spec.md:571` |

### 2.4 How a durable blocking tool would work

Closest example in the repo: **none for a human wait**. The nearest are `test/examples/23-subagent-background.ts` (a tool starts a background conversation, awaits `submission.wait(taskContext)` at `:91-92`, then commits a report, 364 lines, **ran** `wc -l`) and `24-child-tasks.ts` (a task parks `waiting` on four child tasks, 146 lines). Neither waits for a person.

Design that follows from the sources (**unverified**: no example runs it; it uses only the pieces in §2.3):

1. The tool `ask_user_question` is a durable tool. On call it commits a document `Pending = { id, kind: "question", payload, answer?: … }` in the same transaction that records the intent (`tx.doc`, README `:749`), then waits.
2. Waiting is either (a) `execute()` awaits a promise that the host resolves when it sees the answer in the document (cheap, but a crash loses the await; with `replay: "safe"` the tool reruns and finds the pending document again, so it re-waits: the document makes the rerun idempotent), or (b) the tool returns a task state `waiting`/`sleeping` and is resumed by the host (`resume()`) once the answer is committed. (b) survives restarts without a live await, but needs a task type of cpd's own (`defineTask`, as `24-child-tasks.ts`).
3. The cockpit lists `Pending` documents (`watchDoc`, or a read of the storage) as the "blocked" reason that `facts.prompt(true)` produces today, draws the questionnaire in ratatui, and **commits the answer** (a document write; the host process, which owns the storage, is the only writer: "no cross-process locking", README `:790`). So the answer path goes cockpit → host process → storage, not cockpit → SQLite directly.
4. The tool result is the answer text, unchanged from `buildQuestionnaireResponse` (`pi/ask-user-question/tool/response-envelope.ts`, 69 lines).

What the cockpit must do to answer it: (i) learn a question exists (watch or poll the pending list), (ii) render the questionnaire, which is the 5.9 k-line vendored TUI today (**ran** §8) and must be a ratatui widget (another Job, `widgets`), (iii) send the answer to the host process by a channel that exists today: `cpd session send`-style Events (`core/src/session/send.rs`, 195 lines) or a new `cpd` command, (iv) show a stale/lost state when the host died (document says pending, no process). plannotator has no durable analogue: it is a pi extension using `pi.events`; it must be either a separate process (its web page is already outside pi) driven by a durable tool, or dropped for an in-cockpit plan view. **Unverified** whether the `@plannotator/pi-extension` package can run outside pi; I did not read its source.

### 2.5 Gap

| Item | Gap | Close it by |
|---|---|---|
| Blocking tool + pending list | No human-wait primitive; one extension of ~150 TypeScript lines (tool + `Pending` document + resolver) | Build. Basis: `pi/lib/secret.ts` 58 lines + `pi/lib/hostapprove.ts` 60 lines are the same shape (one dialog tool), **ran** `wc -l`; `24-child-tasks.ts` 146 lines for the task variant |
| Questionnaire UI | pi TUI component (`ctx.ui.custom`) not available | Port to ratatui (outside this half) |
| Plannotator | pi extension on `pi.events` | Keep as a subprocess + durable tool (~100 lines: basis `pi/lib/plannotator.ts` 78 + `plan-review.ts` 77), or replace by an in-cockpit review |
| G1–G4 guards (`askguard`, `pnguard`) | Re-hosted as durable `beforeTool` hooks (`ToolTask`, README `:235`) | Port, ~190 lines. Basis: `askguard.ts` 104 + `pnguard.ts` 40 + `mainguard.ts` is core's, not here |

## 3. (i) Commands

### 3.1 Every command and tool the adapter registers

**ran** `git grep -n 'registerCommand(\|registerTool(\|registerMessageRenderer(\|registerMcpServer(' -- pi ':!*test*'` gave exactly these registrations:

| Registration | File:line | What it does | Fate |
|---|---|---|---|
| command `plan-review` | `pi/lib/plan-review-command.ts:15` | reopens the pending chat Plan-review dialog | **Becomes a cockpit action** (the dialog is a cockpit view; the Pending list of §2.4 replaces it) |
| command `cpd-reload-after-install` (`RELOAD_COMMAND`) | `pi/lib/reload-command.ts:11` (`pi/lib/reload.ts:12`) | the poll types it to reload pi after an install | **Dropped**: a durable host reloads by restart/reconfigure (`Conversation.configure`, README `:381-396`; extensions are stored by name). Basis: `docs/research/pi-reload.md` |
| command `compact-tools` | `pi/compact-tools/extensions/compact-tools-command.ts:87` | choose tool-row style | **Dropped** (the row style is a ratatui widget choice) |
| command `silent` | `pi/compact-tools/extensions/compact-tools-silent.ts:372` | show only prompts and final answers | **Becomes a cockpit action** (a filter of the chat view) |
| tool `ask_user_question` | `pi/ask-user-question/ask-user-question.ts:377` | §2 | rebuilt (§2.4) |
| tool `cpd_plan_review` | `pi/lib/review.ts:241` | §2 | rebuilt |
| tool `cpd_secret_approve` | `pi/lib/secret.ts:15` | approve one secret command | rebuilt as a blocking tool |
| tool `cpd_host_approve` | `pi/lib/hostapprove.ts:31` | approve a checkout repair | rebuilt as a blocking tool |
| MCP servers | `pi/lib/mcp.ts:13` (`pi.registerMcpServer`) | gives pi the cpd MCP servers | out of this half (parity-core covers MCP) |
| message renderers | `pi/lib/adapter.ts:31` (`pi.registerMessageRenderer`) | draw `cpd` custom messages | **Becomes a cockpit view** (ratatui renders the `cpd` entry kind directly) |

Hooks the adapter also uses (not commands): `session_start`/`session_shutdown`/`tool_call`/`tool_result` (`pi/lib/adapter.ts:37,48,59,95`); `before_agent_start`, `input`, `agent_start`, `ui_prompt_*`, `agent_settled`, `session_info_changed` (`pi/lib/agent-state.ts:24-84`); `before_agent_start` (`first-message.ts:15`, `loop-guard.ts:48`, `turns.ts:58,74`); `agent_end` (`turns.ts:84`). **read** all of them; fate in §4.

### 3.2 pi's own slash commands the cockpit chat would lose

pi has 24 built-in slash commands (**read** `pi:packages/coding-agent/src/core/slash-commands.ts:20-43`). Those the brief names, and the rest that cpd Sessions can reach:

| Feature | Where cpd uses it | pi-coding-agent | pi-durable | Gap | Close it |
|---|---|---|---|---|---|
| `/compact` | typed by the User; automatic compaction is pi's | `handleCompactCommand` `pi:packages/coding-agent/src/modes/interactive/interactive-mode.ts:7053`; `AgentSession.compact` `core/agent-session.ts:2828` | `Conversation.compact(instructions, context)` and auto-compaction settings (README `:550-575`) | small: wire a cockpit key to `compact()` (compaction itself belongs to parity-core) | Build: ~40 lines TS host + ~30 Rust (key, send) — basis `pi/lib/reload-command.ts` 30 lines |
| `/new` | the User clears a chat | `interactive-mode.ts:3261-3264`; `agent-session-runtime.ts:226` (`newSession`) | `Conversation.reset(handoffNote, context)` (README `:545`) and `createConversation` | none functional; cpd's identity (`--session-id`) must map to a conversation id | Port: map Session id → conversation id in the host (~50 lines TS) |
| `/reload` | rare; the adapter's own poll uses `ctx.reload()` (`pi/lib/reload-command.ts:25`) | `interactive-mode.ts:3274,6403`; `agent-session.ts:3720` | `configure({extensions})` applies from the next request; host `Harness` reopen for new code | covered: durable reload needs no pi reload dance | Drop the 300-line `pi/lib/reload.ts` (**ran** `wc -l`) after verifying a host restart is cheap: **unverified** (no measurement of durable reopen time) |
| `/mcp` | User inspects MCP servers | extension `pi:packages/coding-agent/src/extensions/mcp/index.ts:1260` | nothing; **ran** `grep -rniE 'mcp' durable/src -l` → only `codemode/*` and `tool-search/tool-search.ts` (name prefixes, no client). Client is `@earendil-works/pi-mcp` (`pi:packages/mcp`, 1.4.0) | durable has no MCP client; parity-core owns the MCP half | Take `@earendil-works/pi-mcp` as a dependency; the bridge is `api.executeTool` (README `:309`) |
| `/login` | provider auth by the User | `handleLoginCommand` `interactive-mode.ts:5824`; `auth-storage.ts` (507 lines, auth.json, file lock) | **ran** `grep -rniE 'login\|oauth\|auth.json' durable/src durable/README.md` → one hit (the word "login" in a prompt example). Auth lives in `@earendil-works/pi-ai` (`pi:packages/ai/src/auth/*`, `oauth/*`) | no login UI. cpd already needs credentials in the environment to run | Take pi-ai's `CredentialStore`/oauth; build a cockpit login view (~300 lines Rust: basis `radius-login-selector.ts` + `oauth-selector.ts`, **ran** see §8) or keep `pi /login` as a one-off command outside the cockpit. Recommended simplest: keep it outside (**unverified** UX call) |
| `/model` | the User changes the model; the adapter retiers (`pi/lib/retier.ts:73` lines → `setModel`) | `handleModelCommand` `interactive-mode.ts:5167` | `Conversation.configure({ model: { provider, modelId }, thinkingLevel })`, applies from the next request (README `:381-396`) | selector UI missing; the switch itself exists | Build a ratatui picker (widgets Job); retier port ~70 lines TS (basis `retier.ts` 73) |
| `/fork`, `/clone`, `/tree`, `/resume` | cpd does **not** use pi's; it uses `cpd session fork/open/resume` | `agent-session-runtime.ts:196,262`, `session-manager.ts:1632,1815` | `fork`, `createConversation`, document `pi.live` etc. | see §3.3 | |
| `/thinking`, `/settings`, `/export`, `/import`, `/share`, `/copy`, `/name`, `/session`, `/hotkeys`, `/changelog`, `/trust`, `/logout`, `/quit`, `/bug`, `/scoped-models` | used ad hoc by a User at the chat; no cpd code depends on them (**read** `git grep` for them in `core/ pi/` is not exhaustive: **unverified**) | `slash-commands.ts:20-43` | nothing (no UI) | each is a cockpit decision | **Dropped** unless the User asks; `/name` already has the cpd-owned equivalent `session rename`, which the adapter forwards (`agent-state.ts:84-90`) |

### 3.3 Fork, resume, open, park

| Feature | cpd today | pi-coding-agent | pi-durable | Gap | Close it |
|---|---|---|---|---|---|
| `cpd session fork` (§6.7, `docs/commands.md:2473`) | registers a Lead in a parked window; the pane waits until the source Lead is idle (`core/src/session/fork_wait.rs:79-88` `turn_finished`: last `message` entry is an assistant with `stopReason` `stop`; quiet for `fork_quiet_seconds`, :70-74), then `launch.rs:117-134` starts `pi --fork <source file>` | `--fork` flag; `SessionManager.forkFrom` `pi:packages/coding-agent/src/core/session-manager.ts:1815`; `createBranchedSession` `:1632` | **`Conversation.fork(at, options, context)`** `pi:packages/durable/src/harness/types.ts:763`; README `:631-634`: sees parent entries to `at`, keeps the parent's agent as of that entry, fresh provider session id; `fork: "initial"\|"current"\|"asOf"` setting `:738` | The idle test must read durable (`pi.live` document / no running task) instead of a file; the fork primitive exists | Port `wait_for_source` (109 lines) to a storage read: ~60 Rust lines + a host command `fork` ~60 TS. Basis: `fork_wait.rs` 109 lines |
| Review Job: Worker forks the Lead (B2, `launch.rs:165-177`) | `pi --fork` of the Lead's file | same | `fork()` inside one storage; **cross-process**: the Worker is another process, the Lead's storage is owned by the Lead's host (README `:790`). The fork must be made by the Lead's host or the Worker opens the same SQLite file read-only (**unverified**: README forbids two owners; I found no read-only open: **ran** `grep -rn 'readOnly\|ReadOnlySession' durable/src` → no hit) | one-owner rule | Ask the Lead's host to fork (command over the Event stream) or run review Workers on pi. This corrects `docs/research/pi-durable.md:134` (cited above): durable forks natively, but **not across processes**. |
| `cpd session open` / `park` / `resume` (`commands.md:2506,2532,2965`) | move a Session's tmux window in/out of the chat slot; `resume` starts pi with `--session <file>` (`launch.rs:123`, a model named first, hs-d506) | `--session <file>` | `harness.root()`/`conversation(id)` reopen; unfinished work resumes (README `:127`) | the "park" is a tmux operation: with a ratatui chat there is no window to move; open/park become "which conversation the chat widget shows" (cockpit state, no process move) | Rust: delete most of `core/src/session/moves.rs` (572 lines, **ran**); keep the state rows. Net effect is **negative** size, but the `in_chat_slot` refusals (`docs/commands.md:465,667,1142,1189`) are rewritten |

## 4. (j) The adapter's event loop

### 4.1 Structure today

**read** `pi/lib/adapter.ts` wires hooks to: a `follower` (`pi/lib/follower.ts`, 160 lines) that spawns `cpd events follow` as a child, restarts it with 1 s → 30 s back-off (`Timing`, `:5-20`), and never retries the permanent error codes (`PERMANENT_CODES`, `:27-37`); `delivery.ts` (44 lines) turns an Event into a pi custom message with `deliverAs: "steer" | "followUp"` and `triggerTurn` (`LEAD_MESSAGES`, `:9-17`) and queues a turn-starting send while the first message is pending or the loop guard is tripped (`gated`, `:20`); `agent-state.ts` (92 lines) keeps `facts.active` (agent start/settle) and the prompt reason, then calls `host.refresh(r)`, which reports state to cpd (`cpd session state`); `turns.ts` (100) counts assistant turns for the nudge baseline; `startup.ts` (268) starts the run; `reminder.ts`, `first-message.ts` handle the first message and reminders; `loop-guard.ts` stops runaway loops.

### 4.2 Feature table

| Feature | cpd today | pi-coding-agent | pi-durable | Gap | Close it |
|---|---|---|---|---|---|
| Follow the cpd Event stream | `pi/lib/follower.ts:1-160` (child process, back-off) | extension host keeps the process alive | nothing; durable has no cpd knowledge. The host is plain Node, so the same follower (child `cpd events follow`) can run unchanged | none, but it must be re-hosted | Reuse as is (runner-neutral TS: no `pi` import except `Spawn`); ~0 changed lines, ~30 lines of glue. Basis: file is 160 lines and imports only `node:child_process` and `cpd.ts` (**read** `:1-2`) |
| Deliver `session-message`, `pr-feedback`, `escalation`… with steer/follow-up | `pi/lib/delivery.ts:9-17,20-44`; sent with `pi.sendMessage(…, {deliverAs, triggerTurn})` (`pi/lib/pi.ts:23-30`) | `sendMessage`, `sendUserMessage` `pi:packages/coding-agent/src/core/extensions/types.ts:443,448`; `isIdle()` `:348`, `hasPendingMessages()` `:356` | `submit({ type: "input", whenBusy: "steer" })`; a follow-up is the default queued input (README `:506-517`); `afterFailedRun` setting (`:220`); a write with only `kind`+`data` appends at once even mid-run (README `:517`) | The mapping exists: `steer` ↔ `whenBusy: "steer"`; `followUp` ↔ default queue; `customType: "cpd"` ↔ an entry kind (write with `kind`/`data`). The renderer is a cockpit view. `triggerTurn:false` ↔ write without `model` | Port `delivery.ts`/`event.ts`: ~150 lines TS. Basis: both files, 44 + 98 lines (**ran**) |
| Idle / blocked reporting (`session state`) | `pi/lib/agent-state.ts:54-90`, `state.ts` 164 lines; pi events `agent_start`, `agent_settled` (`ctx.isIdle()`), `ui_prompt_start/end` | the events (1.3.0 and 1.4.0 both have them, §1) | `watchEvents(harness, conv, ctx)` gives coding-agent style events from commits (README `:590-604`); `docs["pi.live"]` shows running generation/tools; `pi.inbox` queued inputs (README `:98`) | The state is derivable from documents instead of hooks, and **better**: crash-proof. A host-side state machine replaces `agent_start`/`agent_settled` | Port `agent-state.ts` + `state.ts`: ~200 lines TS. Basis: 92 + 164 lines |
| Nudge / turn count / no-submit | `pi/lib/turns.ts:58-100` (assistant turns at nudge time, A5) | `before_agent_start`/`agent_end` | count of `pi.assistant` entries in storage, or `pi.usage` | read from storage instead of the file | see §5 |
| First message, reminders, policy and time-zone prompt text | `first-message.ts` 40, `reminder.ts` 83, `policy.ts` 25, `time.ts` 56; `before_agent_start` handlers `agent-state.ts:24-47` | `before_agent_start` can alter the system prompt/messages | `defineExtension({ sections: [section(…)] })` prompt sections (README `:26-60` example `26-coding-agent.ts`); hooks (`GenerationTask`) | rebuild as sections/hooks; the content is cpd's | Port ~250 lines TS. Basis: the four files, 204 lines |
| Session rename forwarded to cpd | `agent-state.ts:84-90` on `session_info_changed` | event + `setSessionName` | conversation has no title concept I found (**unverified**: `grep -rn 'title' durable/src/session` not run) | host must own the title; a cockpit rename is already cpd's | Drop the hook; the cockpit renames through `cpd session rename` |
| Loop guard | `pi/lib/loop-guard.ts:48-60` | `before_agent_start` | hook on `GenerationTask` | port | ~60 lines TS |
| Reload poll | `pi/lib/reload.ts` 300 lines + command | `ctx.reload()` | n/a | **Delete** | see §3.1 |

Everything else in `pi/lib` that is *policy over cpd commands* (`cpd.ts`, `event.ts`, `cursor.ts`, `protocol.ts`, `handle.ts`, `host.ts`, `run.ts`) does not import pi at runtime (types only, `pi/lib/pi.ts:1-3`: "Types only: nothing of pi is bundled or run") — **read**. It survives as is.

## 5. (j, k) What cpd reads from pi's session file, and what changes on pi-durable

| Reader | File:line | What it takes from the pi file | On pi-durable |
|---|---|---|---|
| Find the file | `core/src/host/pi/session_file.rs:12` (`<agent dir>/sessions/*/*_<id>.jsonl`; `$PI_CODING_AGENT_SESSION_DIR`) | path | replace by a storage path per Session (e.g. `$CPD_HOME/run/<plan>/sessions/<id>.sqlite`): chosen by cpd, so **no search needed** (simplification; 66 lines go) |
| A5 `judge` | `core/src/driver/run.rs:121-135` | `state` from the Session row + last assistant `stopReason` (`stop`, `toolUse`, `error`, other) + `errorMessage` + `turns` | `pi.assistant` entry `model[0].stopReason`, `errorMessage` (same `AssistantMessage`, `pi:packages/ai/src/types.ts:578-600`; **StopReason** `"pending" \| "stop" \| "length" \| "toolUse" \| "error" \| "aborted" \| "deferred"` at `:482`). The values `pending`, `deferred`, `length`, `aborted` are **new** against cpd's `judge`, which maps any other value to `Crashed` (`:133`): each needs a rule. Provider-error retry is built in (`settings.retry`, README `:420`; `afterFailedRun`, `:220`), which changes what `provider_error` means |
| Metrics (A9) | `core/src/driver/worker.rs:454-472` (`metrics::parse_file`, `last_error_message`); engine `engine/src/watch/engine.rs:321-322`; cost design `docs/research/session-cost.md` | per-message `usage` in the JSONL | **better**: `pi.usage` document holds totals, keyed by model, and `Harness.usage(context)` sums across conversations (`pi:packages/durable/src/harness/usage.ts:7-58`; `harness.ts:355`). The A9 metrics would be a read of one document: fewer lines than the parser |
| "Source idle" for forks | `core/src/session/fork_wait.rs:79-88` | last `message` entry | `pi.live` document / no running task |
| Worker reading by the engine (liveness) | `engine/src/watch/liveness.rs`, `engine.rs:321` | the file's mtime and stop reason | storage `mtime` is unreliable (WAL; **unverified**); use the `pi.live` document or a heartbeat from the host |
| History view | `tui/src/history/render.rs:1-60` (544 lines in `tui/src/history/*.rs`, **ran**) | JSON lines: `type == "message"`, `role`, `content[].type` (`text`, `tool_use`…) | see §6 |
| Facts about the session in `session state` | `pi/lib/state.ts` | none from the file | unchanged |

**ran** `grep -n 'pi::session_file\|metrics::parse_file\|metrics::last_error' -r core/src engine/src tui/src cli/src` (non-test): hits at `core/src/driver/run.rs:539`, `core/src/driver/worker.rs:308,454,455,467,472`, `core/src/session/run.rs:373,379`, `engine/src/watch/engine.rs:321,322`: **9 call sites** need to move to a "SessionStore" port. This is the whole Rust surface of "what reads pi's file" apart from the History view. A5/A9 in `docs/contract.md:938,942` need a rewording, not a redesign: the contract says "parsed from the Worker's session file"; the new rule says "read from the Session's store".

## 6. (k) Session storage and history

| Aspect | pi JSONL file | pi-durable | Gap | Close it |
|---|---|---|---|---|
| Format | JSONL, one entry per line, tree via `id`/`parentId` (not re-read in full here: `docs/research/session-cost.md` §1 documents it) | SQLite tables `conversations`, `entries`, `tasks`, `submissions`, `documents`, `document_revisions` with `record TEXT CHECK (json_valid(record))` (**read** `pi:packages/durable/src/storage/sqlite/migrations.ts:15-92`); or JSONL "append-only files in one directory" (README `:766`) | different shape; no pi-compatible import | none to fix: a new reader |
| Read from Rust | `serde_json` over lines (`tui/src/history/render.rs:23-30`) | `rusqlite` is already a workspace dependency (`Cargo.toml:23`, **ran** `grep -n rusqlite Cargo.toml` → `rusqlite = { version = "0.40", features = ["bundled", …] }`), so a read-only reader costs no new crate. The entry `record` is JSON: `pi.assistant` `model[0]` is an `AssistantMessage` whose `content` is `(TextContent \| ThinkingContent \| ToolCall)[]` (`pi:packages/ai/src/types.ts:578-580`), the same shape the renderer walks | A reader that follows the *head* path (entries have `head`, a `commit_seq`, and forks share ancestors): **unverified** how a branch is walked in SQL; I did not read `storage.ts` beyond the schema | Build ~200 lines Rust (basis: `tui/src/history/render.rs` + its siblings are 544 lines, **ran**; the new reader replaces the line parser, keeps the rendering) |
| Concurrent read while the host writes | the file is appended; a partial last line is skipped (`render.rs` doc comment) | SQLite WAL mode with `synchronous = NORMAL` (README `:766`) allows a reader in another process; durable says "one process owns a storage" for *durable* use (README `:790`); a plain SQL `SELECT` is not a durable owner, but **unverified**: I ran nothing | verify with a mock-free read of a stored file in the Job that builds it | |
| Live view | History re-reads the file at the tick | `watchEvents` and `watchDoc` stream commits in-process (README `:590-604`) | across processes: poll `commit_seq` (`entries.commit_seq`) from Rust | ~80 lines Rust |
| Fork source file for review Jobs | file path | same storage owner | §3.3 | |
| Backup/inspect by a human | `cat`/`jq` of a file | `sqlite3` | acceptable | |
| Unification | session file per Session; cpd keeps its path in the row (`row.file`, `core/src/session/run.rs:373`) | one storage per Session or one per Plan? Storage is a lock-owned single database (README `:790`); one per Session keeps cpd's rows 1:1 | decision for synthesis | |

The one-file-per-Session scheme also keeps `cpd session archive` and `delete` (`core/src/session/archive.rs`) trivial. **Unverified** (not tested).

## 7. (l) Hosting a Manager or Lead on pi-durable

### 7.1 How it is launched today

- **read** `core/src/host/pi/launch.rs`: `Launch` is "what tmux runs for this launch in `cwd` (`new-window -c <cwd> -e K=V … -- pi …`)" (`:25`); `lead()` at `:117` builds `pi [--model m --session file | --fork src] --session-id <id> -n <name> -- @<message file>`; env `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `PLANNOTATOR_PORT`, `PLANNOTATOR_URL_HOST`, `PLANNOTATOR_DATA_DIR` (`:145-156`); `PATH` starts with `$CPD_HOME/bin` (the `git` wrapper, `:101-103`). The Manager has no `CPD_PLAN` (`:66`). The Worker adds `CPD_ATTEMPT` (`:185`).
- **ran** `git grep -l -i tmux -- core/src engine/src tui/src cli/src` (non-test): **89 files** name tmux; `git grep -n 'tmux_pane\|chat_slot'` (same filter) gives **103 lines**; `tui/src/*.rs` has 2 lines naming `tmux_pane`.
- Panes: `core/src/host/tmux.rs` (47 public functions, **ran** `grep -c 'pub fn\|pub(crate) fn'`); chat-slot moves in `core/src/session/moves.rs` (572 lines).

### 7.2 Process model

| Option | Fit | Cost / risk |
|---|---|---|
| **One Node host per Session** (a small `cpd-host` script that opens a `Harness` + SQLite file for one conversation) | Keeps cpd's lifecycle 1:1: `session run` starts one process, the Session row's `state` is that process, `pi` crash == process exit, `kill-window` becomes `kill <pid>`; the storage's single-owner rule is satisfied for free (README `:790`); `@earendil-works/pi-durable` loads TypeBox ≈ 23 MB peak RSS unbundled, ~4 MB bundled (README, last paragraph of Storage, **read**) | Node start per Session (not measured; **unverified**); the engine's liveness (A5, `engine/src/watch/liveness.rs`) must find a process, not a pane |
| One Node host per machine (many conversations in one Harness) | Cheaper memory; durable is built for many conversations (`harness.root`, `createConversation`) and a crashing tool task would be `faulted`, not kill others (spec `:1750`) | A crash or a stuck tool call takes down every Session; cpd's per-Session stop/kill semantics, `CPD_SESSION` env, and the `$CPD_HOME/bin` PATH per Session (the PATH wrapper is per *process*; one host would have to set it per `bash` call through the `ExecutionEnv`, `NodeExecutionEnv({ cwd })`, `pi:packages/durable/test/examples/26-coding-agent.ts:46` — feasible but **unverified**) are all rebuilt |

Recommendation for synthesis (not a design): one host per Session for Managers and Leads; it is the only choice that deletes no cpd invariant.

### 7.3 What the host needs from cpd to start

- Env: `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN` (absent for a Manager), `CPD_ATTEMPT` (Workers), `PATH` with `$CPD_HOME/bin` first (`launch.rs:101-111`). A host started as a normal child process takes the same env block unchanged. **read**.
- A storage path and the conversation id (new): chosen by cpd; the host calls `harness.root()` on first start, `conversation(id)` after.
- The first message file: today `-- @<file>` (a pi CLI feature). A host reads the file and `submit`s it; ~15 lines.
- The model: `--model p/m:tier`; `configure({ model: {provider, modelId}, thinkingLevel })`; ~20 lines. The tier names (`:medium`) are cpd's; the resolution at `launch.rs:123` stays.
- The extension set: `lead_pi_packages` and `-e <adapter>` become a list of durable extensions (`defineExtension`), registered with `createRegistry().install(...)`, as in the example (`26-coding-agent.ts:19-37`).
- Credentials: pi-ai `CredentialStore` (`pi:packages/ai/src/auth/credential-store.ts`) with auth.json compatible store: pi's file store is `coding-agent/src/core/auth-storage.ts` (**read**, 507 lines, `proper-lockfile`). **Unverified** whether pi-ai ships a reusable file-backed store without coding-agent; I did not check.
- Trust/project settings that pi derives (`isProjectTrusted`, `ctx.cwd`): the host sets `cwd` explicitly (`ExecutionEnv`).

### 7.4 Restart and crash behaviour

| Today | With the host |
|---|---|
| pane dead → driver A5 `crashed` (`contract.md:938`); `cpd session resume` restarts pi with `--session <file>` (hs-d506 model first) | host exit → same `crashed`; `resume` restarts the host, which reopens storage; **unfinished work resumes by itself** (README `:127`: "Work interrupted by a crash or close stays pending. `resume()` starts the task scheduler"); a tool without `replay: "safe"` returns `interrupted` (README `:277`). This is **better** than pi, which loses the in-flight tool call |
| A5 stop-reason decision read from file | read from storage (§5) |
| tmux `kill-window` ×3 in A9 | `kill` the host's pid, `Harness.close(context)` first for a clean stop (spec `:1045`) |
| the reload poll (`pi/lib/reload.ts`) | none |
| the `driver` lock/liveness | unchanged, if the Session row has a `pid` instead of `tmux_pane` (a schema migration; `0001-initial.sql` and `0010-manager.sql` name tmux, **ran** the grep list in §7.1) |

### 7.5 How `cpd tui` finds and focuses a chat

Today the cockpit knows the pane id of each Session and moves panes in and out of the chat slot with tmux commands (`core/src/session/moves.rs`; 2 `tmux_pane` lines in `tui/src/`; `core/src/host/layout.rs`; `cockpit.rs`). **read** the moves; the `tui/src/cockpit.rs` details are **unverified** (I read the grep, not the file).

With a native chat, the cockpit needs instead:
1. a *Session id → store path* lookup (a column or a path rule; replaces `tmux_pane`);
2. a *Session id → host endpoint* to send input and answers: a Unix socket in `$CPD_HOME/run/<plan>/` or the existing Event stream (`cpd session send`, `core/src/session/send.rs`, 195 lines). The single-owner rule (README `:790`) means the cockpit **cannot** write the storage itself;
3. a chat view state (`which Session is shown`) held in `cpd-tui`, with no process move.

Net: `chat_slot` state, `in_chat_slot` refusals (`docs/commands.md:465,667,1142,1189`) and layout code are removed or rewritten; this is the biggest Rust change in this half.

## 8. (m) Everything else the adapter provides, and the sizes

### 8.1 Items the table lacked

| Feature | Where | pi / durable | Fate |
|---|---|---|---|
| `@mentions` | `pi/lib/mentions.ts` 67, `mentions-refresh.ts` 31 | pi editor autocomplete is the pi TUI's | cockpit input widget; the data comes from `cpd` |
| Slack helpers | `pi/lib/slack-channel.ts`, `slack-parse.ts`, `slack-post.ts`, `slack-react.ts` | tools over `cpd`/Slack CLI (not read) | runner-neutral if they only shell out (**unverified**: I did not read them) |
| `review-chat.ts` | `pi/lib/review-chat.ts` 71 | uses `ctx.ui` | rebuilt with the blocking tool (§2) |
| `updates.ts` | `pi/lib/updates.ts` 323 | not read | **unverified**: I did not open it; sized only |
| `jira-transition.ts`, `models-list.ts`, `shell.ts`, `reply.ts` | `pi/lib/` | not read | unverified |
| Themes, pills, balloons, `pad.ts`, `render.ts`, `hint.ts`, `italic.ts`, `phi.ts`, `compaction-ratio.ts` | `pi/lib/*` (render 237, pad 82, hint 85, pill 53) | pi TUI components (`Component` from `@earendil-works/pi-tui`, `pi/lib/pi.ts:3`) | delete (the ratatui chat draws these itself) |
| `mainguard.ts`, `noblock.ts`, `testguard.ts`, `secretguard.ts` | 253, 84, 85, 23 lines | `tool_call` guards over `bash` and tools | tool guards belong to parity-core; port as `beforeTool` hooks (README `:235`) |
| packages (`core/src/host/pi/packages.rs`, 369 lines) | resolves which npm/git pi packages are installed and skips duplicates; pi identity rules | none | delete with pi packages; keep the `[-e …]` rule only if plannotator stays |

### 8.2 Sizes and bases (every number is from the command shown)

- **ran** `timeout 60 bash -c 'cat $(ls pi/lib/*.ts | grep -v test) pi/extension.ts pi/mcp-extension.ts | wc -l'` → **5484** lines of non-test adapter TypeScript.
- **ran** same pattern over `git ls-files pi/ask-user-question | grep '\.ts$' | grep -v test` → **5892**; over `pi/compact-tools` → **7705**. These two are vendored forks; the compact-tools one is deleted whole, the ask-user-question UI becomes ratatui.
- **ran** `wc -l` of the adapter files named above: follower 160, delivery 44, handle 173, startup 268, state 164, agent-state 92, turns 100, reminder 83, first-message 40, updates 323, run 94, host 117, cpd 87, event 98, cursor 16, protocol 22, retier 73, loop-guard 60, mentions 67, review 267, review-chat 71, plan-review 77, plan-review-command 36, plannotator 78, pntrack 157, pnguard 40, askguard 104, noblock 84, mainguard 253, secretguard 23, testguard 85, hostapprove 60, secret 58, reload 300, reload-command 30, policy 25, time 56, render 237, pad 82, hint 85, pill 53, mcp 18.
- **ran** `wc -l` Rust: `core/src/host/pi/{dirs 172, launch 548, mod 26, packages 369, session_file 66}`, `core/src/session/{fork_wait 109, run 540, moves 572, send 195, history 49}`, `tui/src/history/*.rs` 544.
- **ran** pi-durable examples: 11-extension-state 87, 12-tasks 64, 20-inbox 72, 22-subagent-foreground 136, 23-subagent-background 364, 24-child-tasks 146, 27-plan-mode 116, 26-coding-agent (head read), 31-reload-and-restart 90, 13-recovery 91.

### 8.3 Lists

**Does not need replacing (runner-neutral cpd logic).** *Read* for each: cpd's Event stream and cursor (`event.ts` 98, `cursor.ts` 16, `protocol.ts` 22), the follower (`follower.ts` 160: Node child process only), `cpd.ts` (87), `host.ts`, `handle.ts`, `run.ts`; the cpd call surface `session send/state/rename`; the driver's A5 ladder (`judge`'s *decision table*, `driver/run.rs:121-135`: only its inputs change), nudge-once rule; `session fork` registration and ladder; `cockpit` rows; the `$CPD_HOME/bin` PATH wrapper; the env contract (`CPD_*`); Slack and Jira helpers if they only shell out (**unverified**).

**Must be rebuilt for pi-durable.** (1) the host process (open Harness, root conversation, submit first message, model/tier, extension set): ~250 TS; (2) blocking tools with a `Pending` document and resolver: ~250 TS (§2.5); (3) hooks → `beforeTool` guards (`askguard`, `pnguard`, `mainguard`, `noblock`, `testguard`, `secretguard`): ~550 TS (basis: files 104+40+253+84+85+23 = 589 lines); (4) delivery of Events as inputs with steer/follow-up: ~150 TS; (5) state reporting from `pi.live`/`pi.inbox`: ~200 TS; (6) prompt sections (first message, reminder, policy, time, loop guard): ~320 TS (basis 40+83+25+56+60 = 264 lines plus margin for sections); (7) retier: ~70 TS; (8) Rust "SessionStore" port for the 9 call sites (§5): ~250 Rust; (9) Rust history reader over SQLite: ~200 Rust; (10) host launcher (`launch.rs` host variant) and Session row `pid`/store path (migration): ~300 Rust + 1 migration; (11) fork-wait and fork command over storage: ~120 Rust + ~60 TS; (12) cockpit chat endpoint (socket or Events) and the removal/rewrite of `chat_slot`/`tmux_pane` (103 lines, 89 files name tmux): **1–1.5 k Rust** (most are small edits, some are rewrites of moves.rs 572 lines); (13) contract/docs rewrite of A5/A9 and §6 launch (docs, not counted).

**Can be taken as a dependency (convention over configuration).** `@earendil-works/pi-durable` itself (`Harness`, SQLite storage, `CodingTools`, extensions, hooks, `watchEvents`, usage documents, `fork`, compaction); `@earendil-works/pi-ai` (models, auth, oauth: `pi:packages/ai/src/auth/*`); `@earendil-works/pi-mcp` (`pi:packages/mcp`, "Standalone Model Context Protocol client", **read** its `package.json` description); `rusqlite` (already in `Cargo.toml:23`) for the cockpit's reader; plannotator as a subprocess if its web page can be driven without pi (**unverified**). Not takeable: a ratatui chat (no crate in the workspace list; the widgets/crates Jobs decide).

**Size estimate.**

| Part | Lines | PRs | Basis |
|---|---|---|---|
| TypeScript host + extension (items 1–7 above) | **2.5–3.5 k** | 5–6 | the adapter is 5484 lines today (**ran**); roughly 60 % is policy/guards/prompt/deliver (rebuilt), 25 % is pi-UI rendering (deleted: render 237 + pad 82 + hint 85 + pill 53 + reload 300 + compact-tools 7705 forks), 15 % is runner-neutral (kept: follower 160, cpd 87, event 98…). The 60 % figure is **unverified**, a sum of the files in §8.2 |
| Rust port, history reader, launcher (items 8–11) | **~0.9 k** | 3 | basis: replaced files `session_file.rs` 66 + `fork_wait.rs` 109 + `history` 544 + `launch.rs` 548 (host variant) |
| Rust cockpit chat endpoint and removal of tmux-pane assumptions (item 12) | **1–1.5 k** | 3–4 | basis: 103 `tmux_pane`/`chat_slot` lines in 89 files (**ran** §7.1), `moves.rs` 572 lines |
| **Total (this half)** | **~4.5–6 k lines; 9–12 PRs** | | Excludes the ratatui chat widgets and the questionnaire UI (other Jobs) |

Deleted at the end (subtraction, not additions): `pi/compact-tools` (7705 lines), `pi/ask-user-question` TUI (5892; the logic moves), `pi/lib/reload.ts` + command (330), `render/pad/hint/pill/italic/balloon/phi` (≈ 520), `core/src/host/pi/packages.rs` (369) — **ran** sums above.

## 9. Not verified, and how to verify it

| Open point | Why unverified | How to verify |
|---|---|---|
| A human-wait tool works with `replay: "safe"` and a `Pending` document | no example; design from README/spec only | write a throw-away durable script with the faux provider (as `26-coding-agent.ts`), kill the process mid-wait, reopen |
| A second process can read the SQLite storage while the host writes | README forbids two owners, a read-only `SELECT` is unmeasured | `sqlite3 file 'select max(commit_seq) from entries'` while a host runs |
| Branch walk in SQL (head path) | I read only the schema | read `pi:packages/durable/src/storage/sqlite/storage.ts` (957 lines), `scanEntries` |
| Node host start time | not measured | `time node host.js` against a prebuilt tree-shaken bundle |
| plannotator outside pi | package not read | read `@plannotator/pi-extension` (`npm pack`) |
| `updates.ts`, Slack and Jira helpers | not opened | read them: they are runner-neutral if they only call `cpd`/CLIs |
| Time pi 1.3.0 vs 1.4.0 | only two checks (§1) | `diff` of the two npm packages' `dist/core/extensions/types.d.ts` |

Cleanup: **ran** `rm -rf /tmp/pra1` (the clone) and `ls /tmp/pra1` → no such directory; nothing was unpacked with `npm pack`; `git status --short` shows only this file.
