# Decisions made unattended (2026-10-07 night)

Clayton was away. Standing instruction: *"if new decisions are required, opt for the recommended
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

Clayton already chose "daemon, with pi embedded" in an earlier clarify. D86 only fixes *how* it is
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

**Why:** Clayton's existing decision ("lets use PI, but keep the IPC ACP-shaped for easy swap").
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

**Why:** Clayton prefers Copilot as provider; the token already exists, is unexpired, and lists 34
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

**Why:** Clayton's explicit minimum bar: "lead chat working inside CPD, persistent across
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
