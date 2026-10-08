# Decisions made unattended (2026-10-07 night)

The user was away. Standing instruction: *"if new decisions are required, opt for the recommended
one, the one that would be more native to the chosen stack, the simplest and more vanilla.
Document every decision made without the user."*

Every decision below is reversible. Each lists what was chosen, why, and what the alternative was.

---

## D86 — The daemon is a plain Node process, started separately from Vite

**Chosen:** `packages/daemon` (or `daemon/`) run with `node --watch`, Vite stays a pure frontend
dev server, two terminals (or one `pnpm dev` that runs both via a script).

**Why:** the daemon owns the SQLite file and the Harness. Vite's dev server restarts on every file
edit; an owner that restarts mid-turn is exactly the failure Pi Durable exists to prevent.

**Alternative rejected:** Vite middleware plugin. Fewer processes, but couples DB ownership to the
HMR lifecycle.

The user already chose "daemon, with pi embedded" in an earlier clarify. D86 only fixes *how* it is
started.

---

## D87 — Transport is HTTP + SSE, not WebSocket

**Chosen:** `POST /api/...` for commands, `GET /api/stream` (Server-Sent Events) for the live feed.

**Why:** most vanilla option that satisfies the requirement. Durable's `viewState().subscribe` is
one-way server->client; SSE is one-way by design, is plain HTTP, auto-reconnects in the browser
with zero library code, and needs no extra dependency. A WebSocket would add a dependency and a
heartbeat/reconnect protocol to carry traffic that only ever flows one way.

Client commands are ordinary `fetch` POSTs. Vite dev-proxies `/api` to the daemon, so the browser
sees one origin and there is no CORS configuration.

**Alternative rejected:** WebSocket (`ws`). Revisit only if client->server streaming is needed.

---

## D88 — The IPC stays ACP-shaped

**Chosen:** keep the previously agreed ACP-shaped seam: `session/new`, `session/prompt`,
`session/update`-style names, even though Durable is now in-process.

**Why:** the user's existing decision ("lets use PI, but keep the IPC ACP-shaped for easy swap").
Nothing about embedding Durable invalidates it; the seam now protects in both directions.

---

## D89 — No new runtime dependency in the frontend for engine state

**Chosen:** a hand-written `useEngineStream()` hook over `EventSource` + `useSyncExternalStore`.

**Why:** "zero customization, vanilla" standing preference. React 19 ships
`useSyncExternalStore`, which is precisely the primitive for subscribing to an external store.
No TanStack Query, no Zustand, no Redux.

**Alternative rejected:** a query/state library. Nothing here needs cache invalidation or
optimistic mutation yet.

---

## D90 — Chat UI uses Mantine primitives only

**Chosen:** `ScrollArea` + `Stack` + `Paper` + `Textarea` + `ActionIcon`. No custom chat library.

**Why:** "mantine is our UI language"; these all exist in 9.7.1 and are already in the bundle.

---

## D91 — The lead conversation is the Durable ROOT conversation

**Chosen:** one root per CPD project, created with `harness.root(ctx, { agent })`.

**Why:** Durable reserves a root conversation per Session and `ROOT_CONVERSATION_ID` is exported.
Plans fork from it, which is exactly the hierarchy. Using the built-in root rather than creating a
sibling keeps `scanConversations` results meaningful (the root is the one with no `parent`).

---

## D92 — Model is `claude-opus-5` via the existing Copilot token

**Chosen:** read `~/.pi/agent/auth.json`, provider `github-copilot`, `modelId: "claude-opus-5"`.

**Why:** The user prefers Copilot as provider; the token already exists, is unexpired, and lists 34
models. No login flow needs building.

**Public-repo note:** `auth.json` is read from `$HOME` at runtime and is NEVER copied into the
repo. The path is configurable via `CPD_PI_AUTH`. Nothing secret is committed.

---

## D93 — SQLite file lives at `.cpd/cpd.sqlite`, gitignored

**Chosen:** `.cpd/` added to `.gitignore`.

**Why:** it holds conversation transcripts, i.e. potentially identifiable content. The repo is
intended to stay public.

---

## D94 — Jobs stay fixtures this round

**Chosen:** unchanged. Only the lead chat becomes real.

**Why:** the user's explicit minimum bar: "lead chat working inside CPD, persistent across
restarts". Job execution is the next phase.

---

## D95 — Daemon is TypeScript, run with `node --watch`, no bundler

**Chosen:** `tsc` to `dist/` or direct `node --experimental-strip-types`.

**Why:** Node 22.22 is installed and strips types natively. Vanilla and dependency-free.
If it proves flaky, fall back to `tsc --watch` + `node --watch dist/index.js`.

---

## Engine facts established empirically tonight

These are not decisions, they are measurements. All verified by running code.

1. A real Copilot turn completes through Pi Durable and **survives a cold restart** — reopening the
   SQLite file replays `pi.user`, `pi.system`, `pi.assistant`.
2. Four non-obvious requirements, each of which fails **silently**:
   - `createModels()` has **no** `providers` option; use `models.setProvider(...)`.
   - `CredentialStore` is app-owned and must be supplied; pi's `auth.json` is already the right shape.
   - `harness.resume()` must be called or no task is ever scheduled.
   - the agent doc field is **`modelId`**, not `id`. Wrong name settles `unanswered`/`no_model`.
   - `submit()` takes `{ type: "input", content }`, not `{ text }`.
3. `await submission.wait(ctx)` returns `{status, reason}` — `reason` is the only diagnostic.
4. Durable **does** have the conversation tree: `ConversationRecord.parent {conversationId, at}`
   plus an `owner` edge. `harness.conversation(id)` reopens any thread;
   `storage.scanConversations()` enumerates them. What it lacks is pi's in-place *leaf cursor*
   and automatic branch summaries.
---

## D96 — Daemon tests run as a chained second suite, not a Vitest workspace

**Chosen:** root `"test": "vitest run && pnpm --filter @cpd/daemon test"`, with the root Vitest
config excluding `daemon/**`.

**Why:** the app suite is jsdom, the daemon suite is node. Two environments in one Vitest project
needs either a workspace file or per-file environment pragmas. Chaining two plain `vitest run`
calls is the simplest thing that works and keeps each config honest about its environment.

**Caveat for whoever reads CI output:** `pnpm test` now prints **two** summaries
(19 files/114 tests, then 1 file/11 tests = 125 total). Reading only the last one
undercounts badly — I made exactly that mistake tonight.

**Revisit if:** a third suite appears. At that point a real Vitest workspace earns its keep.

---

## D97 — `.worktrees/**` is excluded from the root Vitest config

**Chosen:** `exclude: [..., '.worktrees/**', 'daemon/**']`.

**Why:** each card runs in a git worktree *inside* the repo, so every sibling branch's `src/` is
visible to the runner. Without this exclusion a builder sees other cards' in-progress failures and
cannot trust its own gate. Introduced by a builder; endorsed.

---

## D98 — Dependent cards block themselves rather than guess

Observed, not decided, but worth keeping: G1 was dispatched while F1 was still open. The builder
checked `git merge-base --is-ancestor`, found F1's commit absent from `main`, and **blocked itself
with evidence** instead of inventing the daemon's API surface.

**Lead-side fix:** a worktree is created at dispatch time and does not auto-rebase when its parent
merges. After merging a parent, `git reset --hard origin/main` in the dependent worktree before
unblocking, or the builder restarts against a stale base.

---

## Prototype status — the bar is met

Verified by the lead in a browser against the real stack, not by reading a builder's report:

- typed a message into CPD's own chat panel;
- it reached GitHub Copilot through the embedded Pi Durable harness;
- the reply streamed back over SSE and rendered;
- the transcript survived a full daemon restart (SQLite replay);
- Vite proxies `/api` to the daemon, single origin, no CORS.

Jobs remain fixtures, as scoped. **Known defect:** user message bubbles overflow the panel's right
edge — filed as card G2, not hand-patched.
---

## D99 — CPD enforces single-daemon ownership of the SQLite file itself

**Context:** Pi Durable's README says *"One process owns a storage at a time; there is no
cross-process locking."* That invariant is **unenforced**. Verified empirically: opening the same
SQLite file twice succeeds both times, and two harnesses over those handles also both open. SQLite's
`busyTimeoutMs` is a per-statement wait, not an ownership lease.

**Chosen:** a lockfile at `<dbPath>.lock` holding `{pid, startedAt}`, created with `fs.openSync(…,
"wx")` (atomic create-or-fail), with `process.kill(pid, 0)` liveness detection so a dead owner's
lock is reclaimed automatically. A live owner causes the second daemon to refuse and exit non-zero.

**Why this and not something cleverer:** most vanilla option that actually works. An advisory
`flock` needs a native dependency; a control socket is a protocol we'd have to maintain. A pid
lockfile is stdlib-only and sufficient for a single-machine personal tool.

**Why it matters in practice:** `pnpm dev:all` plus a stray `pnpm daemon` in another terminal is
two daemons on `.cpd/cpd.sqlite`, diverging silently with no diagnostic.

Filed as card G3. **Flagging for review:** if CPD ever runs its daemon on more than one machine
against shared storage, a pid lockfile is the wrong abstraction and this needs revisiting.

---

## D100 — Observer state must be disposed; `watch()` is single-consumer

`viewState()`, `documentState()` and `taskGraph()` return refcounted mounts — *"built on the
Session line by its first observer and dropped with its last."* Without `dispose()` they leak for
the daemon's lifetime. F1 subscribes per SSE client, so this is a real leak; included in G3.

`WatchHandle.start()` installs *"the sole asynchronous listener"* — one watch serves exactly one
consumer, and listeners are serialized so a slow client backpressures the whole watch. **If CPD
ever serves multiple browser tabs, the daemon must fan out itself.** Not needed yet (single local
user), but it is a load-bearing assumption worth stating.

---

## D101 — Deferred: `watchEvents` for token-level streaming

`watchEvents(harness, conversationId, context)` is a **top-level export**, not a method — which is
why probing `root.watchEvents` returned `undefined`. It is marked *Experimental*, but it is the
only observation path with a documented safe overflow policy (undelivered batches collapse to a
single snapshot); `viewState`/`watch` may silently skip sequences under overflow.

**Chosen: not now.** The current SSE transcript works and is simple. When CPD wants in-flight
rendering (streaming assistant text, live tool-execution status), `watchEvents` is the intended
door, carrying 22 event types including `message_update` and `tool_execution_*`.

Note: progress is throttled at ~100ms (`ProgressPolicy`), **not per token**, and every tick is a
storage write — that knob trades UI smoothness against write amplification.

---

## D102 — Tune compaction explicitly before relying on it

Generation blocks above `contextWindow - reserveTokens`. Copilot's `claude-opus-5` advertises a
**1,000,000-token** context window; with the default `reserveTokens` of 16384, compaction fires
extremely late. Not urgent for short lead chats, but it must be set deliberately before any
long-running thread. Unset defaults are not a decision.

---

## D103 — Offline model tests use pi-ai's `faux` provider

pi-ai ships 44 providers including `faux`, a fake. Prefer it over hand-mocking the model layer in
daemon tests. No card yet; the rule applies when a test needs a model.

**Correction to an earlier claim of mine:** I previously recorded that `storage.close?.()` might be
undefined. It is not — `close(context)` is a **required** member taking a **required** context. In
practice call `harness.close(context)` instead, which seals admission, settles commits, then closes
storage.
---

## D104 — The spacing scale has been dead since PR #18, and the tripwire did not catch it

**The defect:** `src/theme.css` had a stray `}` at line 39 (introduced by commit `037a8a4`, PR #18)
that closed `:root` early. Every declaration after it — the **entire** `--space-1..6` scale plus
`--pad` — became an orphaned top-level declaration that no element ever sees.

**Verified in the running app, not inferred:**

```
--bg-panel  -> "#1c1c1c"                 (declared BEFORE the brace: resolves)
--blue-tint -> "rgba(111,168,234,0.2)"   (before: resolves)
--pad       -> ""                        (AFTER line 39: dead)
--space-2   -> ""                        (dead)
computed padding on the ScrollArea root -> 0px
```

Brace balance: 4 open / 5 close; depth reaches 0 at line 39 and −1 at line 71.

**How long it survived:** every single gate stayed green the entire time. `pnpm typecheck`,
`pnpm test` (135 tests), and `pnpm build` cannot see a CSS brace. `src/theme.test.ts` — the
tripwire written specifically to guard the token system — asserts each token is *declared
somewhere in the file*, which a text grep satisfies whether or not the token is inside `:root`.
**The test passed while the thing it guards was completely broken.**

**Chosen:** bundle the one-line brace fix into G4 rather than filing a separate card. G4's own
acceptance criterion (measure a real padding gap) is unverifiable — and its fix is a no-op — while
`--pad` resolves to the empty string. One defect, not two.

**Required alongside it:** a *positional* assertion in `theme.test.ts` — braces balanced, and every
`--space-*`/`--pad` declared **inside** the `:root` block. A declaration-exists grep is exactly
what failed here.

**Lesson worth keeping, and the one I'd most want challenged:** three green gates plus a
purpose-built tripwire did not notice that every spacing token in the application was dead. It
surfaced only because a builder tried to *measure* a real pixel value and found nothing to measure.
Assertions about source text are not assertions about behaviour. Our suite is almost entirely the
former.

**Credit:** the builder on G4 found this, correctly judged it out of scope for a padding-only card,
and stopped to ask instead of silently widening its diff. That was the right call.
---

## D105 — Conversation titles live in our own table, not Durable's schema

Durable has no title field. G1 added a `conversation_titles (id, title)` table in the **same**
SQLite file, opened as a second connection via `openNodeSqliteDatabase`.

**Why:** writing into Durable's own tables would couple us to its schema across upgrades. A
sidecar table in the same file keeps one file to back up and one thing to delete.

**Verified safe w.r.t. G3's lock:** the second connection is opened by the *same* process that owns
the lock, so the single-owner invariant holds. SQLite handles intra-process connections fine.

**Title rule:** stored override wins, else derived from the first user message (60 chars, cut at a
word boundary, ellipsis); root is always "Lead"; empty is "Untitled".

---

## D106 — New conversations are detected by a 1s poll

Durable emits no dedicated "conversation created" event. G1 polls `scanConversations` every second
and pushes `{type:"conversations"}` over SSE when the set changes.

**Chosen** as the simplest thing that works. **Flagged for your review:** a 1s poll is a placeholder,
not a design. If the tree grows large this is the first thing to revisit — `watchEvents` (D101) is
the likely replacement.

---

## D107 — `/api/messages` and `/api/stream` take an optional `?conversation=<id>`

Defaults to root, so F2's existing chat panel is untouched. Keeps the frozen API contract
backward-compatible while making every thread addressable.

**Verified live by the lead** (not from the PR body) — fork from entry 7, then restart:

```
POST /api/fork {"at":7,"title":"Lead review thread"} -> {"id":16}
GET  /api/conversations ->
  [{"id":1,"parentId":null,"at":null,"title":"Lead"},
   {"id":16,"parentId":1,"at":7,"title":"Lead review thread"}]
GET  /api/messages?conversation=16 ->
  [{"role":"user","content":"Reply with exactly: PROTOTYPE-LIVE"}]   # history up to `at`, nothing after
```

After killing and restarting the daemon the tree was byte-identical, and the G3 lock released
cleanly on SIGTERM — no stale-lock refusal on restart.

**The backend for the navigation tree is now real.** `parentId`/`at` are exactly what the sidebar
needs; no UI consumes it yet.
---

## D108 — The real conversation tree renders ALONGSIDE the fixture model, not merged into it

**The mismatch.** `src/model/navigation.ts` encodes a fixed three-level hierarchy
(`leads -> plans -> jobs`) keyed by **string** ids. `/api/conversations` returns a tree of
**arbitrary depth** keyed by **numeric** ids, where a fork of a fork is just another node. They do
not line up. `src/model/types.ts` already says so at the top: *"PROVISIONAL … not a contract.
Expect churn."*

**Chosen:** for H1, render the real tree as its own additive sidebar section. `navigation.ts`,
`types.ts`, the canvas, and the fixtures stay untouched. Selecting a thread changes **only** the
chat transcript and must not drive canvas navigation.

**Why I did not reconcile them.** Collapsing the fixture model into the real one answers a question
I have not decided and did not want decided silently inside a wiring card: **is a "plan" a
conversation, or does a plan merely *have* one?**

- If a plan **is** a conversation, the three canvas levels become a view over conversation depth,
  and `navigation.ts` mostly disappears.
- If a plan **has** a conversation, `Workflow` keeps its own identity and gains a
  `conversationId`, and the canvas is unaffected.

The second is less disruptive; the first is closer to what the engine actually gives us. **This is
the most consequential open design question in CPD right now and it is yours to make.** Keeping the
two trees side by side makes the eventual merge a deliberate, reviewable step rather than a side
effect of a wiring card.

**Cost of this choice:** the sidebar temporarily shows two trees — a real "Threads" section and the
fixture hierarchy. That is intentional and ugly on purpose; it should not survive the decision above.

**Also specified:** conversation ids stay numeric end to end, stringified only at Mantine `Tree`'s
`value` boundary and parsed back on selection, so a stringified id never reaches the API.
---

## D109 — Messages carry their Durable **entry id**; `at` is never a message index

**The gap:** `/api/fork` needs `at`, a numeric *entry* id, but `/api/messages` returned
`{role, content}` with no id. The UI could not name a fork point — so the fork API shipped
unusable from the front end.

**Probed against our own `.cpd/cpd.sqlite`** (not inferred from docs):
`conversation.context(ctx, {})` returns `{head, entries, contributions, messages}`.
`view.messages` — what `flattenMessages` consumed — has **no ids**. The ids live on
**`view.entries`**: `{ id, kind, conversationId, model: [...], byTaskId? }`.

```
id= 7  kind=pi.user   role=user       "Reply with exactly: PROTOTYPE-LIVE"
id=10                 role=system     (instructions)
id=11                 role=assistant  "PROTOTYPE-LIVE"
id=12  kind=pi.user   role=user       "Reply with exactly: UI-ROUNDTRIP-OK"
id=15                 role=assistant  "UI-ROUNDTRIP-OK"
```

**Two traps this makes explicit:**

1. **Entry ids are SPARSE** — 7, 10, 11, 12, 15; the gaps are real. An id is never an array index,
   and `at` must never be computed by counting messages.
2. **One entry can expand to several messages** (its `model` array). So message index ≠ entry
   index, and two messages may legitimately share an id — which means a React `key` must be
   `` `${id}:${index}` ``, not the bare id.

**Chosen:** rebuild `flattenMessages` over `view.entries` and widen `Message` to
`{ id, role, content }` in both the daemon and `src/engine/types.ts`. Filtering is unchanged
(skip `toolResult`, skip empty system, join text parts).

**Rejected:** zipping `view.messages` against `view.entries` by position — it is wrong whenever an
entry yields more than one message, and it fails silently rather than loudly.

Filed as card H2, serialized behind H1 since both edit `ChatPanel`.

---

## D110 — Forking is offered only on **user** messages

A hover `ActionIcon` on user messages starts a plan thread there. Assistant messages get no
affordance: forking mid-turn is a semantic we have not thought through, and offering it would
invite a question we cannot yet answer. Easy to widen later; hard to take back.

The title is derived **server-side** by G1's existing `deriveTitle`. The frontend sends no title,
so there is exactly one implementation of that rule.
---

## D111 — Forks hold real, independent Copilot turns (verified), and `/api/prompt` has a silent misroute

**The good news, proven live.** A forked conversation is not just a tree node — it runs. Submitting
into fork `16` produced a real Copilot turn that landed *only* in that thread:

```
POST /api/prompt {"text":"Reply with exactly: FORK-TURN-OK","conversation":16} -> {"status":"done"}
GET  /api/messages?conversation=16 ->
  user "Reply with exactly: PROTOTYPE-LIVE"      # inherited up to `at`
  user "Reply with exactly: FORK-TURN-OK"
  assistant "FORK-TURN-OK"
```

The inherited history, the new turn, and the reply all live in the child; the root is untouched.
**The core hierarchy CPD is built on — a thread that forks from a message and then carries its own
independent conversation — works end to end.**

**The defect found on the way there.** The daemon is inconsistent about where the conversation id
comes from:

| Route | Source |
|---|---|
| `GET /api/messages` | query string |
| `GET /api/stream` | query string |
| `POST /api/prompt` | **JSON body** |

So `POST /api/prompt?conversation=16` — which matches every sibling route — returns
`200 {"status":"done"}` and writes to the **root**. No 400, no warning. I hit this myself and
briefly believed forks could not accept turns at all.

**Chosen:** accept `?conversation=` on `/api/prompt` (same helper as the other routes), keep the
body form for compatibility, and **400 when the two disagree** rather than silently preferring one.
An unknown or non-numeric explicit id must **404**, not fall back to root.

**Also required:** `sendPrompt(text, conversationId)` takes the id explicitly instead of defaulting
to root internally. The silent default is what made the bug invisible.

Filed as H3, serialized behind H1 and H2 (all three touch the daemon routes).

**Pattern worth naming — this is the third silent-success failure tonight.** The Durable `no_model`
case, the dead `--pad` token, and now this: each returned success while doing nothing or the wrong
thing. Our defaults are too forgiving. Where a parameter means "which thread does the user's work
go into", the correct behaviour is to fail loudly, not to guess the root.
---

## D112 — A plan **is** a conversation. Jobs live in a Durable document scoped to it.

D108 asked whether a plan *is* a conversation or merely *has* one. I probed Pi Durable directly
rather than keep guessing, and the engine answers it for us.

**Probe 1 — a conversation can carry structured job data, and it persists.**

```js
const PlanDoc = defineDoc({
  kind: "cpd.plan", version: 1, scope: "conversation",
  history: "rewindable", fork: "asOf",
  initial: () => ({ jobs: [] }),
});
await conv.commit(async (tx) => {
  const d = await tx.doc(PlanDoc, conv.id);   // ← id required even inside that conversation's commit
  d.jobs.push({ id:"j1", title:"Build thing", status:"queued", needs:[] });
}, ctx);
```

Read back `{jobs:[j1,j2]}` with `needs` edges intact, and **byte-identical after a cold restart** —
new storage handle, new `Harness`. Jobs with dependency edges are already durable without us
designing a schema.

**Probe 2 — the document follows a fork, copy-on-write. This is the decisive one.**

```
child = root.fork(lastEntryId, …)
child doc at creation        -> {jobs:[j1,j2]}   # inherited
child.commit(push j3)
parent after child write     -> [j1, j2]         # untouched
child  after child write     -> [j1, j2, j3]
```

A forked plan inherits the parent's jobs and then diverges. **"Branch this plan and try a different
job graph" is a primitive the engine already provides** — no copying, no diffing, no reconciliation
code of ours.

**Chosen: a plan IS a conversation.** `Workflow` does not get a `conversationId`; it stops being a
separate entity. The three canvas levels become a *view* over conversation depth, and
`navigation.ts`'s fixed three-level string-id hierarchy largely dissolves — exactly the collapse
D108 anticipated. Jobs live in a `cpd.plan` document scoped to that conversation.

**Rejected — a plan *has* a conversation:** we would hand-roll persistence, forking, and history
for the job graph in a second store, kept in sync with the conversation tree by our own code. That
is strictly more machinery to get strictly less than `fork: "asOf"` already gives us. It also
contradicts the standing instruction to prefer what is native to the stack.

**Caveat for implementers:** `tx.doc(Def)` throws `TypeError: Document cpd.plan requires a
conversation ID` unless the owner id is passed explicitly — `tx.doc(Def, conv.id)` — even inside
that conversation's own `commit`. And `documentState()` is refcounted like `viewState()`: always
`dispose()`.

This unblocks the jobs canvas, which was the last thing waiting on a human decision. Per the
standing instruction I took the recommended, most-native option; flagging it here for review.
---

## D113 — Jobs execute as Pi Durable **tasks**; `needs` edges are the engine's native join

Before specifying job execution I probed whether Durable can actually *run* a job, or whether CPD
would have to build a scheduler. It can, and we must not build one.

**Probe 1 — a durable task runs to completion.** A two-phase `cpd.step` task ran, performed a real
side effect, and settled durably:

```
state: { status: 'terminal', outcome: { status: 'ok', value: { ran: 'build', n: 1 } } }
marker: ran:build
```

**Probe 2 — `needs` edges are free.** A `cpd.gate` task spawned two children, parked itself
`waiting` on them, and resumed only once both were terminal:

```
marker:  ran:a / ran:b / gate-resumed
gate:    { status:'terminal', outcome:{ status:'ok', value:{ joined:['a','b'] } } }
```

`TaskState` has a first-class `waiting` status carrying `on: TaskId[]` and a `JoinPolicy` of
`failFast | allSettled`. **That is exactly CPD's `needs` semantics, already implemented, durable
across restarts.** `failFast` ≈ cancel dependents when a dependency fails; `allSettled` ≈ run
everything and report. We get both by naming one.

**Chosen:** a CPD job is a Durable task; `needs` is expressed as a `waiting` state with
`policy: "allSettled"` (report every failure rather than hiding later ones behind the first).
H4's `cpd.plan` document stays the **declarative** graph the user edits; tasks are the
**execution** of it. Document = intent, task = run.

**Rejected:** our own queue/worker/dependency resolver in the daemon. It would duplicate a durable,
crash-safe scheduler we already depend on, and would not survive restarts without us rebuilding
checkpointing too.

**Four API traps, all hit during the probe — builders must not rediscover these:**

1. `registry.add()` **does not exist**. Tasks register through an extension:
   `registry.install({ name: "cpd", tasks: [MyTask] })`.
2. `createRegistry()` is the **tool** registry that also holds tasks; there is no separate task
   registry.
3. A phase handler **returns nothing**. It must call `runtime.commit(() => nextState, ctx)`;
   returning a state object silently makes no durable progress and **faults the task**.
4. The checkpoint is at **`task.state.checkpoint`**, not `task.checkpoint`. Reading the wrong path
   faults the task with a `TypeError` that surfaces only in the stored outcome.
5. `harness.task(id)` does not exist — read a task with `tx.task(id)` inside a commit.

Trap 3 is the dangerous one: it is the same silent-success family as D111, except the task is
recorded `faulted` with a confusing message rather than visibly refusing.
---

## D114 — Jobs run real commands through `NodeExecutionEnv`; output arrives only by callback

D113 established that a job *is* a durable task. The remaining unknown was whether such a task can
do real work — run a build, a test suite, an agent — or only shuffle state. It can.

Pi Durable ships `@earendil-works/pi-durable/env/node` exporting **`NodeExecutionEnv`**, a full
filesystem + shell environment (`exec`, `readTextFile`, `writeTextFile`, `watch`, `absolutePath`,
…). It is supplied once at harness open and reached inside a task via `rt.env(ctx)`:

```js
const h = await Harness.open(storage, {
  models, registry,
  env: async () => new NodeExecutionEnv({ cwd: someWorkdir }),
}, ctx);
```

**Verified** — a `cpd.run` task executed a real command and settled with its output:

```
RESULT: { status:"terminal",
          outcome:{ status:"ok", value:{ exitCode:0, output:"CPD-JOB-RAN\nv26.7.0" } } }
```

**Chosen:** job execution uses `NodeExecutionEnv` rather than our own `child_process` wrapper. It is
the native path, it already handles cwd/env/timeout/abort/output-spill, and it keeps jobs portable
to other environments (the same `ExecutionEnv` interface backs non-Node hosts).

**Rejected:** spawning with `node:child_process` in the daemon. We would reimplement streaming,
timeouts, abort propagation and spill handling, and tie jobs to a Node host forever.

**Two traps, both hit:**

1. **`ShellExecResult` is only `{ exitCode, spillPath? }` — there is no `stdout`/`stderr` field.**
   Output is delivered *exclusively* through the `onOutput(text, ctx, info)` callback as it
   arrives. My first probe returned `exitCode: 0` with an empty string and looked like a command
   that produced nothing; it had produced output I never collected. **A job that does not pass
   `onOutput` silently discards its own logs.**
2. The option is **`timeout`**, not `timeoutMs` (contrast `busyTimeoutMs` elsewhere in the package).
   An unknown option is ignored silently, so the job runs unbounded.

Trap 1 is the fifth silent-success defect of the night, and the most expensive kind: a *green* job
with no evidence it did anything.

**Design consequence for the daemon:** `onOutput` is explicitly "raw, unbounded, unthrottled", and
every durable write costs. Job logs must **not** be committed per chunk. Buffer in memory, persist
a bounded tail (and `spillPath` when present), and stream live output to the UI over the existing
SSE channel rather than through the document.
---

## D115 — The daemon's job shape stays minimal; an adapter fills the UI's richer shape

The UI's `Job` (`src/model/types.ts`) carries `owner`, `profile`, `tier`, `attempt`,
`artifactCount` and a ten-value `JobStatus`. The daemon's job (H4) carries five fields and five
statuses. These must meet somewhere.

**Chosen:** the daemon shape stays minimal and a **pure adapter** (`src/model/fromPlan.ts`,
`planToCpdData`) widens it for the canvas. Checked first: the daemon's five statuses
(`draft|queued|running|done|failed`) are a **strict subset** of the UI's ten, so status passes
through unchanged — the mismatch is only in the surrounding fields.

**Rejected — widening the daemon's job to match the UI.** It would persist `tier`, `attempt` and
`artifactCount` into the durable document before anything computes them, violating "keep the data
set and contracts at minimum" and freezing speculative fields into storage that is expensive to
migrate.

**Rejected — shrinking the UI's `Job`.** Those fields encode real design decisions (colour = owner,
tier badges, attempt counters). Deleting them to match today's daemon would throw away settled
design work to save an adapter of maybe forty lines.

**Rule the adapter must follow: absent data renders as absent, never as a plausible default.**
`tier` stays `undefined`, `artifactCount` is `0`, `attempt` is `1`. A tier badge invented by the
adapter would be indistinguishable on screen from one that was actually computed — the same class
of defect as tonight's five silent successes, but aimed at the user instead of at me.

The adapter also **drops `needs` edges that reference absent jobs**. H4 prevents dangling edges at
write time, but the adapter does not own that payload and ELK mislays or throws on an edge to a
missing node. A partial graph beats a blank canvas.

This keeps `buildGraph` — already pure and tested — completely untouched, and makes the seam between
engine truth and UI presentation a single testable function instead of a rewrite on either side.
---

## D116 — The daemon runs under Node's strip-only TypeScript; emit-requiring syntax is banned

`pnpm run daemon` executes `node --watch --experimental-strip-types daemon/src/index.ts`. Node does
not *compile* TypeScript there — it **erases** it. Any TS construct that must *emit JavaScript* is a
hard `SyntaxError` at startup.

PR #34 (H4) shipped four **parameter properties** (`constructor(public readonly id: number)`), and
the daemon could not boot at all:

```
SyntaxError [ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX]: TypeScript parameter property is not
supported in strip-only mode   —   daemon/src/engine.ts:258
```

**Banned in `daemon/`:** parameter properties, `enum`, `namespace`, decorators, and constructor
parameter modifiers. **Fine:** type annotations, `interface`, `type`, generics, `as`, and
`import type` — everything that vanishes without a trace.

**Chosen:** keep strip-only mode and adapt the code (an explicit field plus an assignment in the
constructor). Adding a build step or `tsx` for the daemon would buy nothing but a compile stage and
a second toolchain; the constraint costs a few lines and keeps `node file.ts` as the whole story —
the most vanilla option available, per the standing preference.

**Why this got through — the gates do not run the daemon.** `pnpm typecheck` (tsc) and Vitest both
*compile* TypeScript, so both accept parameter properties happily. 47 daemon tests passed against
code that could not start. **Our test suite proves the daemon's functions work; it never proved the
daemon runs.** A boot smoke test — spawn the real entrypoint, poll `/api/health`, kill it — is now
required, because it is the only check that exercises the same loader production uses.

This is the sixth silent-success of the night, and the first where *green tests themselves* were the
disguise.
---

## D117 — HTTP fetch for initial state, SSE for updates; one EventSource per conversation

The `/api/stream` handshake pushes `{type:"messages"}` and `{type:"conversations"}` on connect, but
**not** the plan. A client listening only to SSE would see every plan *change* and never the
*initial* plan.

**Chosen:** the client fetches initial state over HTTP and uses SSE purely for subsequent updates.
This is already the shape `useEngineStream` uses for messages, so the plan follows an established
in-house pattern rather than a second one invented beside it.

**Rejected — adding the plan to the SSE handshake.** It would make the stream the source of both
initial and incremental state, meaning every new consumer must be added to the handshake, and the
first paint would be gated on the SSE connection. It also would have meant editing
`daemon/src/index.ts` while H6 owns that file, serialising two cards that are otherwise parallel.

**Hard constraint carried into the card: exactly one `EventSource` per conversation.** H1 built a
refcounted keyed map with proven teardown; plan frames must be exposed from that same module, the
way `onConversationsSignal` already is. A second connection per consumer would duplicate every
frame, double reconnect storms, and leak on unmount — and the daemon's `watch()` serves only one
consumer, so fan-out is the client's job and must happen in exactly one place.

**Scope note:** only the `jobs` level moves to real data. `leads` and `plans` stay on fixtures
because they map to the project/session hierarchy the daemon does not expose yet. Converting them
needs that hierarchy to exist first and is deliberately a separate card.
---

## D118 — The daemon was never typechecked by any gate

Root `tsconfig.json` references `./tsconfig.app.json` and `./tsconfig.node.json`; the latter
includes **only `vite.config.ts`**. `daemon/tsconfig.json` exists and is referenced by **nothing**.

Proven by appending one line to `daemon/src/plans.ts`:

```ts
const __typecheck_probe: number = "definitely a string";
```

`pnpm typecheck` → clean. `pnpm build` → built in 4.91s. Only
`tsc -p daemon/tsconfig.json --noEmit` caught it (`TS2322`).

**The daemon's only real safety net has been Vitest**, which compiles per-file on demand and never
checks the project as a whole. That is the same blind spot that let D116's boot break through
review with 47 tests passing, and it means the shared note in every card — *"`pnpm build` is the
real typecheck gate"* — has been **false for all daemon code**.

**Chosen:** wire `daemon/tsconfig.json` into the root references so `tsc -b` walks it — the vanilla
TypeScript mechanism, one command, no new tooling. Fixed in card H10.

**Rejected — a separate `typecheck:daemon` script.** Two commands mean the one nobody remembers is
the one that rots, which is exactly how this gap was born.

**Standing lesson, now twice-proven: a gate that has never failed has not been shown to work.**
Both times I found a real defect tonight it was by deliberately breaking something and checking the
alarm sounded — the EventSource teardown, the entry-id test, the dangling-`needs` filter, and now
the typecheck itself. Cards that add a gate must demonstrate the gate failing, not just passing.
---

## D119 — Scrubbed the user's name from this file; the public-repo tripwire does not cover prose

The repo is intended to be public. An audit of tracked files found **no secrets and no home paths**,
but `DECISIONS.md` named the user five times ("The user was away", "The user already chose", …).
Replaced with "the user" — no loss of meaning, since the decisions matter and the name does not.

**The existing tripwire could never have caught this.** `src/fixtures/sample.test.ts` walks the
`sampleData` object for `/missionlane|jira|slack|EEC-/i`. It only sees fixture strings, so it
cannot see markdown, source comments, or card text — and its pattern does not include personal
names at all. It guards the one place we were already careful about.

A repo-wide guard is carded (H11): one test that greps **tracked files** for personal names,
employer, work ticket prefixes, home paths and token shapes. Deliberately *not* a git hook — a
plain Vitest test runs in the gate everyone already runs, which is the most vanilla option
available.

**Left alone deliberately, needs the user's call:** commits on `main` carry an author *name*
derived from the user's real name (the email, `cpd@duck.com`, is already a throwaway). The name is
identifying and sits in every commit. Changing it means rewriting published history, which is not a
decision to take unattended — flagged for review.

**Lesson, matching D118:** a guard that has only ever been pointed at the safe place has not been
shown to work. The tripwire passed every run tonight while the leak sat in a file it does not read.
---

## D120 — Privacy allowlist is by (file, pattern-class) pair, never by file

H11 shipped the repo-wide tripwire promised in D119. It scans **tracked files only** (`git ls-files`
— exactly the set that becomes public; `docs/`, `node_modules` and build output are never read).

The builder blocked the card first, correctly: my card said "report matches, don't delete" *and*
"the clean tree must pass", which cannot both hold when the scan finds pre-existing matches. All
four it found were the guard **describing itself** — the old fixture tripwire's regex vocabulary,
quoted in `sample.test.ts` and in this file's prose.

**Decision: exemptions are (file, class) pairs, not file skips.**

    sample.test.ts   -> work-context class only
    DECISIONS.md     -> work-context class only
    repo-privacy*.ts -> all classes (self-reference)

A blanket file skip on `DECISIONS.md` would have re-opened the exact hole D119 closed, since the
real leak (the user's first name) lived in that very file. Verified: appending a personal name to
`DECISIONS.md` still fails the suite, naming file and line.

**`$HOME/` and `~/` never trip the guard.** Only literal absolute macOS/Linux home-directory
prefixes do. Flagging portable paths would teach people to write worse ones.

**The allowlist is itself unit-tested** — there are assertions that `DECISIONS.md` is *not* exempt
for the name, path, credential and email classes. An exemption nobody tested is just a hole.

Proven by deliberate leak, not by a green run: injecting an absolute home path plus a GitHub
token literal into `theme.css` fails the suite with file, line and a redacted match. Matches are reported redacted, so
the guard never reprints a real secret into CI logs.

No hook, no CI file, no new dependency — a plain Vitest test in the gate everyone already runs.
**Addendum (first real catch).** This very entry tripped the new guard on PR #38: documenting the
path patterns meant quoting them. Fixed by **rewording the prose, not by widening the allowlist** —
exempting `DECISIONS.md` for the path class would have reopened the hole D120 exists to close.
Rule: when the guard flags the docs, change the docs. The guard caught a real pattern in its first
PR, which is more than the old fixture tripwire ever did.
---

## D121 — Jobs execute real commands (H6). HTTP parse layer must be tested at the HTTP level

H6 landed job execution: `JobTask` runs a job's command through `NodeExecutionEnv`, honours the
`needs` join (`allSettled`), streams output via `onOutput` buffered in memory, and settles
`done`/`failed`. Covered by real daemon tests — `echo hi`, `exit 3`, a commandless no-op, and a
two-job dependency with marker files. All D114 traps honoured (`timeout` not `timeoutMs`, no commit
per chunk) and no strip-only-illegal syntax (D116).

**But the prototype still cannot run a job from the API.** Verified by live curl against merged
main: `POST /api/plan/job` with a `command` returns `command: null`, and running it reports
`status: "done"` having executed nothing. The route parses `title` and `needs` only — `grep -n
command daemon/src/index.ts` has zero hits. The engine and task layers are correct; the HTTP parse
drops the field.

**Why 55 green daemon tests missed it:** every test calls engine functions directly. There was no
HTTP-level test for any plan-job route, so the parse layer was never exercised. Carded as H12, which
must fix the parse **and** add server-level tests over real `fetch`.

**Decision: a job's terminal status is not evidence that it did anything.** H12's end-to-end test
must assert a **marker file written by the command**, not just `status === "done"`. A commandless
job legitimately settles `done` untouched, so status alone cannot distinguish "ran successfully"
from "ran nothing" — which is precisely how this shipped.

This is the seventh "silent success" tonight (D114's missing stdout, the dead `--pad` token, the
`/api/prompt` misroute, `edit --status`, the un-typechecked daemon, the fixture-only tripwire, now
a dropped request field). The recurring shape: **a layer that is never exercised by a test is a
layer that is not working.** Gates must exercise the seam, not just the unit behind it.
---

## D122 — The prototype runs real work end to end. `undefined` = leave, `null` = clear

H12 closed the D121 gap. `command` now survives `POST` and `PATCH /api/plan/job`.

**Proven end to end against the running daemon, by side effect rather than status:** a job created
over HTTP with `command: "echo CPD-E2E-RAN > /tmp/..."` ran and **the marker file existed with the
expected contents**. This is the milestone the prototype was missing — create a job over HTTP, run
it, and real work happens.

**Decision (patch semantics):** for `command` on `PATCH`, **`undefined` means leave unchanged and
`null` means clear**. The existing merge used the `?? job.x` idiom, which *cannot* express clearing
— `null ?? job.command` silently yields the old value. The builder blocked the card first, correctly:
my card forbade touching `engine.ts` while requiring a change only reachable there (`patchPlanJob`'s
type had no `command`). Ruled to extend it rather than drop the PATCH requirement, because a job
drafted before its command is known would otherwise be permanently commandless.

Validation: non-string or blank-only `command` -> **400**, matching how `needs` already behaves.
Verified live: `123` -> 400, `"   "` -> 400, PATCH `null` clears, a later title-only PATCH leaves
it `null`.

**Decision (testing the seam):** plan-job routes now have **HTTP-level tests** that start the real
server on an ephemeral port and drive it with `fetch` — the layer D121 showed was never exercised.
The end-to-end test asserts the **marker file**, not `status === "done"`.

Proven by sabotage, not by a green run: replacing the POST parse with `command = undefined` fails
*both* the round-trip and the marker-file tests. An earlier sabotage attempt broke the file's syntax
and the suite reported "6 skipped" — **a skipped test is not a passing test, and not evidence**; the
probe was redone surgically.

**Audit:** checked every other route for the same dropped-field shape. `/api/fork` parses and
forwards its optional `title` correctly, so the `command` miss was isolated, not systemic.
---

## D123 — Capability in the daemon is not capability in the product

With D122 the daemon can create, run and execute jobs end to end. **None of it was reachable from
the GUI.** `src/engine/client.ts` exported only read paths (`getMessages`, `getConversations`,
`sendPrompt`, `forkConversation`, `getPlan`) — no `createJob`, no `runJob` — and `JobCard`/`JobNode`
had no interactive elements at all. The canvas could display a plan; a human could not drive one.

This is the same shape as the silent-success pattern, one level up: **a capability nobody can reach
is a capability that does not exist** for the product. CPD is a GUI, so "works via curl" is not the
bar. Carded as H13.

**Decision (optional-prop interactivity):** the run control is added as an **optional** `onRun?`
prop on `JobCard`. Absent the prop, the card renders exactly as today, so the Gallery and existing
component tests keep passing untouched. Rejected making the card always-interactive: the Gallery
renders cards as a static design reference, and a permanent button would change every fixture and
couple presentation to a live daemon. Explicitly required a test asserting **no run button when
`onRun` is absent**, so the no-regression promise is enforced rather than assumed.

**Decision (errors are UI, not console):** a 400 from an invalid command must render where the user
can see it. Chosen inline Mantine `Text c="red"` over adding `@mantine/notifications` — no new
dependency, and the standing rule is to use Mantine components as-is with zero customisation.

Updates continue to arrive over the existing SSE `{type:"plan"}` frame (D117) — subscribe, never
poll, and never open a second EventSource.
---

## D124 — Fixture navigation and real conversations are two disconnected trees

H13 landed the UI controls: a Mantine `Modal` for New job, a run `ActionIcon` behind an **optional**
`onRun` prop, and `createJob`/`runJob`/`deleteJob` clients. Verified in the browser — the modal is
stock Mantine, Create is correctly disabled on a blank title, and a failure renders **visibly** in
the modal rather than vanishing into the console.

**But pressing Create returned `404 Not Found`.** Instrumenting `window.fetch` in the live GUI showed
the canvas requesting **`/api/plan?conversation=w3`** — `w3` is a *fixture workflow id* from
`sampleData`, not a conversation, so the daemon correctly 404s. Clicking a real thread under THREADS
switches the chat but leaves the canvas on the fixture plan.

Root cause in `App.tsx`: `nav.planId` is resolved against `sampleData.workflows`, then passed to
`usePlan` via `as unknown as ConversationId` on the strength of "D112: a plan IS a conversation".
That holds for real conversations and is false for fixture ids. **The error was in my D112
application, not in H13's code** — H13 is correct and merged; it is simply unreachable.

**Decision:** keep both trees and join them at selection time (H14). Selecting a thread points the
canvas at that conversation. `sampleData` continues to drive the project/plan levels and the
Gallery — it is the design reference and deleting it would cost the fixture-only visual tests.
Rejected converting fixtures into fake conversations: that would manufacture plausible-looking data
with no daemon behind it, violating D115.

**Decision (id representation):** treat `nav.planId` as a conversation id **only when it parses as a
positive integer**; otherwise the level is fixture-backed and issues **no request**, rendering an
empty canvas. This removes the `as unknown as` cast and makes the fixture case explicit rather than
an accidental 404.

**Evidence rule restated for H14:** a job reaching `done` is not proof it ran — a commandless job
also reaches `done` (D122). The card requires showing the **marker file contents** from a job
created and run through the GUI.

This is D123 one level deeper: the capability existed, the controls existed, and it still could not
be driven by a human. **Nothing counts until someone can reach it through the product.**
---

## D125 — `needs` is currently a deadlock, not an ordering primitive

I had been repeating that job dependencies work, on the strength of a green test. Tested it through
the real HTTP API instead: job A (`sleep 2 && echo A`), job B (`echo B`, `needs: [A]`), B started
first on purpose.

```
ORDER FILE: 'A'                      <-- B never wrote
statuses:   A=done   B=draft (permanently)
```

B correctly refused to start before A — **and never started after A settled.** The join blocks and
never releases.

**Root cause:** `jobTask.ts` does `needs.map((id) => id as unknown as TaskId)`. `needs` holds **job
ids** (plan-document UUIDs); Pi Durable's `on:` expects **task ids**. The task waits on ids that
never exist. `runPlanJob` passes `job.needs` straight through, so nothing bridges the two id spaces.
**The `as unknown as` cast is precisely what stopped the compiler from catching it.**

**Why the green test missed it:** `jobTask.test.ts` passes *real task ids* into `needs`
(`needs: [taskIdA, taskIdB] as unknown as string[]`). It tests Pi Durable's join — which works —
and bypasses the job-id -> task-id translation the real path needs and lacks. **A test that
pre-substitutes the value under test cannot catch a bug in producing that value.**

**Decisions for the fix (H15):**
- `needs` stays **job ids** in the plan document and the API — that is the user-facing contract and
  H4/H12 depend on it. Translation happens at task creation.
- An unstarted dependency must **not** be treated as satisfied; waiting on nothing would make the
  dependent run immediately, which is worse than the deadlock. Chosen: **auto-start unstarted
  dependencies depth-first**, then wait on their real task ids, so pressing Run on a leaf job runs
  its prerequisites then itself — the behaviour a user expects from a dependency graph.
- Both `as unknown as` casts (source and test) must be deleted. Distinct id spaces belong in the
  types, not papered over with casts.

**Lesson, now third of its kind (D118, D121, D125): a cast is a silenced error.** Every silent
failure tonight was a place where something could not fail loudly — an ignored field, an
un-typechecked directory, a test asserting status instead of effect, and now a cast across two id
spaces.
---

## D126 — Dependencies work; "Run" on a job auto-starts its prerequisites

H15 fixed the D125 deadlock. `runPlanJob` now resolves each needed **job id** to a real **task id**
before creating the waiting task, depth-first, auto-starting any dependency that was never started.
Proven through the live HTTP API: creating A (`sleep 2 && echo A`) and B (`echo B`, `needs: [A]`)
and running **only B** produced an order file containing `A` then `B`, both `done`. Previously B sat
at `draft` forever.

**Decision confirmed in implementation:** pressing Run on a leaf job runs its prerequisites first.
`needs` stays job ids in the document and API; translation happens at task creation.
`JobDependencyCycleError` guards cycles so a cyclic graph fails loudly instead of hanging the
daemon.

**On the remaining `as unknown as TaskId` casts:** two survive, and they are *not* the D125 defect.
They convert a **stored `taskId` string** back into the branded `TaskId` type — same id space,
string-to-brand. The bug was casting a **job id** into a `TaskId`, bridging two *different* id
spaces. The auto-start path uses the real `depTaskId` with no cast at all. Accepted as-is;
tightening the brand round-trip is cosmetic and not worth churn tonight.

**Sabotage lesson — I nearly cleared this on a false negative.** My first probe reintroduced the bug
on the `needJob.taskId` branch and all 9 tests still passed. That branch is only reached when a
dependency is *already started*; every test exercises the auto-start path, so I had sabotaged code
the tests never run. Re-aimed at the live path (`taskIds.push(depTaskId)`) and **3 of 9 tests
failed**.

Rule: **when sabotage does not fail a test, first prove the sabotage was reachable.** A passing
suite under sabotage means one of two very different things — the tests are weak, or the mutated
line is dead on that path — and they demand opposite responses. Checking which cost one extra run
and prevented me from either merging blind or wrongly blaming the builder's tests.
---

## D127 — The prototype is drivable end-to-end from the GUI

H14 (PR #42) connected thread selection to the canvas: `nav.planId` is treated as a conversation id
only via explicit `parseConversationId` (positive integer), and the `as unknown as ConversationId`
cast is gone. Fixture plans no longer issue requests that 404.

**Proven by driving the real browser, not by curl.** Clicked a thread -> canvas loaded
`/api/plan?conversation=16`; clicked **New job**, filled the Mantine modal, clicked **Create** (job
appeared on the canvas, no 404); clicked **Run job** on the node. Result:

```
MARKER: 'CPD-GUI-RAN'
jobs:   [('UI Smoke','done',...), ('GUI proof','done','echo CPD-GUI-RAN > ...')]
```

A human can now create and execute real work entirely through the UI. This closes the D123 gap
("capability in the daemon is not capability in the product"). **The prototype milestone is met.**

**Process note — I almost reported a crash that was my own fault.** Mid-verification the page went
completely blank: empty body, zero buttons. That looked like a severe product regression in the PR I
was reviewing. It was my own instrumentation: I had monkey-patched `window.fetch` to log request
URLs, and my wrapper broke the app. A clean reload rendered perfectly.

Rule: **before blaming the product, remove your own instrumentation and retest.** A debugging probe
is itself untested code running in production context. I verified the GUI flow again with no
patching, which is also the only honest way to claim "a human can do this" — the patched run was not
the shipping product.

**Remaining known gaps** (none block using the prototype): new-conversation detection is a 1s poll
(D106); `watch()` serves a single consumer; compaction triggers late; no layout-regression tests; no
ESLint; `docs/engine-plan.md` is stale; commit authorship still carries the user's real name
(D119) — rewriting published history is the user's call.
---

## D128 — Crash resumption works; two false results on the way to proving it

Jobs survive losing the daemon. Verified: started `sleep 25 && echo SURVIVED3`, `kill -9`ed the
daemon 5s in, **also killed the orphaned child shell**, confirmed the marker was empty and no
process could finish the work. Restart -> task `running` -> `done`, marker `SURVIVED3`. This is
pi-durable's central promise and it holds. H16 cards the regression test, since **nothing guarded
it**.

Getting there produced two wrong answers, both worth recording.

**False positive #1 — the orphan finishes the work for you.** My first test killed only the daemon.
The marker appeared and I nearly recorded "durability proven". But `sleep 18 && echo ...` had been
spawned as a **child shell**, which outlived the daemon and wrote the marker on its own. The side
effect would have appeared with no resumption whatsoever. A durability test must kill **the whole
work-performing tree**, or it proves nothing. Side-effect proof (D122) is necessary, not sufficient:
you must also establish that nothing *else* could have produced the effect.

**False negative #2 — thirteen stale daemons.** The corrected run reported `failed` with no retry,
which looked like a real durability defect. It was not. `pkill -f 'cpd/daemon'` never matched the
actual processes (`node --watch --experimental-strip-types daemon/src/index.ts`), so **13 daemons
from earlier tonight were still alive**. My "restarted" daemon hit the sqlite lock and exited, while
`/api/health` answered cheerfully **from a different, older process**. I was reading one daemon's
health and another daemon's database.

Fixed by matching on `daemon/src/index.ts`, killing with `-9`, and running the test daemon **without
`--watch`** so the process tree is exactly one process.

**Decisions:**
- Test and debug daemons run **without `--watch`**. The watch supervisor survives inner-process
  death and turns a crash into a silent zombie.
- `GET /api/health` must report the daemon's **pid and sqlite path** (H16). A health endpoint that
  cannot tell you *which* daemon answered is a liveness check that actively misleads during
  restarts.

**Lesson, and the strongest one tonight: a green signal is only as trustworthy as your confidence
about who produced it.** Three times now — the sabotage that hit dead code (D126), the orphan shell,
the stale daemon — the output was real and my attribution was wrong. The discipline that caught all
three is the same: before believing a result, prove the thing you think produced it was actually
the thing that did.
---

## D129 — Resumption is now guarded; `/api/health` identifies which daemon answered

PR #43 (H16) landed `daemon/src/durability.integration.test.ts` plus the stale-daemon fixes.
Main: 246 tests (180 app + 66 daemon), typecheck clean, build green.

**Verifying the test took three attempts, and the first two verdicts were both wrong.**

My card demanded the test fail when the second harness does not `resume()`. **It passed without
it.** By my own D126 rule I did not accept that at face value, and I did not blame the builder
either — a passing suite under sabotage means either weak tests *or* an unreachable mutation.

Discriminator: delete the second harness entirely and just wait 4s after the crash.
`ORPHAN_CHECK marker_exists=false` — the marker does **not** appear, and the test fails. So no
orphaned `sleep 1` shell is completing the work (the D128 trap is genuinely absent), and the job
only finishes once a second harness opens over the same sqlite file. Pointing that second harness at
a *different* sqlite file also fails the test. **The test does prove resumption.**

Conclusion: `waitForTask()` resumes implicitly, so the explicit `second.harness.resume()` is
redundant — removing it does not disable resumption. My card's suggested falsification targeted a
no-op line. The *test* was right and my *instruction for checking it* was wrong. Left as-is:
`resume()` is harmless, matches the documented recipe, and mirrors the daemon's real boot path.

**Stale-daemon fixes verified live:**
```
health: {"ok":true,"conversationId":1,"pid":81974,"db":".cpd/cpd.sqlite"}   (pid matches the live daemon)
second: cpd-daemon: another daemon (pid 81974) already owns .cpd/cpd.sqlite. This process
        (pid 82112) is exiting without starting -- the port may still be served by pid 81974.
```
That is precisely the trap that produced D128's false negative, now self-announcing. No auto-kill
or lock-stealing was added.

**Lesson: when a falsification probe comes back clean, the probe is a suspect too.** Three times
tonight the mutation was the problem, not the code: dead-code sabotage (D126), an orphan shell
(D128), and now a redundant line (D129). "Make the test fail" is only evidence once you have
confirmed the thing you disabled was load-bearing.
---

## D130 — Reaped worktrees for merged PRs only; 15 unmerged branches left for the user

A batch of replayed kanban notifications (H4-H16, all already reviewed and merged) included
`t_56d7b396 gave up after repeated spawn failures: fatal: 'wt/h4-plan-doc' is already used by
worktree`. Board and PR list were both empty, so nothing was outstanding — but that error pointed at
real debris: **23 worktrees from earlier cards were still checked out**, and a branch held by a
worktree cannot be reused by a new dispatch.

**I did not trust my first classifier.** `git merge-base --is-ancestor <branch> origin/main` said
**NO for all 23** — which, taken at face value, means "nothing is merged and nothing is safe to
delete". That is wrong: every PR here is **squash-merged**, so the branch tip is never an ancestor of
main by construction. An ancestor test is the wrong instrument for a squash-merge workflow and would
have had me either delete nothing or, with a sloppier reading, delete everything.

Authoritative signal instead: `gh pr list --state all --json headRefName,state`.

- **8 branches with state `MERGED`** -> worktree removed, branch deleted. Their content is provably
  in main.
- **15 branches with `CLOSED` or `NO_PR`** -> **left completely alone.** A closed PR or a branch that
  never had one may hold work that never reached main, and `git branch -D` is unrecoverable once the
  worktree is gone. Deleting those is the user's call, not an unattended agent's.

Gates after cleanup: typecheck clean, 246 tests (180 app + 66 daemon), main unchanged at `dcce4d9`.

**Decision:** worktree reaping is keyed on **PR merge state**, never on commit ancestry, and never on
"the card is done" (a card can be done while its PR was closed in favour of another). Destructive
cleanup is opt-in per branch with positive evidence the work survives elsewhere.
---

## D131 — I declared a prototype "working" that the user found unusable

The user opened the app and reported: only stubs visible, clicking them loads nothing, no way to
start a session, no composer, layout not as defined, padding 0 everywhere, no workflow reachable.
**Every point reproduced.** D127 ("the prototype is drivable end-to-end") was wrong as a statement
about the product.

**What I actually verified vs. what I claimed.** My proof path was: seed `conversation=1` by hand
with curl, click a Thread, create a job, run it, check the marker file. That path works. But I had
*manufactured its starting conditions myself*, then generalised to "a human can drive this". A user
does not arrive with a hand-seeded conversation.

**The three real defects:**

1. **The app renders fixtures, not real data.** `App.tsx` drives all navigation from `sampleData`.
   Fixture ids (`w1`,`w2`,`w3`) are not conversations -> `/api/plan?conversation=w3` -> 404 ->
   permanently empty canvas. Real conversations are relegated to a separate "THREADS" list.
   **H14 made fixture ids stop requesting instead of making navigation reach real data** — I fixed
   the symptom (the 404 noise) and reported the disease cured.
2. **No session can be created.** The daemon has `/api/fork` (needs a parent) and **no route to
   create a conversation from nothing**. Every conversation in the DB was made by me via scripts.
   A new user hits a dead end. I never noticed because I never started from empty.
3. **No `AppShell`.** `grep -rn AppShell src/` -> zero hits; layout is hand-rolled
   `<div style={{display:'flex'}}>`. That is why padding is 0, and it violates the standing "stock
   Mantine, zero customization" rule.

**Why the 246 tests missed all of it:** they test fixture-driven units and daemon internals. Not one
exercises "cold start -> create a session -> reach a workflow". **A suite that never starts from the
user's starting state cannot tell you the product works.**

**User's decisions (asked, not assumed):**
- Fixtures are **deleted from the running app**; the tree shows only real daemon conversations;
  empty DB -> empty tree + "New session". Fixtures stay on disk for `/gallery` and ~12 unit tests.
- Layout: **full AppShell rebuild** (Header/Navbar/Main/Aside), stock spacing, **composer present at
  every level**.

Cards: **I1** (daemon: `POST /api/conversation`, empty plan returns 200 not 404) and **I2**
(frontend: real data + AppShell + New session), dispatched in parallel — disjoint paths
(`daemon/` vs `src/`).

**Lesson, and it supersedes D123's wording: verifying a path you prepared yourself proves only that
the path exists, not that a user can find or reach it.** The correct acceptance test for a product
is always *cold start to outcome*, with no hand-seeded state. I will not call a prototype usable
again without running that path from an empty database.
---

## D132 — The layout moves from AppShell columns to a full-bleed canvas with floating panels

The user supplied a wireframe of the intended final layout. It **supersedes the AppShell direction
agreed earlier the same day** (I2). Confirmed with the user before acting — not assumed.

**The structural difference:** `AppShell` *reserves* space — Navbar/Aside are columns and `Main` is
what's left over. The wireframe has **"Canvas fills the screen"** with the tree, attention queue,
transcript, composer and action bar **floating above it** as panels. Those are mutually exclusive
models, so AppShell is now the wrong primitive. Panels become Mantine `Paper` positioned over a
full-bleed canvas.

This is not a reversal of the *reason* for I2 — fixtures out, real data, stock components, real
spacing all stand. Only the container changes. **I2 was not wasted:** it deleted fixture navigation
and made the tree real, which this layout needs regardless.

**Ruled by the user (`clarify`, 4 questions):**
1. **Tree click on a Job** -> stay on that job's parent workflow and **select/highlight** the node.
   The tree navigates; the canvas follows context. *(Not a zoom, not a detail view.)* This also ends
   the drill-down model: session/workflow/job are **one continuous tree**, not separate screens.
2. **The red stack with the `12` badge** is an **attention queue** — everything needing the user
   (blocked jobs, failures, questions); `12` is an **unread count**. Not "count of blocked".
3. **The three FABs** are **New session / New workflow / New job**.
4. **Direction confirmed:** full-bleed canvas, floating panels.

**Sequencing (deliberate):** `I3` (selection-survives-reload) is **in flight and owns `src/App.tsx`**
— the exact file a layout rewrite guts. Dispatching the rewrite now would guarantee a conflict, so
the frontend rewrite waits for I3 to merge. **J1 was dispatched in parallel instead because it is
daemon-only** and the layout cannot be built without it.

**J1's finding — a contract split nobody noticed:** the frontend already has a `blocked` status
(`derive.test.ts` asserts red + `hand-stop`), but `daemon/src/plans.ts` `JobStatus` is
`draft|queued|running|done|failed` — **there is no `blocked` in the daemon.** The attention queue,
the headline feature of the wireframe, had **no data source**. Only the frontend half was tested,
which is why it read as working. J1 adds `blocked` + `blockedReason`, makes blocked jobs refuse to
run (409), stops a blocked prerequisite from silently deadlocking dependents (the D125 failure mode),
adds `GET /api/attention`, and pushes an `attention` SSE frame.

**Lesson: a type that exists on one side of an API and not the other is a bug the tests actively
hide — each side tests its own half and both stay green.**
---

## D133 — Layout proportions fixed at 20/40/40; FABs top-right

Second wireframe from the user, annotated with measured proportions. Refines D132:

- **20% left** — Tree panel (top) + attention queue (bottom).
- **40% centre** — canvas band + Action bar (`Comment · Stop · Skip`) at the bottom, tethered to the
  selected node by a dashed line.
- **40% right** — transcript ("grows up, then scrolls") above the Composer.
- **FABs moved to top-right** (they were top-left in the first sketch). The second wireframe wins.

The percentages locate the **panels**, not the canvas: the canvas stays full-bleed *behind* them.

**Sequencing that made this dispatchable:** `I3` merged first (it owned `src/App.tsx`), freeing the
file for the layout rewrite. I3 verified live: selecting a conversation sets `#/c/16`, reload keeps
the same conversation and its messages, and `#/c/abc`, `#/c/999999`, `#/gallery` all degrade to the
empty state with **zero** `/api/plan` or `/api/messages` requests and no crash.

**J2 is deliberately the shell only.** The attention queue and action bar render as placeholders
marked `TODO(J3)` because their data source — `blocked` status and `GET /api/attention` — is still
being built in J1. Wiring a panel to an API that does not exist yet is how the fixture problem
(D131) happened: a component that looks right against invented data and dies against real data.
**Build the container against real components; leave the unbacked panels visibly stubbed rather than
plausibly fake.**

Noted for J2: `data-testid="canvas-area"` currently sits on `AppShell.Main`, and existing tests
query `.mantine-AppShell-*` classes that will vanish. Flagged in the card so the builder updates the
queries instead of keeping AppShell alive just to satisfy a selector.
---

## D134 — The wireframe was drawn in Mantine; every panel maps to an installed component

User: *"mantine has all native components we will ever need, this wireframe was designed with
mantine in mind."* I had written J2 describing the layout in terms of CSS positioning — correct in
outcome, wrong in means. Caught before the builder wrote any code (worktree was still clean) and
corrected on the card.

**Verified against `ls node_modules/@mantine/core/lib/components/`** — the installed listing is the
authority, not memory and not the docs site. We are on **Mantine 9**, whose additions cover this
wireframe almost exactly:

| Wireframe element | Component | Note |
|---|---|---|
| Action bar (`Comment · Stop · Skip`) | **`ActionBar`** | `extends BoxProps, AffixBaseProps, PaperBaseProps` — a floating Affix-positioned Paper. `opened` is **required**. Has `ActionBar.Divider` / `ActionBar.CloseButton`. |
| FABs, top-right | **`Affix`** | `position={{top,right}}`; portals by default, escaping the canvas stacking context. |
| `12` badge on the stack | **`Indicator`** | `label={12}`, with `position`/`offset`/`size`. |
| Transcript "grows up, then scrolls" | **`ScrollArea`** | flex-column + `justify-content:flex-end`, pinned to bottom. |
| Panels | **`Paper`** | plus `Card`, `Stack`, `Group`, `EmptyState`, `Alert`. |

**The trap I avoided by reading the `.d.ts` instead of guessing from the name:** `Scroller` sounds
like the transcript container and is **not** — `Scroller.d.ts` exposes `scrollAmount`,
`startControlIcon`, `edgeGradientColor`: it is a *horizontal control-button scroller for toolbars*.
The transcript must use `ScrollArea`. A plausible name is not an API.

**Deliberately NOT used although installed:** `Splitter` and `FloatingWindow`. The user specified
**fixed** 20/40/40 proportions; those components offer user-resizable and draggable windows, which
is a different product behaviour nobody asked for. *Available is not a reason to use it.*

**Standing rule (already rule zero in the `mantine-ui` skill, now binding on every CPD card):**
before building any component, list the shipped inventory. We previously hand-built ~175 lines of
tree expand/collapse logic that `Tree` + `useTree` already shipped, then paid again to delete it. A
card that describes UI in raw-CSS terms invites exactly that waste — **cards must name the component,
not the pixels.**
---

## D135 — review.sh was mine and undocumented; ports are env vars now

User: *"wtf is review.sh? set ports as env vars"*. Both halves were fair.

`review.sh` was added by **me** in `8a733e5` ("Phase B: the canvas stops lying") and **never
documented or mentioned** — not in the README, not to the user. They discovered it only because I
pointed them at port 4000. **A tool I invent for my own workflow and leave undocumented in a shared
repo is indistinguishable from cruft.**

It had two real defects beyond being a surprise:
1. `PORT=4000` hardcoded — the only baked-in port literal in the repo. Everything else already used
   env vars (`CPD_DAEMON_PORT`, `CPD_DAEMON_URL`).
2. **It started Vite only.** No daemon, so it served an app with nothing behind `/api`: empty tree,
   looks broken. This is what the user actually hit.

**J4 (merged, `c504b5e`):** `CPD_DAEMON_PORT` (4317), `CPD_REVIEW_PORT` (4000), `CPD_DEV_PORT`
(5173), and `CPD_DAEMON_URL` **derived from** `CPD_DAEMON_PORT` rather than repeating the literal —
so changing the daemon port cannot desync the proxy. `review.sh` now starts both processes, polls
`/api/health`, and **exits non-zero rather than serving a backend-less app**. Documented in README.
Verified live on non-default ports (4100/4400) with data flowing through the proxy.

**J5 (merged, `d4a9dbf`) — the asymmetry defect.** J4's teardown killed the daemon reliably but
**left Vite running after Ctrl-C**, leaving exactly the dataless app on the review port that J4
existed to prevent. Cause: `pnpm dev &` captures *pnpm's wrapper*; the real `vite/bin/vite.js` is a
**grandchild** that escapes the process-group kill. The daemon only survived because J4 added an
explicit `pkill` fallback — and no equivalent existed for Vite. **The bug was in the half the author
didn't think about twice: knowing a hazard and fixing it on one side only is worse than not knowing,
because the surviving case looks deliberate.** Fix is port-scoped (`pkill -f "vite --port $PORT"`,
never bare `pkill vite`) plus an `lsof` sweep. Verified: after SIGINT, 0 daemons, 0 vite, both ports
free.

**Two false alarms I raised against myself during this review, both my own environment:**
- "Conversations are gone" — the daemon was simply dead (I had `pkill`ed it); the DB had *more*
  conversations than any backup. **I nearly restored a stale backup over good data.**
- "Teardown is broken, 2 orphans survived" — my `pgrep -f 'review.sh'` matched my own test harness,
  so I signalled the wrong PID. Signalling the real PID showed clean teardown. The *actual* Vite bug
  was found only after that correction. **Confirm process state with `lsof -nP -iTCP:<port>`, not by
  trusting that a kill landed.**
