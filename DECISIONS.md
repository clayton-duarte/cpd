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
