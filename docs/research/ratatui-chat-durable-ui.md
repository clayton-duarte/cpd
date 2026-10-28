# ratatui chat: pi-durable's API for an external UI (evidence file `durable-ui`)

Part of the Plan `ratatui-chat-research` (Question 1); the synthesis `docs/research/ratatui-chat.md` reads this file. Written
2026-10-28 (EDT, finished about 15:10 EDT) by Attempt `durable-ui-a1`. Nothing here decides the architecture: facts, options, sizes.
Prior docs are not restated; `docs/research/pi-durable.md` is corrected where the source disagrees (last section before the table).

Label of every claim: **read** (file and line opened), **ran** (command run, output quoted) or **unverified** (inferred, with why).
Source paths are relative to a clone of `earendil-works/pi` unless they start with `origin/main:` (the cpd repo).

## Versions read

| What | Value | Evidence |
|---|---|---|
| npm latest | `@earendil-works/pi-durable@1.4.0` | **ran** `npm view @earendil-works/pi-durable version dist-tags` on 2026-10-28 EDT: `version = '1.4.0'`, `dist-tags = { latest: '1.4.0' }` |
| tag `v1.4.0` | commit `4a080252f`, 2026-10-21 18:29 EDT | **ran** `git log -1 --date=format-local:'%F %H:%M %Z' v1.4.0^{commit}` with `TZ=America/Toronto` |
| `main` | `33385f9572303e79bfb3811cc92cc8dd65e22261`, 2026-10-28 14:53 EDT | **ran** `git rev-parse HEAD` in a fresh `git clone https://github.com/earendil-works/pi` |
| `main` vs 1.4.0 in `packages/durable` | 157 files changed, 17439 insertions, 4397 deletions | **ran** `git diff --stat v1.4.0 HEAD -- packages/durable \| tail -1` |
| cpd | `origin/main` `d943c49c` | **ran** `git rev-parse --short origin/main` |

Two APIs matter and they differ: **1.4.0 (npm)** and **`main` (Unreleased, `CHANGELOG.md` "Unreleased")**. Every answer says which.
`main` has no npm release yet (**ran** the `npm view` above: latest is 1.4.0). A cpd build today can only take 1.4.0
(`npm pack @earendil-works/pi-durable@1.4.0`, unpacked: no `readOnly` in `dist/harness/types.d.ts`, **ran** `grep -rl readOnly pk/package/dist/harness/types.d.ts | wc -l` printed `0`; `watchEvents` present in `dist/index.d.ts`, **ran** `grep -c watchEvents` printed `1`).

## 1. Client API

**Answer.** pi-durable has no client/server API of its own: a "client" is code in the process that owns the `Harness`, calling
`Conversation` methods. Submit, abort and watch exist and are enough to drive a chat UI; the view is typed state (messages, streamed
partial answer including thinking and tool-call arguments, tool slots with running output, queue, run failure, usage). 1.4.0 and `main`
differ in the watch surface: `watchEvents()` and `viewState()` exist only in 1.4.0; `main` removed both.

### Submit (**read** `packages/durable/src/harness/types.ts:73-111`, `:794-` `Conversation.submit`)

```ts
type SubmissionDraft = { requestId?: string } & (
  | { type: "input"; content: UserInput;                 // string | content blocks
      whenBusy?: "steer" | "followUp" | "reject";        // default followUp
      whenIdle?: "reject";                               // main only
      entry?: { kind: string; data?: JsonValue } }       // render as an app entry, model still sees content
  | { type: "write"; entry: EntryDraft }                 // append an entry without asking the model
  | { type: "continue" } );                              // main only; run on the context as it is
conversation.submit(draft, context): Promise<Submission>
Submission { id; status(ctx); wait(ctx): Promise<{status: "done" | "unanswered", reason?}>; abort(ctx): "aborted" | "already_placed" | "settled" }
```

- **read** `README.md:531-575` (main): while a run is going, `whenBusy` default queues a follow-up in `docs["pi.inbox"]`;
  `"steer"` is placed after the current tool round; `"reject"` throws `ConversationBusy` and writes nothing. A retried `requestId`
  returns the existing submission (`README.md:129-132`). `submission.abort()` withdraws a queued item only.
- **read** `harness/types.ts:869-871` / `README.md:117-125`: `resume()` starts the scheduler (`harness.resume()`); submit and wait start it too.
  After a restart, `harness.submission(id)` reacquires a submission (`README.md:138`).
- **read** the cpd adapter already uses the same two delivery modes: `origin/main:pi/lib/delivery.ts:11-18` maps `pr-feedback` and
  `escalation` to `deliverAs: "steer"`, `job-merged`, `job-failed`, `plan-done`, `error` to `"followUp"`. These map one to one to `whenBusy`.

### Abort, resume, read-only

- **Abort:** `conversation.abort(context, { background? }): Promise<void>` (**read** `harness/types.ts:~830`, `README.md:714`): withdraws queued
  inputs, aborts every task of the current work, resolves once idle. `harness.abortTask(id, ctx): "marked" | "terminal"` (**read** `types.ts:907`). Details in section 3.
- **Resume after restart:** `Harness.open(storage, {models, registry}, context)` then `harness.root(context)` (same root) then `harness.resume()` (**read** `README.md:115-130`).
  A run that ended without an answer is continued with `submit({ type: "continue" })` (**read** `README.md:565-580`, main).
- **`readOnly: true`** (main only; **read** `harness/types.ts:728-735`, commit `be14d9732`, 2026-10-22, not in the 1.4.0 tag:
  **ran** `git merge-base --is-ancestor be14d9732 v1.4.0` printed `readonly-after-1.4.0`): open loads running tasks as committed and reconciles nothing;
  writes, `resume()` and every task-running call reject with `ReadOnlySession`; reads and watches work. The spec says what it is for:
  "a viewer of a session another process drives, such as a synced copy on a device that only shows it" (**read** `docs/spec.md:833-842`).
  It does **not** make a second process see the first one's commits live; see section 2.

### Watch surfaces

| Surface | 1.4.0 | main |
|---|---|---|
| `conversation.viewState(ctx)` (Chord read-only state, `subscribe(value)`) | yes, **read** `v1.4.0:packages/durable/README.md:476-486` | removed, **read** `CHANGELOG.md` "Removed the Chord state getters `Session.documentState()`, `Conversation.viewState()`, `Harness.taskGraph()`" |
| `conversation.watch(ctx)`, `start(cb)` | `cb(value, ops)` positional, 100 undelivered frames then one full-view frame (**read** `v1.4.0 README` lines 488-499) | `cb({ kind: "value", value, ops, retired }, ctx)`, commits that land during a callback are folded, more than 10,000 pending ops become `[["r", view]]` (**read** `README.md:477-527`) |
| `watchEvents(harness, conversationId, ctx)` (free function; coding-agent style events) | yes, "Experimental", **read** `v1.4.0 README` section "Agent Events (Experimental)" and `v1.4.0:packages/durable/src/harness/events.ts` (506 lines, **ran** `git show ... \| wc -l`) | **removed** with spec §9.4; "everything the events carried ... is in the conversation view" (**read** `CHANGELOG.md` Unreleased, "Removed `watchEvents()`") |
| `harness.observeConversation/observeDoc/observeTaskGraph` (synchronous observers), `watchDoc`, `watchTaskGraph` | task graph and doc watches yes | yes (**read** `README.md:523`) |
| `ConversationTranscript` (merge, `loadEarlier`, `fillGap`, `resume`) | no | yes (**read** `README.md:497-523`, `src/harness/transcript.ts` 285 lines) |
| `view.entries` | the active transcript (whole) | a window: newest 50, up to 100, then splice to 50; `watch(ctx, { min, max, from })` (**read** `CHANGELOG.md` "`ConversationView.entries` is a window") |

### What is in a view (**read** `src/harness/view.ts:42-58`, `src/harness/live.ts:159-200`, main)

`ConversationView = { conversation, entries: EntryRecord[], before?, docs: { "pi.live", "pi.inbox", "pi.usage", "pi.agent", "pi.provider" } }`.

| Wanted | Where | Shape |
|---|---|---|
| Messages | `entries` (kind `pi.user`, `pi.assistant`, `pi.tool-result`, `pi.system`, `pi.reset`, app kinds) | `EntryRecord { id, conversationId, kind, model?: Message[], data?, head?, edits?, byTaskId? }` (**read** `src/types.ts:458-472`) |
| Streamed answer incl. thinking | `docs["pi.live"].generation.message` | the whole partial `AssistantMessage` (content blocks `text`, `thinking`, `toolCall` with partial arguments), committed at most every 100 ms (**read** `harness/generation.ts:663-725`, `harness/agent.ts:35`: `partialIntervalMs: 100`) |
| Retry, deferred | `generation.retry { at, error }`, `generation.deferred { pollAt }` | **read** `live.ts:166-172` |
| Tool calls with partial output | `docs["pi.live"].tools: ToolSlot[]` | `{ callId, name, status: "pending" \| "running" \| "done", output?, droppedBytes?, droppedLines?, details?, diagnostics?, taskId?, entry? }`, output committed at most every 100 ms (**read** `live.ts:14-45`, `agent.ts:36`) |
| Nested tool calls, subagent link | `nestedTools[]`; subagent `details: { conversationId }` | **read** `live.ts:47-70`, `README.md:719-745` |
| Errors | `docs["pi.live"].runFailure { reason, detail, inputs, taskId }` with reasons `model_error`, `no_model`, `aborted`, `reset`, `faulted`, `orphaned`, `interrupted`, `restart_limit`, `nothing_to_answer`, `turn_limit`; `compactionFailure`; tool error results are entries | **read** `live.ts:79-130` |
| Busy | `docs["pi.live"].run` present exactly while busy | **read** `live.ts:159-163`; since main `pi.live` is not `{}` after a failed run, test `run === undefined` (**read** `CHANGELOG.md`) |
| Queue | `docs["pi.inbox"].items: { id, mode: "steer" \| "followUp" \| "write", content }` | **read** `src/harness/inbox.ts:8-21` |
| Usage | `docs["pi.usage"]`: tokens and cost per `provider/model` and per tool | **read** `README.md:844` |
| Agent choice | `docs["pi.agent"]`: model, thinking level, extensions, tools, cwd | **read** `README.md:92-114`, `harness/agent.ts` |
| Compaction | `docs["pi.live"].compactions[]` | **read** `live.ts:72-83` |

What a Rust cockpit needs of this: a serde model of `ConversationView` (entries and five documents). The old TUI at `7472cda0^` did exactly that with a
plain boundary: `DurableView { session, conversation: ConversationView, conversations, models, notices, tasks? }` and `DurableController
{ submit(text, whenBusy), compact, abort, cycleThinking, setModel, toggleTasks, switchConversation }` (**read**
`git show 7472cda0^:packages/coding-agent/src/experimental/durable/runtime.ts` lines 40-95: "Everything the TUI renders. Plain values; no Harness objects cross this boundary").
That interface is the one a JSON IPC would carry.

### The deleted experimental UI stack (counts at `7472cda0^`)

Commit `7472cda0d6e51dfbf53c03f46b47aa42ac64bf64`, 2026-10-12 06:19 EDT, Mario Zechner: "feat(coding-agent): remove the experimental server stack and the client, protocol, and server packages" (**ran** `git show 7472cda0 --stat --format=...`: "161 files changed, 32 insertions(+), 21561 deletions(-)").
Its message gives no reason beyond the list ("Deletes packages/client, packages/protocol (CBOR codec), packages/server, coding-agent's src/experimental (except source-resolver.ts ...), src/cli/experimental, src/client, the example plugin, and their tests. coding-agent no longer depends on @earendil-works/chord."). **Why** it was removed is therefore **unverified**; the files carry no statement. Facts around it: the commit before it, `2ee5c1e56` (2026-10-12) "docs(durable): drop the Chord facet usage guide and its test"; the stack was "experimental" in every README (**read**).

Counting command (per directory, `P=7472cda0^`): `git ls-tree -r $P --name-only -- <dir> | while read f; do git show "$P:$f" | wc -l; done | awk '{s+=$1;n++} END{print n" files, "s" lines"}'`; non-test = paths without `test`. All **ran**:

| Directory at `7472cda0^` | Files | Lines, all | Non-test files / lines | Test files / lines |
|---|---|---|---|---|
| `packages/client` | 19 | 2169 | 12 / 1327 | 7 / 842 |
| `packages/protocol` (CBOR codec, framing) | 17 | 1609 | 12 / 1021 | 5 / 588 |
| `packages/server` | 29 | 3278 | 16 / 1754 | 13 / 1524 |
| `packages/coding-agent/src/experimental/durable` (TUI coding agent on pi-durable) | 8 | 1427 | 8 / 1427 (`tui.ts` 660, `runtime.ts` 373, `harness-setup.ts` 123, `prompt.ts` 74, `README.md` 66, `sessions.ts` 56, `subagent.ts` 55, `main.ts` 20) | 0 |
| `packages/coding-agent/src/experimental/vacation` (vacation planner TUI) | 7 | 1389 | 7 / 1389 (`tui.ts` 660, `runtime.ts` 369, `vacation.ts` 142) | 0 |
| rest of `src/experimental` (coordinator, session workers, relay, services, plugins) + `src/cli/experimental` + `src/client` | | | see below | |

The "rest" (**ran** `git ls-tree -r $P --name-only -- packages/coding-agent/src/experimental packages/coding-agent/src/client packages/coding-agent/src/cli/experimental`, then `wc -l` each): `session-worker-manager.ts` 848, `session-worker.ts` 840, `client-tui.ts` 809, `server.ts` 770, `radius-relay.ts` 724, `coordinator.ts` 612, `services/connection.ts` 399, plus about 20 smaller files. Total of the client + protocol + server + experimental `durable` TUI: 2169 + 1609 + 3278 + 1427 = **8483 lines** (with tests), **5529** without tests (1327 + 1021 + 1754 + 1427).

What they exposed (**read** the READMEs at `7472cda0^`):
- `pi-protocol`: protocol version 8, length-prefixed CBOR frames (4-byte big-endian length + one definite-length CBOR item), a hello handshake with a logical `serverId`, request/response with cancellation, subscription updates, out-of-band `attachment` messages; payloads are opaque strict JSON, whose grammar is **Chord's** service calls (`{ serviceId, instance?, member, args }`).
- `pi-server`: a local server that routes clients to application-hosted durable Sessions, one **worker process per Session** (`session-worker-manager.ts`), multiple presentation attachments per Session, Unix-socket preset (`createUnixServer`); "Neither an open JavaScript Session nor a Harness crosses the process boundary."
- `pi-client`: a transport-neutral client, `Client.connect({ serverId, transportFactory })`; no automatic reconnect or replay: "call `reconnect()`, attach through the application's management service again, and explicitly repeat only operations known to be safe."
- The two TUIs ran **in the process that owns the Harness**: "One process owns the model runtime, the durable Harness, its SQLite storage, and the TUI", with a `proper-lockfile` lock on the session directory "a lock keeps a second process out" (**read** `experimental/durable/README.md`, `sessions.ts:20-56`). They rendered `Conversation.viewState()`.
- Both the TUIs and the stack were built on the **`pi-tui` and Chord facets** of that time; the server stack hosted pi **coding-agent** sessions (workers), not only durable Harnesses (**read** `session-worker.ts` is named in the list; its internals not read: **unverified** whether it hosted a Harness or an `AgentSession`).

Reusable today? Nothing deleted is on `main` or in npm at a version newer than 1.1.0 (`pi-client`/`pi-server`/`pi-protocol`, per `docs/research/pi-durable.md`, unverified here). They would be a pinned old dependency, built on the Chord API of 2026-10-12, which `main` has since changed (`CHANGELOG.md` "Watches use Chord's `WatchHandle`..."). **Unverified** that they still run against pi-durable 1.4.0.

## 2. Streaming and cross-process visibility

**Answer.** Partial deltas (text, thinking, tool-call arguments) and running tool output are **committed** to the in-memory view every 100 ms, but **stored** to SQLite only
later, in batches: at the next commit that reads `pi.live` (end of the answer, a tool settling) and at the latest after `persistWithinMs` (default **1000 ms**).
So a second process tailing the SQLite file sees streamed text up to about 1 s late with the defaults and in 100 ms steps with `persistWithinMs: 0`. Nothing
wakes a watcher in another process: every watch is driven by the in-process adoption of a commit. A Rust process must poll or file-watch the store itself
and rebuild `pi.live` from stored base documents plus Chord delta operations.

- **Committed vs delivered.** **read** `README.md:527`: "Partial answers and tool output are committed at most every 100 ms by default, and watches see each commit at once.
  Storage gets them later and together, as deferred commits ... with the next commit that reads `pi.live` ... and at the latest after `settings.progress.persistWithinMs` (default 1,000 ms)."
  **read** defaults `src/harness/agent.ts:35-37` (`partialIntervalMs: 100`, `outputIntervalMs: 100`, `persistWithinMs: 1_000`). **read** the stream loop `generation.ts:676-725`: events feed a
  `PartialChanges` tracker (text and thinking deltas, `StreamingJsonParser` for tool-call arguments), flushed by timer through `runtime.commit(..., PROGRESS_COMMIT)`.
  This deferral is **new on main**: **read** `CHANGELOG.md` Unreleased: "Partial answers and running tool output are stored later than they are shown ... A crash now loses up to about a second of streamed content instead of 100 ms ... set `settings.progress.persistWithinMs: 0` to store progress right after each commit, about as before (one storage transaction per partial)." So at **1.4.0 the partials were stored every 100 ms** (**read** the same sentence "instead of 100 ms"), and on main they are stored within 1 s by default.
- **What is stored where.** **read** `src/storage/sqlite/migrations.ts:12-85`, `schema.ts:9-19`: tables `durable_schema`, `durable_metadata`, `record_ids`, `conversations`, `entries`, `tasks`, `submissions`, `documents`, `document_revisions`.
  Entries and conversations are `record TEXT` JSON rows (easy to read). Documents (`pi.live`, `pi.inbox`, ...) are `document_revisions(document_id, seq, kind 'base'|'delta', version, content)`: a **base** value, then **delta** rows holding Chord `Op[]`
  (`["r", v]`, `["s", path, v]`, `["d", path]`, `["a", path, text]` string append, `["t", path, n]`, `["p", path, i, n, items]` array splice, `["m", path, perm]`; **read** `chord/src/delta/index.ts:32-41`). The reader in pi's own storage does
  `value = apply(base, deltas...)` (**read** `storage/sqlite/storage.ts:636-658`). A Rust reader must re-implement `apply` for those seven ops. The TypeScript is 125 lines (**ran** `wc -l packages/chord/src/delta/apply-immutable-trusted.ts` = 125).
  `checkpointWhen` decides when a delta chain gets a new base (**read** `README.md:803-830`; `inbox.ts:32` is `value.items.length === 0`).
- **Latency of the read.** Stored partials: at most `persistWithinMs` + the commit. cpd's own tail loop polls every `events.poll_ms`, default **200 ms** (**read** `origin/main:core/src/config/file.rs:370`) and `tui/src/read/watch.rs` wakes on a `notify` event of `cpd.db`/`cpd.db-wal` (**read**, 46 lines, **ran** `wc -l`).
  So an estimate for a Rust tail of `session.sqlite`: first token visible after about (`persistWithinMs` or the next `pi.live`-reading commit) + file event or 200 ms poll; **unverified** (no measurement: this Job forbids running a model or building a prototype).
- **What wakes a watcher in another process.** Nothing. **read** `README.md:479-481` "`watch()` returns a watch ... then each commit that touches it"; **read** `session/session.ts:978-1130` (publication happens "Inside `adopt`" of the storage's synchronous step); **ran** `grep -rn "data_version\|poll\|refresh" packages/durable/src/session packages/durable/src/storage/sqlite` found no cross-process change detection (only hits are comments about the line and timers); `README.md:881` states "One process owns a storage at a time; there is no cross-process locking."
  Consequence (**unverified**: I did not run two processes; inference from the three reads above): a `readOnly` Harness opened on a file another process writes gets the state at open and the in-process changes it makes (none, it is read-only); it does not tail. The spec's use case is a "synced copy", i.e. a replica kept in step by `observeCommits()` publications (**read** `docs/spec.md:2005-2022`: "A consumer that applies `observeCommits()` publications to stored state, such as a replica, stays equal to Storage in every live document").
- **WAL.** **read** `chord/src/sqlite/node.ts:206-208`: `PRAGMA journal_mode = WAL`, `synchronous = NORMAL`, `wal_autocheckpoint = 1000` pages. WAL allows concurrent readers during a writer (SQLite behaviour; **unverified** for `rusqlite` read-only opens of a WAL file without a writable `-shm`, which cpd's own store does already: `tui` reads `cpd.db` while the engine writes, **read** `tui/Cargo.toml` rusqlite, `read/watch.rs`).
- **Can a Rust process watch the store directly?** Yes for history and (coarsely) live view; read-only; no submit or abort. cpd precedent: **read** `origin/main:core/src/events/mod.rs:127-215` (`follow_loop`: `PRAGMA data_version` per poll, fetch rows past a cursor), `tui/src/read/watch.rs` (`notify`).

## 3. Steering and interruption

**Answer.** A message sent mid-turn with `whenBusy: "steer"` waits in `pi.inbox` and is placed at the next **post-tools boundary**; it never interrupts the model request in flight or a running tool. With no tool round left, it is placed at the `final` boundary and starts a successor run, like a follow-up. Abort (Esc) is `conversation.abort()`: it aborts the model request and every running tool, kills a `bash` process tree, keeps the partial answer as an `aborted` entry that the model does not see afterwards, and gives each aborted tool call an error result `Tool <name> was aborted` with the output so far.

- **Queue and placement.** **read** `docs/spec.md:2951-2955`: boundary `postTools` places "all" writes and steers (first or all by `steeringMode`), no follow-ups; boundary `final` places writes, steers and follow-ups. `README.md:541`: "Steers are placed after the current tool round and join the running work. Follow-ups are placed when the run answers, and start the next run." **read** `spec.md:2987`: at `postTools` "generation continues even with no queued trigger; selected steer IDs join that continuation". Same shape as pi's `steer` ("pi injects the message after the current tool call", **read** `origin/main:pi/lib/delivery.ts:6-7`).
- **Writes** of only `kind`/`data` are appended at once, even mid-run (**read** `README.md:557`): a chat note shows at once.
- **Esc.** **read** `README.md:714`: `abort()` withdraws queued inputs (queued writes stay), aborts every task of the current work, resolves once idle. **read** `harness/generation.ts:383-408` (the generation task's abort handler): converts a committed partial to an `aborted` `pi.assistant` entry (`generation.ts:651-658`), appends `aborted` error results for calls never started, ends the run `unanswered` with reason `aborted` (`runFailure.reason: "aborted"` in the view).
  **read** `harness/context.ts:10`: `EXCLUDED_STOP_REASONS = {"aborted", "error", "deferred"}`, so the model's next request omits the aborted partial answer. A run killed mid-request is stored as a partial in the UI history but is absent from model context (`context.ts:222-227`).
- **Running tool.** **read** `harness/tool.ts:215-233`: the tool task's abort handler settles an error result built from the call's slot with code `aborted`, message `Tool <name> was aborted`, plus the output committed so far (`fromSlot`, `tool.ts:272-300`). For `bash`, the abort signal reaches the environment: **read** `env/node.ts:717-719` (`onAbort` calls `killProcessTree(child.pid)`) and `env/node.ts:283-310` (`process.kill(-pid, "SIGKILL")` for a process group, `taskkill` on Windows); `tools/bash.ts:120-122` maps an aborted exec to an error. So Esc on a running `bash` kills it with SIGKILL, not SIGTERM, with no grace period (**read**).
- **What the model sees afterwards** (**read** above): the tool result `Tool bash was aborted` and its partial output, no assistant text of the aborted request. After an interrupted-by-restart tool: `Tool X was interrupted by a restart and may have partially run` (**read** `tool.ts:221-225`). The user message that follows is a new submission.
- **Gap against pi (**unverified** whether it matters to cpd):** pi lets a steer arrive "after the current tool call" within a round; pi-durable places steers after the whole tool round (`postTools`, **read** spec table above). A steer sent during a long model response waits for that response and its tools; there is no "interrupt and redirect now" except `abort()` then a new submit.
- **Submit during abort/idle races:** `whenIdle: "reject"` (main) makes a steer fail with `ConversationIdle` if the run already ended (**read** `README.md:536-543`), which a chat UI uses to turn a late steer into a normal input. In 1.4.0 this does not exist (**ran** `git show v1.4.0:packages/durable/src/harness/types.ts | grep -c whenIdle` printed `0`; commit `6a2825113 feat(durable): reject input on idle conversations with whenIdle`).

## 4. Human-in-the-loop

**Answer.** There is **no** built-in primitive for "wait for a client's answer": no signal, no input, no external-event wait. A task can only park `waiting` on other tasks or `sleeping` until a time. Closest examples: `test/examples/21-subagent-foreground.ts` (a `replay: "safe"` tool that rederives its child after a crash and awaits its answer) and `22-subagent-background.ts` (a background reporter task posts the answer back as a follow-up input). A durable `ask_user_question` needs one of three patterns, none shipped as an example: (A) end the run (`control: { terminate: true }`) and take the answer as the next submit; (B) a `replay: "safe"` tool that stores the question in an extension document and awaits an in-process watch of that document that the host's IPC handler writes; (C) a custom task that `sleepUntil`s in a poll loop and reads the answer document.

- **Task states.** **read** `src/types.ts:655-700`: `pending`, `running`, `waiting` ("Parked without an invocation until every task in `on` is terminal"), `sleeping` ("until the Harness clock reaches `until`"), `completing`, `terminal`. There is no `awaiting-input` state. **ran** `grep -rn -i "approval\|human\|signal(\|external event" packages/durable/docs/spec.md README.md`: only `beforeTool` returning `{ block: "Needs approval" }` (`README.md:235`) and "human effects run outside the line" (`spec.md:1844`); the spec says never to await humans inside the Session line (`spec.md:6305`).
- **`packages/chord`** (not researched beyond this question): **ran** `ls packages/chord/src/next/jobs` lists `job.ts`, `run.ts`, `running.ts`, ...; `job.ts` runs work "outside transactions" with an abort signal and records "the outcome in one transaction". No human or input wait found in `jobs/job.ts` (read the comments around lines 30-76). **unverified** for the rest of `chord/src/next` (not read; the `chord-research` Plan covers it).
- **Pattern A, closest shipped analogue:** `test/examples/26-plan-mode.ts:34-45` (**read**): the tool `submit_plan` commits the plan into the extension's own `PlanDoc` and returns `control: { terminate: true }`: "when every result of the round asks for it, the run ends without another model request" (**read** `README.md:277`). The user's reaction is the next `submit`. This is durable across a restart for free (nothing is waiting in memory), at the cost that the model does not get the answer as a tool result in the same run: it gets a new user message.
- **Pattern B, restart handling.** **read** `README.md:277` (Tools): a tool call is its own task; on reopen it reruns only when `replay: "safe"`, else the model gets an `interrupted` error result with the output so far. A replay-safe tool reruns `execute()` from the top, so it must find its question again (example 21: `scanConversations({ ownerTaskId })`, `requestId: "subagent:${api.taskId}"`, **read** `README.md:719-745`). The tool holds an in-memory invocation while it waits (**read** `README.md:162`: a phase has no sleep in the middle, "a phase reruns from its top after a restart"). Waiting hours is fine for a live process; each host crash counts toward `settings.restartLimit` (default 5) for tasks running at the crash, and "a new state the task commits starts the count over" (**read** `README.md:216`). Whether a waiting replay-safe tool commits state that resets the count is **unverified**.
- **How the answer arrives:** the cockpit is another process, and a `readOnly` Harness cannot write (**read** `README.md:127`), so the answer must go through the process that owns the Harness: IPC to the host, which commits to the document (`conversation.commit(tx => ...)`, **read** `harness/types.ts:812`) or admits a `write` submission (**read** `types.ts:96`). The tool in the same process can `harness.watchDoc(...)` for it (**read** `README.md:840`). **unverified:** that a tool can reach `harness.watchDoc`; `api` exposes `api.commit`, `api.conversation`, `api.doc`-style reads (**read** `README.md:719-745`), not read in full.
- **cpd today does the HITL wait in cpd's store**, not in pi: `origin/main:core/src/ask/question.rs:1-60`: `ask` inserts an `open` Escalation row, then polls its row every 500 ms (`POLL`) until `answered` or a timeout (`timeouts.ask_minutes`). `ask_user_question` and plannotator plan review are **pi TUI tools** (`origin/main:pi/lib/adapter.ts:75`, `askguard.ts`; vendored `pi/ask-user-question`). Neither loads in pi-durable (a different extension API, **read** `docs/research/pi-durable.md` "Extensions (a correction)"). They would be rewritten as `defineTool`s whose prompt is a ratatui dialog, with the question stored in a durable document and the answer returned by IPC.
- **Plannotator** (a separate server process with a browser UI, **unverified**: not read here) is independent of pi's loop; only its pi-extension glue needs porting.

## 5. Hosting

**Answer.** The process that calls `Harness.open()` owns the storage, the registry, the scheduler, all timers and every watch; nothing else can write. One Harness can hold many conversations (root, forks, `createConversation`, owned subagent conversations); one Node process can open several Harnesses (one per SQLite file) by construction, but pi-durable ships no multi-session host. A long-lived chat Session is fine across crashes: idle conversations count no restarts; running work is counted and ends at `restartLimit` (default 5, `Infinity` allowed). SQLite is versioned (`durable_schema.version`, now 2, additive migrations, newer-than-supported refused) but the SQL layout is **not documented** as an interface; history is read back through `Conversation.entries()` or by reading the JSON rows.

- **Owner.** **read** `README.md:881`: "One process owns a storage at a time; there is no cross-process locking." The deleted TUI added its own `proper-lockfile` (**read** `experimental/durable/sessions.ts:44-54`). **read** `README.md:92-93`: a Harness is "one open storage plus the machinery that runs agents on it. All changes go through one line of atomic commits."
  **read** `src/session/session.ts` and `harness/harness.ts:211-262`: a `HarnessImpl` takes one `Storage`; the registry is "process-local" (`docs/spec.md:3022`), "the registry stores nothing", conversations store extension names (**read** `README.md:224-241`).
- **Many conversations.** **read** `README.md:703-709` (`harness.createConversation`, `root.fork`), `README.md:716-757` (owned subagent conversations, background anchors), example `22-subagent-background.ts` (persistent subagents). A Manager, its Leads and their Workers could be conversations of one Harness per cpd Context, or one Harness per Session; both are structurally allowed (**unverified** for scale: no multi-hundred-conversation measurement; `scheduler.ts` is 2565 lines, **ran** `wc -l`).
  One Harness is a single line of commits (**read** `README.md:93`): all conversations in it serialise their commits. The `storage-benchmark.ts` exists for it (`src/testing/storage-benchmark.ts`, not read).
- **Not provided:** a registry of sessions across processes, discovery, or an RPC server. That is what `pi-server` was (section 1) and it was deleted.
- **`restartLimit` and crash recovery for a chat.** **read** `README.md:216`: a crash is counted for "each task a crash, eviction, or reset cut off", default 5 (`settings.restartLimit`); the 5th time the task ends: tool call `restart_limit` error result, run `runFailure.reason: "restart_limit"`, own task `faulted`. A model request in flight at the crash is a failed attempt under `settings.retry` (backoff), "partial answer kept as an aborted entry". `settings.afterFailedRun: "continue" | "withdraw"` decides queued inputs after a failed run (**read** `README.md:220`). A Session sitting idle has no running task, so it accrues nothing (**inferred from** "open counts each task a crash ... cut off": **unverified** against `scheduler.ts`). The old TUI relied on `--continue` plus the same recovery ("kill the process in the middle of a tool call and start it again with `--continue`: the interrupted call gets an interrupted result and the turn finishes", **read** its README).
  1.4.0 differs: "restart limit" and "model request is a failed attempt" are listed in `docs/research/pi-durable.md` as "After 1.4.0" and the **changelog** puts them under Unreleased (**read**); at 1.4.0 a crashed work was parked (that doc, **read**, not re-verified here).
- **Schema and version guarantees.** **read** `storage/sqlite/migrations.ts:105-117` (two versions: 1 initial, 2 rebuilds `tasks` to add the `sleeping` status) and `chord/src/sqlite/migrations.ts:12-47`: contiguous versions, applied in one transaction, `SQLite schema version N is newer than supported version M` throws. **read** `schema.ts:7-9`: "Immutable identifier history ... Never change or remove an existing value". Table prefix option `{ prefix: "pi_" }` (**read** `README.md:864-870`). The README documents the API, not the tables; **ran** `grep -n -i "durable_schema\|document_revisions\|durable_metadata" packages/durable/docs/spec.md README.md` printed nothing, so the layout is internal: a Rust reader couples to it (**read** the file list above), with the migration table as the only compatibility signal. `CURRENT_SQLITE_SCHEMA_VERSION = SQLITE_MIGRATIONS.at(-1)?.version` = 2 (**read** `migrations.ts:119-122`). Integer id columns are `INTEGER` but `next_id` is `TEXT` because "node:sqlite rejects INTEGER results outside JavaScript's safe integer range" (**read** `migrations.ts:14`).
  Node needs `node:sqlite` (**read** `chord/src/sqlite/node.ts`, `new DatabaseSync(path, { timeout })`): **unverified** which Node version cpd would require; `docs/research/pi-durable.md` says "Runs on Node + SQLite".
- **History for a cockpit viewer.** **read** `harness/types.ts:~845` `Conversation.entries(query, limit, cursor, ctx): Page<EntryRecord, Cursor>` (fork-aware, newest first unless `order: "ascending"`); `context()` for what the model sees; `ConversationTranscript` for "load earlier" and gap markers (**read** `README.md:497-523`, example `31-transcript.ts`, 110 lines). Direct SQL: `entries(id, conversation_id, head, commit_seq, record JSON)` with index `(conversation_id, id DESC)` (**read** `migrations.ts:30-39`); but fork and head semantics (what is "active") are in code (`harness/transcript.ts`, `context.ts`), so a Rust history viewer that must show exactly the model's view would re-implement them (**unverified** size).
- **Reload.** `registry.install(ext)` replaces an extension in place; running calls keep the old code (**read** `README.md:467-475`). That is the pi-reload analogue (compare `docs/research/pi-reload.md`).
- **Closing / eviction.** `harness.closed.then(end => end.kind === "failed" && reopen())` (**read** `README.md:921-924`); `onWake` for hosts that are evicted (not needed for a Node process).

## 6. IPC options from Rust

Common facts (**read** unless marked): pi-durable is TypeScript only; the Rust side cannot link it. The owner of the storage is the only writer; every write (submit, abort, answer, model change) must run in the Node process. The Rust cockpit has no tokio; it has `notify`, `rusqlite`, `serde_json`, `nix` (**read** `origin/main:Cargo.toml:10-37`), std threads and `std::process` already spawn children with pipes (`origin/main:core/src/context/engine.rs:290`, `host/spawn.rs`). A new crate needs a written reason (`docs/implementing.md`).

| | (a) Rust tails SQLite read-only | (b) Node host, JSON lines over a Unix socket (or stdio), Rust client | (c) Rust spawns the Node host per Session |
|---|---|---|---|
| Parts | rusqlite read-only open, `notify` + `data_version` loop, Chord `apply` in Rust, serde model of entries and `pi.live`, history queries | a Node host (Harness + registry + watchers + request handlers), a framing/JSON schema, Rust client thread + serde model | (b) with the host as a child process; stdio as transport |
| Read path | direct, stale by `persistWithinMs` (main) | exact: host forwards each `watch` call (`value` or `ops`) at once, no 1 s deferral (**read** README:527: "watches see each commit at once") | same as (b) |
| Write path | **none**: needs a second channel to the host for submit, abort, answers | the same socket | the same pipes |
| Failure modes | schema drift on a pi-durable upgrade (internal layout); torn view between base and delta rows unless read in one transaction (**read** `storage.ts:553` comment: "must observe one committed state"); no signal of host death; `PRAGMA data_version` blind to own-connection writes (irrelevant here) | host down: reconnect, re-attach gets a fresh view "nothing is replayed" (**read** `README.md:529`); slow client: folded callbacks, 10,000 ops then full view (**read**); stale socket; protocol drift between Rust and Node is a cpd-owned contract | cockpit exit kills (or orphans) the chat unless detached; N Sessions = N Node processes (RSS: the package root loads TypeBox "about 23 MB of peak RSS unbundled", **read** `README.md:908`, plus Node itself, **unverified**) |
| cpd parts that fit | `core/src/events/mod.rs:127-215` (`data_version` poll), `tui/src/read/watch.rs` (46 lines, `notify` on db and `-wal`) | `pi/lib/follower.ts` (160 lines) is exactly "child process, line by line, restart with backoff and a cursor"; `pi/lib/delivery.ts` (44 lines) maps events to steer/follow-up; `cpd session state` for working/idle; `cpd events follow` for inbound cpd events | same as (b); `launch.rs` (548 lines) is the place that starts `pi` today |
| Re-creates of the deleted stack | none | a small subset: `pi-protocol` 1021 + `pi-server` 1754 + `pi-client` 1327 non-test lines were a general Chord-routed, CBOR, multi-attach, multi-session stack (**ran**, table above); a one-client JSON-lines protocol needs the request/response, snapshot and update frames only | same as (b) |

### Sizes (estimates; each with its basis, none measured)

All lines are estimates. Basis: the old durable TUI's boundary file `experimental/durable/runtime.ts` is **373 lines** and covers open, view source, controller (submit, abort, compact, model, conversation switch) around a ready Harness (**ran** `wc -l`, section 1). The old `tui.ts` was 660 lines of rendering (not needed: ratatui replaces it). `harness-setup.ts` 123 lines (registry, settings, environments). The two cpd precedents above.

| Option | Node lines | Rust lines | Basis |
|---|---|---|---|
| (a) tail | 0 for reading; a write channel still needed | 700-1000 | store reader 200 (rows to serde: `EntryRecord`, `Message`), `apply` for 7 Chord ops 150-200 (TS is 125 lines, `apply-immutable-trusted.ts`), `pi.live` / `pi.inbox` / `pi.usage` serde 150, tail loop 90 (cpd's `follow_loop` is about 90 lines, **read**) + watcher 46, view assembly and tests 200-300. Excludes all writes |
| (b) host + socket | 400-700 | 500-800 | Node: `runtime.ts` 373 (open, view, controller) + framing and request handlers 100-200 + cpd glue (adapter port: guards, policy, time zone; **not counted**: `pi/extension.ts` + `pi/lib` is far larger, **ran** `git ls-tree origin/main pi/lib` lists about 50 files); Rust: client thread and reconnect 150, serde model of the view 200-300 (the same types as (a) minus `apply`), controller commands 100, tests 150-250 |
| (c) child per Session | (b) + about 50 | (b) + 100-200 | spawn, health, restart with backoff (cpd's `Follower` is 160 lines TS), lifecycle bound to the cockpit |

Whether the frames carry full views or Chord ops is a choice that changes the Rust size by the `apply` port (about 150-200 lines) against frame size: the full `pi.live` view repeats the whole partial message every 100 ms (**read** `README.md:527`: ops exist to avoid that); no frame-size number was measured (**unverified**).

### Option notes

- **(a) alone cannot be the chat.** It has no way to submit or abort (**read**: only the owner writes). It is a viewer (history, a second watching pane); it needs (b)/(c) or a cpd-store path for input. A hybrid exists in cpd's own pieces: Rust writes a cpd Event (`session-message`) and a Node follower (`pi/lib/follower.ts` + `delivery.ts`) submits it to the Harness (`whenBusy` per `delivery.ts`), while Rust reads output from SQLite: **unverified** design, listed only because both halves exist.
- **(b) vs (c) is about who owns the host's life.** Managers and Leads outlive a cockpit (they run in tmux windows today, **read** `core/src/host/pi/launch.rs:25`). A host that is a child of the cockpit dies with it (a crash then recovers on reopen, but nothing drives the run meanwhile). A host started by cpd's engine/driver and attached to by sockets is (b). That is an inference from cpd's current lifetimes, **unverified** against the intended design of the Plan.
- **Moving parts for any option that has a Node host:** open storage and registry per Session, `harness.resume()` on start, `harness.closed` handler to reopen, `onReport` to cpd's error path, one lock per storage (the deleted TUI's `proper-lockfile`, **read**), models and auth (`pi-ai`; whether `~/.pi` logins work is **unverified** per `docs/research/pi-durable.md`), the system prompt sections that pi supplies today (AGENTS.md, skills, time zone, policy: pi-durable loads none, **read** `docs/research/pi-durable.md`).
- **Model of the interface the old stack settled on:** one process owns Harness + view; the UI gets plain values and a small controller (`DurableView`, `DurableController`, **read** `runtime.ts:40-95`). A JSON-lines host would serialise exactly that.

## Corrections to `docs/research/pi-durable.md`

Each is a place where the source disagrees (or the doc went stale since it was written 2026-10-27 EDT):

1. **"`viewState()` (1.4.0) / `watch()` (after 1.4.0)" and "`viewState()` becomes `watch()`"**: both exist at 1.4.0 (**read** `v1.4.0` README 476-499; `watch()` there takes a positional `(value, ops)` callback). On `main`, `viewState()` is **removed** (not renamed), `watch()`'s callback takes one change object, and the view's `entries` became a window (`CHANGELOG.md` Unreleased).
2. **"`watchEvents()` turns commits into the coding agent's familiar events"**: true at 1.4.0 (a free function `watchEvents(harness, id, ctx)`, **read** `v1.4.0` README "Agent Events (Experimental)"), but `main` **removed** `watchEvents()`, its event types and spec §9.4 (`CHANGELOG.md` Unreleased). A UI built on 1.4.0 events breaks on the next release; one built on the view does not.
3. **"cpd could watch a Worker without owning it" (`readOnly: true`)**: the source shows `readOnly` only blocks writes; it adds no change detection (section 2). A second process does not get live updates through it (inference from the reads in section 2, **unverified** by a run). The spec's wording is "a synced copy on a device". Also `readOnly` is after 1.4.0 (not in the npm 1.4.0 `dist`, **ran** above).
4. **Partials are stored on a 1 s deferral on `main`** (the doc says "committed to storage before anything is shown"): that is README concept text at 1.4.0 ("nothing is shown before its commit is stored", **read** `README.md:93`). On `main`, partial answers and running tool output are shown before they are stored ("Watches and the view see it at once", `README.md:527`, `CHANGELOG.md`). The "committed before shown" invariant still holds for entries and tasks (deferral is for exactly "generation partials" and "running tool progress", **read** `spec.md:2023-2026`).
5. **"`main` at `b3a29f11` (2026-10-27 16:57 UTC)"** is stale: `main` is `33385f95` (2026-10-28 14:53 EDT, **ran**). The "After 1.4.0" list is still true; additions since: `whenIdle: "reject"`, `submit(tx, draft)`, `{ type: "continue" }`, per-conversation `maxTurns`, `afterRun` hooks, `ConversationTranscript`, the entries window, `watchEvents`/`viewState` removal, the deferred partial stores (`CHANGELOG.md` Unreleased; **read** the README of `main`).
6. **Example numbers**: the doc's "32 examples ... `18-print.ts`, `19-json.ts`, `26-coding-agent.ts`" and the Plan text's "`31-reload-and-restart.ts`" are the `v1.4.0` names (**ran** `git ls-tree --name-only v1.4.0 packages/durable/test/examples/`: 32 files, `18-print` ... `31-reload-and-restart`). On `main` there are **35** (**ran** `ls packages/durable/test/examples | wc -l`) and they are renumbered: `17-print.ts`, `18-json.ts`, `25-coding-agent.ts`, `30-reload-and-restart.ts`, `31-transcript.ts`, `34-submit-in-commit.ts`.
7. **"SQLite (`pi-durable/storage/sqlite/node`, `NodeSqliteStorage`)"**: there is no `NodeSqliteStorage` identifier; the entry point is `openNodeSqliteStorage(path, options)` returning a `SqliteStorage` (**read** `src/storage/sqlite/node.ts:15-22`; **ran** `grep -rn NodeSqliteStorage src` matched only the options type `NodeSqliteStorageOptions`).
8. **Not wrong, now sourced:** `package.json` has no `bin` (**ran** `git show v1.4.0:packages/durable/package.json | grep -c '"bin"'` printed `0`; the packed 1.4.0 `package.json` also has none, **ran** `grep -n '"bin"' pk/package/package.json` printed nothing).
9. **"the client/server stack (`@earendil-works/pi-client`, `pi-server`, `pi-protocol` 1.1.0 on npm, 2026-10-07)"**: the stack was **not** a pi-durable client: it hosted worker-per-Session Chord services (section 1). Its removal commit says nothing about why (section 1); the doc's phrase "client/server stack ... for the UI" would overstate how close it is to a durable chat protocol.

## What pi-durable gives a chat UI / what is missing

| Chat-UI need | pi-durable gives (1.4.0 / main) | Missing for a Rust cockpit |
|---|---|---|
| Send a message, steer, queue | `submit({ whenBusy: "steer" \| "followUp" \| "reject" })`, `requestId` idempotence, `pi.inbox` view (**read**) | an IPC from Rust to the owner process; no client API of its own; `whenIdle` and `continue` only on main |
| Live transcript | `watch()` view: entries window, `pi.live` partial, tools, queue, usage; `ConversationTranscript` (main) | a Rust model of the view; transport; nothing is "replayed" on reconnect, a client re-attaches to a fresh view |
| Thinking, partial tool-call arguments, tool output | all inside `pi.live.generation.message` and `pi.live.tools` at 100 ms | none inside the view; a Rust reader of the store gets them 1 s late by default (main) |
| Esc | `conversation.abort()` kills model request and tools (SIGKILL on the `bash` process group), keeps an aborted partial entry, tool result `... was aborted` + output so far | `abort` is a write; it must reach the host; no SIGTERM-then-kill grace |
| Steer mid-turn | placed after the tool round (`postTools`) | not "after the current tool call"; no instant interrupt other than `abort()` |
| Blocking question / plan review across hours and a restart | durable state machine, `replay: "safe"` tools, `terminate`, `sleepUntil`, `waiting` on tasks | no "wait for external answer" primitive or example; cpd would build pattern A, B or C, plus the Rust dialog |
| Resume after crash | `resume()`, retry of a failed request, `restartLimit`, `runFailure` reasons, `continue` (main) | cpd's driver does not restart a Node host today; the pi-specific `judge` of A5 reads pi's session file |
| Second process watching | file is plain SQLite in WAL, versioned migrations, JSON rows | no cross-process wake-up (**read**); `readOnly` is a replica mode, not a tail; the layout is internal; deltas need a Chord `apply` port |
| History viewer | `entries()` pages, `context()`, `ConversationTranscript` | in Rust: reimplement fork/head rules or go through the host |
| Many conversations in one process | `createConversation`, forks, owned subagents, one Harness = one line of commits | no multi-session host, registry or discovery (the deleted `pi-server` was that) |
| Extensions of cpd (guards, policy, prompt sections, plannotator, compact-tools, ask-user-question) | `defineExtension` with tools, sections, hooks (`beforeTool` block), wraps, tasks (**read**) | a different API from pi's `ExtensionAPI`; the adapter would be rewritten (per `docs/research/pi-durable.md`) |
| Models and login | `pi-ai` model runtime | whether pi's `auth.json` subscriptions work: not verified |

## Not verified

- **Cross-process behaviour of `readOnly`:** no two-process run (rules: no prototype). Claim "no live update" rests on reading `session.ts`, `storage/sqlite/storage.ts` and `README.md:881`.
- **Real latency** of a Rust tail (first-token delay, persist batching under load): not measured; only the defaults (100 ms, 1000 ms, cpd poll 200 ms) are read.
- **Why `7472cda0` removed the stack:** the message lists what, not why; no linked issue was read. Whether `session-worker.ts` hosted a pi-durable `Harness` or a coding-agent `AgentSession` was not read.
- **Whether the removed packages still run** against pi-durable 1.4.0 or `main`.
- **Idle crash counting and `restartLimit` reset** for a waiting replay-safe tool: read from README prose, not from `scheduler.ts`.
- **That a tool can reach `harness.watchDoc`** or an equivalent inside `execute()` (the tool `api` surface was not read in full).
- **`rusqlite` read-only opening of the WAL file** while Node writes, and SQLite-level torn reads between a document's base and delta rows without one transaction (the pi code reads them in one transaction, `storage.ts:553`).
- **Node version and memory per Node host** for N Managers/Leads; `node:sqlite` availability in cpd's Node.
- **The remaining parts of `packages/chord`** for a human-wait primitive (only `next/jobs/job.ts` comments were read).
- **Whether the cpd hybrid** (cpd Event in, SQLite tail out) is acceptable: both halves exist, the design was not checked.
- **All line-count sizes in section 6** are estimates from the cited bases; none was measured on a prototype.
- **Model and auth**: not examined in this Job.

## Commands (for the numbers above)

```text
npm view @earendil-works/pi-durable version dist-tags
git clone https://github.com/earendil-works/pi /tmp/dui/pi ; git -C /tmp/dui/pi rev-parse HEAD
git diff --stat v1.4.0 HEAD -- packages/durable | tail -1
npm pack @earendil-works/pi-durable@1.4.0 ; tar xzf ... ; grep -rl readOnly pk/package/dist/harness/types.d.ts | wc -l ; grep -c watchEvents pk/package/dist/index.d.ts
git merge-base --is-ancestor be14d9732 v1.4.0
git show 7472cda0 --stat ; P=7472cda0^ ; git ls-tree -r $P --name-only -- <dir> | while read f; do git show "$P:$f" | wc -l; done | awk '{s+=$1;n++} END{print n" files, "s" lines"}'
wc -l packages/durable/src/{storage/sqlite/*.ts,harness/*.ts,session/*.ts}  packages/chord/src/delta/apply-immutable-trusted.ts
git ls-tree --name-only v1.4.0 packages/durable/test/examples/ ; ls packages/durable/test/examples | wc -l
git show origin/main:tui/src/read/watch.rs | wc -l ; git show origin/main:pi/lib/follower.ts | wc -l
```
