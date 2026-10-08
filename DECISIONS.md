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
