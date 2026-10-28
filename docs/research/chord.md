# Chord: what it is, how pi-durable uses it, and what it means for cpd

Plan `chord-research`, Jobs `chord-facts` (facts) and the verdict Job (written 2026-10-28 14:58 EDT). Facts first checked 14:54 EDT. Inspected: npm `@earendil-works/chord@1.4.0` and
git tag `v1.4.0` of `earendil-works/pi`, commit `4a080252f06caf63a525d7873c5d94d3c23887e0`, directory `packages/chord`. `main` was
read only for what differs (commit `33385f9572303e79bfb3811cc92cc8dd65e22261`, committed 2026-10-28 14:53 EDT). The verdict and the comparison with cpd
are the first and the later sections; the middle sections are the facts they rest on. The sibling file
`docs/research/pi-durable.md` already covers pi-durable itself; it is cited here, not repeated.

Labels: a statement is verified when I read the cited source or ran the cited command. Anything else says `unverified`,
`guess:` or `I infer` in the same sentence.

## Verdict

**Chord gives cpd nothing to adopt directly; it is pi-durable's lower layer, and cpd should touch it only as far as pi-durable's API forces.** Evidence: `chord@1.4.0` is an object/contract/replicated-state runtime with no durable tasks, workflows, scheduler or retry (source below), and cpd's Rust store, Events and engine already own those jobs.
- Inside the pi-durable Worker runtime (pro-d164): **partly**. The runner needs only `BACKGROUND_CONTEXT` and `withAbortSignal` from `@earendil-works/chord/context` to call pi-durable; crash resume is pi-durable's storage, not chord.
- Multi-step tool/MCP helpers (`pi/lib/slack-post.ts`, `jira-transition.ts`): **no**. Chord has no durable steps or retries; the queue lives in cpd's store.
- The adapter as a Chord plugin or host (`pi/`): **no**. pi's coding agent does not import chord, and the wire protocol has only a TypeScript implementation.
- Outside pi-durable (Rust store, engine, TUI, metrics): **no**. A Rust client would need a hand-written port of a protocol specified only in prose and TypeScript source, with a version bump per breaking change.
- Telemetry (`./telemetry-sqlite`): **no** for `core/src/metrics`; unneeded by the planned runner.
- Cost: Node >=22.19 against cpd's `>=22.18`, exact-pinned coupling to pi-durable, a release every ~2.5 days with breaking minors.
- **One next step:** when Plan `pi-durable-worker` starts, pin `@earendil-works/chord` to pi-durable's exact version, import only `./context`, and add no other chord use (Recommendations 1 to 3).

## What chord is

**Answer.** `@earendil-works/chord` 1.4.0 is a typed, in-process-and-over-the-wire *object runtime*, not a task or workflow
engine. Applications are built from **objects** (state plus code at an address, created on demand, kept alive by references) that
live on **hosts** (a server, worker processes, browser tabs), are described by TypeBox **contracts**, hold **observable JSON
state** replicated to remote viewers as snapshot plus deltas, talk JSON frames over **links** (memory, Node IPC, WebSocket),
and can be **hot reloaded** while they run (`packages/chord/README.md`, `docs/guide.md`, `docs/spec.md` at `v1.4.0`).
Besides that runtime it ships small shared libraries that other packages use on their own: an explicit `Context`
(cancellation, telemetry, typed values), a JSON delta tracker, a standalone `replicatedState`, bounded output buffers, and
portable SQLite interfaces with Node and Cloudflare drivers.

**The guesses in the Plan's brief are false for 1.4.0.** Verified by reading `src` and `docs` at `v1.4.0`:

| Guess in the brief | What the source shows |
|---|---|
| "Chord provides durable custom tasks" | False. `self.task(fn)` (`src/object.ts:1897`, type at `:193`) is long-running background work of a live object: aborted via `ctx.signal` on reload or disposal, restarted with new code after a reload (`docs/guide.md` "Declarations" table). It is held in memory only; nothing stores it. |
| "child workflows" | False. No workflow, step or child-run concept in `src` (searched `workflow`, `checkpoint`, `schedul`, `retry` in `src/**/*.ts`; matches are unrelated, see "Task, job, workflow, schedule, retry, checkpoint?" below). |
| "scheduling" | False. The only `schedule` hits are `#scheduleAck` in `src/connection.ts:612` (flow-control acks) and an output commit scheduler in `src/output.ts`. Chord has no timer-driven job scheduler. |
| "coordination" | Partly true in a narrow sense: hosts coordinate **hot reload** across processes (`src/cluster.ts`: prepare, vote, commit, abort, driven by the server). That is code rollout, not work coordination. |

**Who owns durable execution and persistence (verified for 1.4.0).** `@earendil-works/pi-durable` owns durable execution: its
task states (`pending`, `running`, `waiting`, `sleeping`, `completing`, `terminal`), scheduler, replay policy and restart limit
live in `packages/durable/src/types.ts` (task state union around lines 599-640), `src/tasks.ts` (`defineTask`),
`src/harness/scheduler.ts` and `src/harness/agent.ts:57` (`DEFAULT_RESTART_LIMIT = 5`). pi-durable also owns the storage
schema and every storage backend (`packages/durable/src/storage/*`; tables `conversations`, `entries`, `tasks`, `submissions`,
`documents`, `document_revisions` in `src/storage/sqlite/schema.ts`). Chord owns no durable state of its own at 1.4.0: its
SQLite module is a generic driver plus a migration runner, and its only tables are for telemetry (see below).

**Chord v2 on `main` is different and not released (verified from `main`, see "Main differs from v1.4.0").** `main` contains an
experimental rewrite in `packages/chord/src/next` with a `jobs` module (intent, run, finish) and a persisted `store`, but the
package `exports` on `main` are the same nine subpaths and the `CHANGELOG.md` says it is "experimental and not exported yet".
Jobs on `main` are in memory only (`persist` is refused in `src/next/jobs/definition.ts:74`). So nothing in v1.4.0 can run
a durable job; the brief's guess is a description of where chord v2 is heading, not of what is published.

### Package metadata and runtime assumptions

Source: `npm view @earendil-works/chord@1.4.0 --json` and `packages/chord/package.json` at `v1.4.0`.

| Field | Value |
|---|---|
| Description | "Typed host runtime for plugins, contracts, replicated state, and RPC over JSON links" |
| Keywords | application, plugins, rpc, contracts, state |
| License | MIT |
| Node | `engines.node >=22.19.0` |
| Module format | ESM only (`"type": "module"`, every export condition is `import`; no `require`) |
| Dependencies | `typebox` 1.3.27, `@earendil-works/pi-telemetry` 1.4.0. No other runtime dependency (WebSocket support is structural: chord does not depend on `ws`, `src/websocket.ts`) |
| Repository | `earendil-works/pi`, `repository.directory` `packages/chord` |
| Side effects | `"sideEffects": false` (`package.json`) |
| Browser / Workers | Core, `./context`, `./delta`, `./output` are documented as "Node and browsers"; `./sqlite/cloudflare` for Cloudflare Durable Objects; `./node` and `./sqlite/node` are Node only (README "Entry points" table) |
| Size | `npm view` `dist.fileCount` 163, `dist.unpackedSize` 1990548 bytes. `src` at `v1.4.0` is 14612 lines in 39 non-test modules (`wc -l`, files directly in `src/` and one directory down); 12482 lines of tests in `test/*.ts` (top level only) |
| Tarball contents | `package.json`, `README.md`, `CHANGELOG.md`, `docs/{guide,spec,tutorial}.md`, `src/delta/README.md` and `dist/` (`npm pack @earendil-works/chord@1.4.0`, `tar tzf`: 39 `.js` files in `dist`). `src/` is not shipped except the delta README |
| Provenance | `npm view` `dist.attestations.provenance` is SLSA v1 |
| Stability | pi-durable's README calls itself "Experimental"; chord's own README has no such banner (I read both). Chord had breaking changes in minors: 1.2.0 replaced the whole facet/service runtime, 1.3.0 renamed terms and raised `PROTOCOL_VERSION` to 4, 1.4.0 made `Context.telemetry` mandatory (`CHANGELOG.md`). |

**Release history and cadence** (`npm view @earendil-works/chord time --json`, converted to EDT). 19 versions in 47 days; `latest` is
1.4.0; dist-tags `initial` 0.0.0 and `latest` 1.4.0.

| Version | Published (EDT) | Version | Published (EDT) |
|---|---|---|---|
| 0.0.0 | 2026-09-04 05:48 | 1.0.0 | 2026-10-01 15:12 |
| 0.85.0 | 2026-09-04 06:09 | 1.0.1 | 2026-10-03 08:30 |
| 0.85.1 | 2026-09-05 08:03 | 1.0.2 | 2026-10-03 20:53 |
| 0.86.0 | 2026-09-19 19:09 | 1.0.3 | 2026-10-05 04:31 |
| 0.86.1 | 2026-09-20 07:13 | 1.0.4 | 2026-10-05 17:44 |
| 0.87.0 | 2026-09-21 12:41 | 1.1.0 | 2026-10-07 18:10 |
| 0.87.1 | 2026-09-22 15:38 | 1.2.0 | 2026-10-12 19:00 |
| 0.99.0 | 2026-09-29 13:16 | 1.3.0 | 2026-10-15 15:10 |
| 0.99.1 | 2026-09-29 14:18 | 1.4.0 | 2026-10-21 18:44 |
| 0.99.2 | 2026-09-30 15:25 | | |

Reading: a release every few days, each minor from 1.2.0 on carrying breaking changes (verified in `CHANGELOG.md`).

### Exports

Every `exports` subpath of 1.4.0 (`npm view @earendil-works/chord@1.4.0 exports`; identical to `package.json` at `v1.4.0`, plus
`./package.json`). Each subpath has `types`, `import` (dist) and `source` conditions.

| Subpath | Source file | What it provides |
|---|---|---|
| `.` | `src/index.ts` | Contracts (`local`, `remote`, `method`, `opaque`, `hostNamed`, `hostFamily`, `bind`, `coverage`), `createHost`, `defineFacet`, `defineRegistry`, links (`memoryLinkPair`, `ipcLink`, `webSocketLink`, `acceptWebSockets`, `inflateMessage`, `PACK_THRESHOLD`), `ChordError` and helpers (`forbidden`, `invalid`, `isChordError`), `PROTOCOL_VERSION`, `replicatedState` and its types, `copyJson`, `isJsonValue`, and the types `Context`, `ContextKey`, `JsonValue`, `Draft`. |
| `./context` | `src/context/index.ts` | The explicit `Context`: `BACKGROUND_CONTEXT`, `TODO_CONTEXT`, `createContextKey`, `withContextValue`, `withAbortSignal`, `withoutAbortSignal`, `withCancel`, `withTelemetry`, `traceIn`, `awaitWithContext`. |
| `./output` | `src/output.ts` | `OutputBuffer` (head or tail window of a stream), `boundOutput`, `sanitizeOutput`, `characterEnd`, `utf8ByteLength`, `OutputLimits`, `OutputSkip`, and `Progress` (paced commits of a changing value, optional `ProgressClock`). |
| `./delta` | `src/delta/index.ts` | The transactional JSON tracker (`track`, `Change`, `Prepared`), `diffRevisions`, `apply`, `applyImmutable`, `applyImmutableBatches`, the op types (`Op`, `WireOp`), `overlap`, op validation, and the wire `encoder`/`decoder` (`src/delta/README.md` is the contract guide). |
| `./node` | `src/node.ts` | Node only: `nodeLoader` (facet module loader), `createHostManager`, `forkDriver`, `childControl`, `processLock` (child host processes). |
| `./telemetry-sqlite` | `src/telemetry-sqlite/index.ts` | `createSqliteTelemetry(db, { retentionMs?, maxQueuedSpans? })`: buffered span and metric recording into SQLite, plus a `TelemetryStore` query implementation. |
| `./sqlite` | `src/sqlite/index.ts` | Portable `SqliteDatabase`, `SqliteExecutor`, `SqliteValue`; `applySqliteMigrations(database, migrations, { table })`; `validateSqliteSchemaPrefix`. |
| `./sqlite/node` | `src/sqlite/node.ts` | `NodeSqliteDatabase`, `openNodeSqliteDatabase(path, options)` over `node:sqlite` `DatabaseSync` (WAL, busy timeout). |
| `./sqlite/cloudflare` | `src/sqlite/cloudflare.ts` | `DurableObjectSqliteDatabase` and the structural `DurableObjectSqliteStorage` binding type. |

### Abstractions

All paths are in `packages/chord` at `v1.4.0`.

- **Contracts** (`src/contract.ts`, 440 lines). A contract is a runtime value (name, version, input schema, members), built with
  TypeBox. `local(...)` types are reachable only inside their host and may have any members; `remote(...)` types cross hosts
  and have `variants` (for example viewer and participant), `state` members (schemas) and `methods` (`method(args?, result?)`),
  and a mandatory placement `hostNamed("server")` or `hostFamily("session:", "session")`. `opaque(name)` names a native
  member. A consumer binds to a provider if versions are equal and the provider's description `coverage`-covers the consumer's;
  otherwise an `accepts[version]` adapter is needed or binding fails `contract_mismatch` (`docs/guide.md` "Versions and
  compatibility"). Fingerprints use a non-cryptographic 64-bit hash (`docs/spec.md` "Decisions").
- **Declarations** (`src/declare.ts`, `src/object.ts`). A factory is synchronous and declares through `self`: `state`
  (observable cell with schema, version, optional `migrate` and async `init`), `derive`, `resource` (`open`/`close`, versioned),
  `on` (listener on state or an event source), `task` (background fn), `get` (reference to another object), `register`
  (registry entry) and `expose`. Activation is asynchronous: open resources, wait for references, run cell `init`s, then go live
  (`docs/spec.md` "Synchronous factory, asynchronous activation").
- **Hosts** (`src/host.ts`, 1303 lines). `createHost({ name, self, principalKey, upstream?, loader?, ... })`; `host.serve(Type,
  factory, { lifetime: "referenced" | "host", linger, accepts })`, `host.get`, `host.hold`, `host.root(fn)`, `host.load`,
  `host.install`, `host.reload`, `host.accept`/`connect`/`route`, `host.invalidatePrincipal`, `host.close`. Principals are the
  application's own type; links stamp them.
- **Plugins, facets, registry, loader, manager.** A plugin has one **facet** per host kind (`defineFacet((host) => ...)`,
  `src/host.ts`); `nodeLoader` (`src/loader.ts`) imports fresh module copies per reload (it leaks old copies: `TODO(loader)` in
  the file). `defineRegistry` (`src/registry.ts`) is a per-code-version table of contributions from `lifetime: "host"` objects.
  `createHostManager` (`src/manager.ts`) forks and supervises a child host process (one instance at most, kernel lock, process
  group kill, liveness reports); it reports `durableTasks`, a number the application supplies in `report(...)` (`src/manager.ts:22,403`),
  and counts it toward idleness. Chord does not track tasks itself.
- **Cells, replicated state and deltas** (`src/cell.ts`, `src/state.ts`, `src/delta/*`). A cell holds an immutable JSON value;
  `change(ctx, draft => ...)` publishes exactly one atomic revision as ops. The delta tracker (`delta/tracker.ts`, 2239 lines)
  records string append, array splice, set and delete ops; remote viewers get a snapshot then ops, `reset` after a reload or
  reconnect, and nothing is replayed (`docs/guide.md` "State"). `replicatedState(initial)` is a standalone observable value
  independent of hosts; `replicatedState(source, { onError })` attaches a publication-only state to an authoritative
  revision source (README "Tracking JSON deltas"). `src/publish.ts` is one process-wide FIFO queue for all observer
  notifications, so observers see revisions in adoption order.
- **Cluster, connection, link, remote, wire, websocket (RPC over JSON links).** `src/link.ts`: an ordered bidirectional frame
  channel; `memoryLinkPair` (tests), `ipcLink` (Node child process), `webSocketLink` and `acceptWebSockets` (`src/websocket.ts`,
  35 lines; the authenticator returns the principal or refuses). `src/connection.ts` (1327 lines) runs one link: hello with
  `PROTOCOL_VERSION` (4 at `v1.4.0`, `docs/spec.md` section 9), ping/timeout, acks with a window (default 256 KiB) that holds
  back state frames only, per-link `maxCalls`, deflate above `PACK_THRESHOLD` (64K characters). `src/wire.ts`: the frame
  vocabulary, all JSON: `hello`/`bye`, `hold`/`release`, `shape`, `call`/`result`/`cancel`, `sub`/`unsub`/`resync`, `state`
  (`snap`, `ops`, `reset`, `error`), `gone`, `root_end`, `retire`, `invalidate`, `ping`/`pong`, `ack`, and reload control
  (`prepare`/`vote`/`commit`/`abort`). `src/remote.ts` binds remote references (one binding per type, input, principal key and
  contract fingerprint). `src/cluster.ts` coordinates cross-host reload.
- **Publish and reload** (`src/publish.ts`, `src/reload.ts`, 723 lines). Reload is load, prepare (new factories run in recording
  mode, writes throw), gate (only when a cell version is raised or an exclusive resource changes), commit (one synchronous step,
  code version E to E'), retire (two-version rule: current and previous). Roots in flight finish on the code they started with;
  `superseded` and `upgrade_conflict` are the only retryable error codes (`src/errors.ts:24`). The commit decision is not
  persisted (`docs/spec.md` section 12).
- **Context propagation** (`src/context/index.ts`, 161 lines). See the next section.
- **Output** (`src/output.ts`, 406 lines). `OutputBuffer` keeps a head or tail window at cost proportional to each chunk;
  `Progress` paces commits of a changing value by size. pi-durable's tools use both (see import table).
- **telemetry-sqlite** (`src/telemetry-sqlite/index.ts`, `store.ts`). Recording does no SQL; a one-second timer flushes settled
  spans and ten-second metric intervals in one transaction with retention cleanup (default seven days; `maxQueuedSpans` default
  10,000; README "SQLite telemetry"). Tables `telemetry_spans`, `telemetry_metrics`, `telemetry_schema` (README).
  I did not read `telemetry-sqlite` line by line; the table names and defaults are from the README, unverified against the code.

### Context propagation: `BACKGROUND_CONTEXT` and the rest of `./context`

Source: `packages/chord/src/context/index.ts` at `v1.4.0` (read in full).

- `Context` is an immutable interface with `abortSignal: AbortSignal | undefined`, `telemetry: TelemetrySpan` (never undefined;
  defaults to `NOOP_TELEMETRY_CONTEXT` from `pi-telemetry`), `value(key)` and `toString()`. It is a linked list of values.
- `BACKGROUND_CONTEXT` is the root: an `EmptyContext` named `[Context BACKGROUND_CONTEXT]` with no signal and no values.
  `TODO_CONTEXT` is a second empty root, a marker for call sites that still need a real context.
- **Propagation is explicit, never ambient.** There is no `AsyncLocalStorage`: a context is passed as an argument and derived
  with `withContextValue(key, value, parent)`, `withAbortSignal(signal, ctx)` (parent and new signal combined with
  `AbortSignal.any`), `withCancel(ctx)` (returns `{ context, cancel }`), `withoutAbortSignal(ctx)` (keeps values, drops caller
  cancellation, "for mandatory cleanup only"), `withTelemetry(span, ctx)` and `traceIn(ctx, options, fn)` (child span).
  Derivation never mutates the parent. `awaitWithContext(promise, ctx)` rejects the waiter on abort without cancelling the
  promise. README: "there is no ambient current span".
- Note the distinction: the host runtime has its own per-call `Ctx` (`{ signal, rootId, codeVersion, get, principal }`,
  `src/types.ts`, `docs/spec.md` section 6). That is a different type from the standalone `Context` in `./context`.
  `Context` is exported from `.` as a type and is what pi-durable and pi-env take as an argument.

### Task, job, workflow, schedule, retry, checkpoint? No, in 1.4.0

Evidence, all `packages/chord` at `v1.4.0`:

- `rg -il "retry|schedul|checkpoint|workflow|durable|persist" src` (non-test) lists `output.ts`, `manager.ts`, `telemetry-sqlite/index.ts`,
  `errors.ts`, `sqlite/node.ts`, `sqlite/cloudflare.ts`, `connection.ts`, `host.ts`, `object.ts`, `reload.ts`. Reading the hits: `retry` is a
  boolean `retryable` on `ChordError` for `superseded` and `upgrade_conflict` (`errors.ts:24-42`), meaning "start a new root";
  `schedule` is ack and output-commit timers; `checkpoint` is SQLite `PRAGMA wal_checkpoint` (`sqlite/node.ts:166`); `durable` is
  the `durableTasks` counter in `manager.ts`, the name "Durable Object" in `sqlite/cloudflare.ts`, and comments about pi's Durable in `host.ts:64` and
  `object.ts:200`; `persist` and `workflow` appear in no declaration, and `object.ts`, `host.ts`, `reload.ts` hits are the `superseded`/`upgrade_conflict` retry comments.
- **Task:** only `self.task(fn)`: in-memory background work, no identity, no state, no persistence, no retry (a throw is not
  restarted by chord; the guide says "Long-running work is a `task`, and long waits are state").
- **Job, workflow, schedule, retry policy, checkpoint:** none. State "survives" a reload only in memory; a cell is "not saved
  anywhere by having one" (`docs/spec.md` section 3: "`init` ... does not persist anything"). `docs/guide.md` "Durable" says
  the pi Durable harness is provided as a local object by the app, i.e. chord hosts Durable, not the reverse.
- **No automatic restart** of a failed host: "A terminal host failure is picked up by the host manager's `keepAlive` or `fatal`
  path, not inside Chord" (`docs/spec.md` section 12).

### Storage chord itself ships

- `./sqlite`: interfaces only (`SqliteDatabase` with async `exec`/`run`/`get`/`all`/`transaction`/`close`), plus
  `applySqliteMigrations` (atomic, contiguous versions from 1, one metadata row per `table`, refuses a database newer than the
  code) and `validateSqliteSchemaPrefix` (`src/sqlite/database.ts`, `migrations.ts`, `schema.ts`). It stores nothing itself
  except the migration metadata table the caller names.
- `./sqlite/node`: `node:sqlite` driver with a serial operation queue, WAL, `wal_autocheckpoint` default 1,000 pages, busy timeout
  default 5,000 ms (`src/sqlite/node.ts:9-15`).
- `./sqlite/cloudflare`: Durable Object SQL driver, serialised per storage, refuses `bigint` outside the safe integer range
  (`src/sqlite/cloudflare.ts`).
- `./telemetry-sqlite` is the only chord module with its own schema (telemetry tables, above).
- `CHANGELOG.md` 1.4.0: these SQLite modules were "Moved from Durable, which keeps its storage implementations and re-exports the
  moved public names". So chord owns the *driver layer*; pi-durable owns *what is stored* (its schema and `SqliteStorage`) and the
  non-SQLite backends.

**Backends pi-durable ships** (pi-durable README "Storage", `docs/research/pi-durable.md`; files verified in `packages/durable/src/storage`):
memory (`memory.ts`), JSONL (`jsonl/`), SQLite (`sqlite/`, `NodeSqliteStorage`, `cloudflare.ts`) and IndexedDB (`indexeddb.ts`). Chord
contributes the delta `Op` type and `apply`/`applyImmutableBatches` for materialising documents in the memory, IndexedDB and SQLite
storages (the JSONL files import only the `Context` and `JsonValue` types), plus the SQLite driver and migration runner for the two SQLite storages.

### Main differs from v1.4.0

`git diff v1.4.0 origin/main --stat -- packages/chord` at `33385f95...` (fetched with `--depth 50`): 341 files changed, 83066
insertions, 119 deletions; the version in `packages/chord/package.json` on `main` is still 1.4.0 and the `exports` list is the same
nine subpaths. What changed (read from `main`'s `CHANGELOG.md` "Unreleased", `README.md` diff and `src` file list):

- **Published-surface changes (Unreleased in the changelog; unverified that any is released after 1.4.0, since npm `latest` is 1.4.0):**
  `traceIn(options, fn, context)` takes the context last; watch types `Watch`, `NativeWatch`, `WatchHandle`, `WatchCall`, `WatchEnd`,
  `Watchable` moved from Durable into chord's root export (`src/watch.ts`); `SqliteExecutor.runCounted`; `PendingOps`, `ownedSlice`,
  `appendText` in `./delta`; Node SQLite statement cache capped at 256. pi-durable on `main` already imports `Watch` and
  `WatchHandle` from chord (`packages/durable/src/session/session.ts:1`, `packages/durable/src/harness/types.ts:1` on `main`).
- **Chord v2, "experimental and not exported yet"** (`CHANGELOG.md` Unreleased "Added"): 21,321 lines under `packages/chord/src/next`
  (counted with `git show` over `git ls-tree`) in modules `contracts`, `data`, `documents`, `host`, `jobs`, `link`, `remote`, `state`, `store`,
  plus `docs/next` (`design.md`, `decisions.md`, `slices.md`). Slices 1-5 and 6a are committed; 6b ("persisted jobs, Durable's
  `StoreStorage`") and 7-9 (reload, registries, telemetry) are not (`docs/next/slices.md` "What exists" and "The slices").
  - `src/next/jobs/README.md`: a **job** is owned work a call starts that outlives the call: intent (a transaction), run (any
    effect), finish (a transaction); optional per-run state document; `concurrency`; keyed dedupe; abort; `finish` retried
    after 1 s doubling to 60 s, blocked after five failures. In memory today: `persist` throws "persisted jobs are not supported
    before slice 6" (`src/next/jobs/definition.ts:74`).
  - `src/next/host/routines.ts`: `on` and `onLive` routines restart after a backoff 1 s to 60 s (`host/backoff.ts`). The explainer
    chapter `docs/next/explainer/src/10-routines.md` also shows `every`, `loop`, `custom`; I found no `every` in `routines.ts` (only in
    docs), so I infer it is design, not code.
  - `src/next/store/README.md`: a store over a SQLite backend (tables, persisted documents, outbox, migrations, export/import).
  - `docs/next/design.md` section 24 describes Durable integration (a `StoreStorage` for pi-durable over this store, the harness as a
    host-lifetime object). It is a design; `rg "chord/next"` over `packages/durable`, `packages/coding-agent` and the `package.json` files
    on `main` finds no use.
  - Because none of this is in the npm package, anything cpd would take from chord today is limited to 1.4.0's nine subpaths.
    That sentence states a fact about packaging; whether it matters is the next Job's.

## How pi-durable uses chord

Source: `rg -n "@earendil-works/chord" packages/durable/src packages/durable/README.md packages/durable/package.json` at tag `v1.4.0`
(re-run with a multi-line-aware script for the table below).

- `packages/durable/package.json:153`: `"@earendil-works/chord": "1.4.0"` under `dependencies`, exact pin. `packages/env/package.json:48` (pi-env)
  pins it the same way.
- `packages/durable/README.md:7`: "Built on `pi-ai` for model access and `@earendil-works/chord` for document state." `:43`: install line;
  `:49`: `import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";`.
- `packages/durable/src` has 84 import or re-export statements of chord in 51 files (list below; counts re-run on `main` give 86). The
  subpaths used are `.`, `./context`, `./delta`, `./output`, `./sqlite`, `./sqlite/node`, `./sqlite/cloudflare`. **`./node` and
  `./telemetry-sqlite` are not imported by pi-durable.**
- Names imported from `.`: types `Context`, `JsonValue`, `JsonRepresentation`, `Draft`, `AttachedReplicatedState`, `ReplicatedStateSource*`; values
  `copyJson`, `replicatedState`. From `./context`: `BACKGROUND_CONTEXT`, `withAbortSignal`, `withoutAbortSignal`, `withTelemetry`, `awaitWithContext`.
  From `./delta`: `track`, `apply`, `applyImmutable`, `applyImmutableBatches`, `overlap`, types `Op`, `Path`, `NonEmptyPath`, `Change`, `Prepared`,
  `Tracker`. From `./output`: `OutputBuffer`, `Progress`, `boundOutput`, `characterEnd`, `utf8ByteLength`, `PROGRESS_BYTES_PER_SECOND` (a constant defined at `packages/chord/src/output.ts:303`), `ProgressClock`,
  `OutputLimits`. From `./sqlite*`: `SqliteDatabase`, `SqliteExecutor`, `SqliteValue`, `SqliteMigration`, `applySqliteMigrations`,
  `validateSqliteSchemaPrefix`, `openNodeSqliteDatabase`, `NodeSqliteDatabase`, `DurableObjectSqliteDatabase`.

### Import table

Every import or re-export of `@earendil-works/chord` in `packages/durable/src` at `v1.4.0`, line = first line of the statement. Generated with a
script over the files (`import`/`export ... from "@earendil-works/chord..."`); statements with `type` imports are included.

| File | Line(s) | Subpath: names | Area (by directory) |
|---|---|---|---|
| `packages/durable/src/codemode/codemode.ts` | 37 | `chord`: Context, copyJson, JsonValue | types: `Context`, `JsonValue`, `Op` |
| `packages/durable/src/codemode/models.ts` | 10 | `chord`: Context, JsonValue | types: `Context`, `JsonValue`, `Op` |
| `packages/durable/src/documents.ts` | 1, 2 | `chord`: copyJson, JsonValue; `chord/delta`: Op | types: `Context`, `JsonValue`, `Op` |
| `packages/durable/src/entries.ts` | 1 | `chord`: JsonValue | types: `Context`, `JsonValue`, `Op` |
| `packages/durable/src/env/index.ts` | 1 | `chord`: Context | execution env: `Context` type |
| `packages/durable/src/env/node-watch.ts` | 5 | `chord`: Context | execution env: `Context` type |
| `packages/durable/src/env/node.ts` | 23 | `chord`: Context | execution env: `Context` type |
| `packages/durable/src/harness/agent.ts` | 1 | `chord`: copyJson, Draft | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/compaction.ts` | 1 | `chord`: Context, Draft | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/context.ts` | 1 | `chord`: Context | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/define.ts` | 1 | `chord`: JsonValue | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/events.ts` | 1, 2 | `chord`: Context, JsonValue; `chord/delta`: Op, Path | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/generation.ts` | 1 | `chord`: Context, copyJson, Draft, JsonValue | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/harness.ts` | 1, 2 | `chord`: AttachedReplicatedState, Context, JsonValue; `chord/context`: withAbortSignal, withoutAbortSignal | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/inbox.ts` | 1 | `chord`: copyJson, Draft, JsonRepresentation | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/json.ts` | 1 | `chord`: JsonValue | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/live.ts` | 1 | `chord`: Draft, JsonRepresentation, JsonValue | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/prompt.ts` | 1 | `chord`: Context | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/provider.ts` | 1 | `chord`: Context | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/scheduler.ts` | 1, 2 | `chord`: Context, copyJson, JsonValue; `chord/context`: awaitWithContext, withAbortSignal, withoutAbortSignal, withTelemetry | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/submissions.ts` | 1, 2 | `chord`: Context, copyJson, JsonRepresentation; `chord/context`: withTelemetry | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/task-graph.ts` | 1, 2, 3 | `chord`: AttachedReplicatedState, Context, JsonValue, replicatedState; `chord/context`: BACKGROUND_CONTEXT, withoutAbortSignal; `chord/delta`: applyImmutable, Op | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/tool.ts` | 1, 2, 3, 4 | `chord`: Context, copyJson, Draft, JsonRepresentation, JsonValue; `chord/context`: awaitWithContext; `chord/delta`: overlap; `chord/output`: boundOutput, OutputBuffer, OutputLimits, PROGRESS_BYTES_PER_SECOND, Progress, ProgressClock | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/types.ts` | 1 | `chord`: AttachedReplicatedState, Context, JsonValue | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/usage.ts` | 1 | `chord`: copyJson, Draft, JsonRepresentation | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/util.ts` | 1 | `chord`: Context | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/harness/view.ts` | 1, 2, 3 | `chord`: AttachedReplicatedState, Context, JsonValue, replicatedState; `chord/context`: withoutAbortSignal; `chord/delta`: applyImmutable, NonEmptyPath, Op, Path | harness: `Context`, `Draft`, `copyJson`, state and context helpers |
| `packages/durable/src/session/forks.ts` | 1 | `chord`: Context | session core: delta `track`, replicated state, context helpers |
| `packages/durable/src/session/observation.ts` | 1, 8, 9 | `chord`: Context, JsonValue, ReplicatedStateSource, ReplicatedStateSourceAttachment, ReplicatedStateSourceFrame; `chord/context`: withoutAbortSignal; `chord/delta`: Op | session core: delta `track`, replicated state, context helpers |
| `packages/durable/src/session/session.ts` | 1, 2, 3 | `chord`: Context, JsonValue, replicatedState; `chord/context`: awaitWithContext, BACKGROUND_CONTEXT, withoutAbortSignal; `chord/delta`: Op, track | session core: delta `track`, replicated state, context helpers |
| `packages/durable/src/session/transaction.ts` | 1, 2, 3 | `chord`: Context, copyJson, Draft, JsonValue; `chord/context`: BACKGROUND_CONTEXT; `chord/delta`: Change, Op, Prepared, Tracker, track | session core: delta `track`, replicated state, context helpers |
| `packages/durable/src/storage/indexeddb.ts` | 3, 4 | `chord`: Context, JsonValue; `chord/delta`: apply, Op | storage backend: `Context`, `JsonValue`, delta `Op` apply |
| `packages/durable/src/storage/jsonl/node.ts` | 1 | `chord`: Context | storage backend: `Context`, `JsonValue`, delta `Op` apply |
| `packages/durable/src/storage/jsonl/storage.ts` | 1 | `chord`: Context, JsonValue | storage backend: `Context`, `JsonValue`, delta `Op` apply |
| `packages/durable/src/storage/memory.ts` | 1, 2 | `chord`: Context, JsonValue; `chord/delta`: applyImmutableBatches, Op | storage backend: `Context`, `JsonValue`, delta `Op` apply |
| `packages/durable/src/storage/sqlite/cloudflare.ts` | 1, 4 | `chord/sqlite/cloudflare`: DurableObjectSqliteDatabase, DurableObjectSqliteStorage; `chord/sqlite/cloudflare`: DurableObjectSqliteDatabase, DurableObjectSqliteStorage | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/storage/sqlite/database.ts` | 1 | `chord/sqlite`: SqliteDatabase, SqliteExecutor, SqliteValue | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/storage/sqlite/index.ts` | 1 | `chord/sqlite`: SqliteDatabase, SqliteExecutor, SqliteValue | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/storage/sqlite/migrations.ts` | 1, 8 | `chord/sqlite`: applySqliteMigrations as applyMigrations, SqliteDatabase, SqliteMigration; `chord/sqlite`: SqliteMigration | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/storage/sqlite/node.ts` | 1, 2, 5 | `chord/sqlite`: validateSqliteSchemaPrefix; `chord/sqlite/node`: NodeSqliteDatabaseOptions, openNodeSqliteDatabase; `chord/sqlite/node`: NodeSqliteDatabase, NodeSqliteDatabaseOptions, openNodeSqliteDatabase | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/storage/sqlite/schema.ts` | 1, 3 | `chord/sqlite`: validateSqliteSchemaPrefix; `chord/sqlite`: validateSqliteSchemaPrefix | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/storage/sqlite/storage.ts` | 1, 2, 3 | `chord`: Context, JsonValue; `chord/delta`: apply, Op; `chord/sqlite`: SqliteDatabase, SqliteExecutor, SqliteValue | SQLite storage: driver types and migration runner re-exported/used |
| `packages/durable/src/testing/env-conformance.ts` | 1, 2 | `chord`: Context; `chord/context`: BACKGROUND_CONTEXT, withAbortSignal | test helpers shipped in src |
| `packages/durable/src/testing/storage-benchmark.ts` | 1, 2 | `chord`: JsonValue; `chord/context`: BACKGROUND_CONTEXT | test helpers shipped in src |
| `packages/durable/src/testing/storage-conformance.ts` | 1, 2, 3 | `chord`: JsonValue; `chord/context`: BACKGROUND_CONTEXT; `chord/delta`: Op | test helpers shipped in src |
| `packages/durable/src/tools/bash.ts` | 1 | `chord`: Context | built-in tools: `Context`, output helpers |
| `packages/durable/src/tools/file-mutation-queue.ts` | 1 | `chord`: Context | built-in tools: `Context`, output helpers |
| `packages/durable/src/tools/path-utils.ts` | 1 | `chord`: Context | built-in tools: `Context`, output helpers |
| `packages/durable/src/tools/read.ts` | 1, 2 | `chord`: Context; `chord/output`: characterEnd | built-in tools: `Context`, output helpers |
| `packages/durable/src/truncate.ts` | 1 | `chord/output`: utf8ByteLength | built-in tools: `Context`, output helpers |
| `packages/durable/src/types.ts` | 1, 2 | `chord`: AttachedReplicatedState, Context, Draft, JsonValue; `chord/delta`: Op | types: `Context`, `JsonValue`, `Op` |

`BACKGROUND_CONTEXT` is used (not just imported) in `session/session.ts:228,563`, `session/transaction.ts:693,705,718`,
`harness/task-graph.ts:174`, and in `testing/*`. It is the context for work that has no caller: closing a session after a failure
(`session.ts:228`), committing a batch (`session.ts:563`), a transaction's own context reset (`transaction.ts:693`), publishing task-graph
ops (`task-graph.ts:174`). Every durable call that does have a caller takes a `Context` argument, and the scheduler derives per-phase
contexts with `withAbortSignal`, `withoutAbortSignal` and `withTelemetry` (`harness/scheduler.ts:2`).

### Are tasks, checkpoints, replay and storage built on chord, or do they only borrow types?

Verified by reading imports and the files they name at `v1.4.0`:

- **pi-durable uses no chord host runtime at all.** `rg "createHost|defineFacet|defineRegistry|remote\(|local\(|memoryLinkPair|createHostManager|hostNamed" packages/durable/src`
  finds nothing. No contracts, hosts, links, objects, registries, hot reload, `./node` or wire frames. Chord's headline abstractions
  are not used by pi-durable at 1.4.0.
- **Durable tasks, checkpoints, replay and restart handling are pi-durable's own code.** Task definitions (`src/tasks.ts`, `defineTask`), the task
  state union (`src/types.ts`, around lines 599-640 incl. `sleeping`/`checkpoint` fields at about line 327), the scheduler and parking at
  `restartLimit` (`src/harness/scheduler.ts:94,453`, `harness/agent.ts:57`), tool replay policy (`replay: "safe"`, README line 277) and the
  code-mode journal (README line 368) do not call any chord API for these. Chord's imports there are `Context`, `JsonValue`,
  `Draft`, `copyJson` and the context helpers.
- **What chord does supply, by actual use:** (1) the **types and helpers every durable call is written against**: `Context` (explicit
  cancellation, telemetry), `JsonValue`, `copyJson`; (2) the **JSON delta machinery** that makes commits and documents work:
  `track`/`Change`/`Prepared` in `session/session.ts:3` and `session/transaction.ts:3` produce the ops that are committed, and storages apply
  them with `apply`/`applyImmutableBatches` (`storage/memory.ts:2`, `indexeddb.ts:4`, `sqlite/storage.ts:2`); (3) **`replicatedState`** for the
  observable document, view and task-graph states (`session/session.ts:372`, `harness/view.ts:118`, `harness/task-graph.ts:116`; README of chord
  names these three); (4) **output bounding** for tool output (`harness/tool.ts`, `tools/read.ts`, `truncate.ts`); (5) the **SQLite driver layer and
  migration runner** for `NodeSqliteStorage` and the Cloudflare storage (`storage/sqlite/*`), moved from pi-durable to chord in 1.4.0.
- **Storage:** the commit protocol and the schema (tables `conversations`, `entries`, `tasks`, `submissions`, `documents`, `document_revisions`,
  `record_ids`, `durable_metadata`, `durable_schema`) are pi-durable's (`storage/sqlite/schema.ts`, `migrations.ts`). Chord is below it as a
  driver and as the delta algebra the documents are made of. The `Storage` interface is pi-durable's (`src/types.ts:1179`).
- **Summary.** pi-durable *depends on* chord's lower layers (context, JSON delta, replicated state, output, SQLite drivers) and
  uses them for real, not just for types; it does not use chord's runtime layer, and the durable execution model is not in chord.
  Other consumers in `packages/*` at `v1.4.0`: `packages/env` imports chord's `Context` type (and `BACKGROUND_CONTEXT`, `withAbortSignal`
  in tests); `rg "@earendil-works/chord" packages --glob '!chord' --glob '!durable'` finds no other source import (`pi-telemetry`'s README
  mentions `@earendil-works/chord/telemetry-sqlite`; `pi-codemode`, `pi-ai`, `pi-coding-agent` and `pi-agent-core` (directory `packages/agent`) have no hit).

## Chord against cpd

cpd paths are on `origin/main` at `d943c49cc72e81264f613120995c37e625ea5c0f` (each path checked with `git ls-tree -r origin/main --name-only`).
Fit: **same job** = both do it, so chord could replace or duplicate; **different job** = related word, other purpose; **no equivalent** = only one side has it.

| Chord primitive (v1.4.0, `packages/chord`) | cpd equivalent | Fit |
|---|---|---|
| Object runtime: `createHost`, `host.serve` (`src/host.ts`) | None; cpd has no long-lived objects. One `cpd watch` engine per machine ticks engines E1.. (`engine/src/watch/engine.rs`) | no equivalent |
| `local`/`remote` contracts, `method`, version coverage (`src/contract.ts`) | `docs/contract.md`, `docs/commands.md` (prose contract; every command prints one JSON value, `cli/`) | different job: chord's are runtime values checked at bind time, cpd's are documents plus tests |
| Replicated state: `state` cell, snapshot plus deltas to remote viewers (`src/cell.ts`, `src/delta/*`) | SQLite store as the one source of truth (`core/src/store/mod.rs`); Events appended in the same transaction (`core/src/events/append.rs`); viewers read the store or follow Events | different job: chord replicates in-memory observable JSON; cpd persists relational rows and a log |
| Links and RPC frames (`src/link.ts`, `wire.ts`, `connection.ts`, `websocket.ts`) | `cpd` CLI one-shot JSON plus `cpd events follow` (`pi/lib/follower.ts`, `pi/lib/delivery.ts`; `core/src/events/select.rs`) | same job (talk to a client), different cost (see case c) |
| `self.task(fn)` background task (`src/object.ts:1897`) | Attempt driver (`core/src/driver/run.rs`, `worker.rs`), engine ticks (`engine/src/watch/engine.rs`) | different job: chord's is in-memory, no identity, no restart |
| `Context`, `BACKGROUND_CONTEXT`, `withAbortSignal` (`src/context/index.ts`) | None in Rust; `pi/lib/` passes `AbortSignal` ad hoc (call deadline `callTimeoutMs`, `pi/lib/follower.ts`) | no equivalent; pi-durable's API requires it |
| Hot reload across hosts (`src/reload.ts`, `cluster.ts`) | `pi/lib/reload.ts` (name only; contents unread, I infer it reloads the adapter) | different job: chord reloads plugin code in a live host |
| `ChordError` with `retryable` (`src/errors.ts`) | `CpdError`, error codes and `PERMANENT_CODES` (`pi/lib/follower.ts`, `core/src/error.rs`) | different job (`retryable` only means "start a new root") |
| Host manager: fork, supervise a child process, kernel lock (`src/manager.ts`) | Driver lock and liveness (`core/src/driver/lock.rs`, `engine/src/watch/liveness.rs`), `core/src/store/lock.rs` | different job: chord supervises its own child host; cpd supervises Workers in tmux |
| `OutputBuffer`, `Progress`, `boundOutput` (`src/output.ts`) | None needed in Rust; pi-durable's tools use them | no equivalent; transitive |
| `./sqlite`, `./sqlite/node`: `applySqliteMigrations`, `NodeSqliteDatabase` | `core/src/store/migrations/*.sql`, migrations frozen by `scripts/check-migrations-frozen.sh`; Rust `rusqlite` store | different job: Node driver and runner for pi-durable's DB; cpd's store is Rust |
| `./telemetry-sqlite`: spans and metrics (`src/telemetry-sqlite/index.ts`) | Per-Attempt `metrics` parsed from the Worker's session file (`core/src/metrics/mod.rs`) | different job: tracing spans vs summed tokens and cost per Attempt |
| Durable tasks, workflows, scheduler, checkpoint, retry | Engine E1.. and `retry_cap` (`engine/src/watch/liveness.rs`), jira queue `jira_transitions` (`docs/contract.md` §4.4), Jobs/Plans (`core/src/job/`, `core/src/plan/`) | chord has none at 1.4.0; pi-durable has them for agent conversations (`packages/durable/src/tasks.ts`) |

Message board, Slack and MCP (`core/src/board.rs`, `core/src/slack.rs`, `core/src/mcp.rs`), policy (`core/src/policy/`) and PRs (`core/src/pr/`) have **no chord counterpart**: chord has no messaging, integration or policy primitive.

## Where chord could help

Only one case clears the bar, and only in a minimal form.

1. **(a) Inside the pi-durable Worker runner: `@earendil-works/chord/context`.** Component: the planned headless runner (Plan `pi-durable-worker`, pro-d164), the cpd durable extension and the guards ported from `pi/lib/testguard.ts`, `mainguard.ts`, `secretguard.ts`, `hostapprove.ts`. API: `BACKGROUND_CONTEXT`, `withAbortSignal` from `./context`. Verified need: pi-durable's README imports `BACKGROUND_CONTEXT` from `@earendil-works/chord/context` (`packages/durable/README.md:49`) and every durable call takes a `Context` (existing section "How pi-durable uses chord"). Adds: a root context and a cancel signal for the runner's own calls (shutdown, the nudge follower). Replaces: nothing. Cost: one direct dependency in `package.json` (Node resolves only declared packages, `docs/research/pi-durable.md`), pinned to the same exact version as pi-durable's (`packages/durable/package.json:153`), and Node >=22.19.0 (`packages/chord/package.json:85`) against cpd's `>=22.18` (`package.json`). Axioms: convention over configuration is met (take the dependency, do not copy `Context`); YAGNI limits the import to `./context`. Verdict: **yes, as a thin direct import**; hand-writing a `Context` is possible since it is an interface (`src/context/index.ts`), but would copy what upstream owns.

## Where chord would not help

1. **(a) Inside the runner: context, state and RPC beyond `./context`.** The runner needs crash resume on the same Attempt. That is pi-durable's storage and scheduler (`packages/durable/src/harness/scheduler.ts`, `NodeSqliteStorage`), which "do not call any chord API for these" (existing section). `replicatedState`, `./delta`, hosts, links and RPC would add an observer path nobody in cpd reads: the follower reads cpd Events through the CLI. The nudge/escalation follower and end/usage report talk to cpd through `cpd` commands (`pi/lib/follower.ts`), not to a chord host. Cost if forced: a Chord host in the runner, a second state source beside cpd's store. Verdict: **no**.
2. **(b) Multi-step tool and MCP helpers.** Components `pi/lib/slack-post.ts` (61 lines), `slack-react.ts`, `jira-transition.ts` (201 lines). Each is a one-shot helper: one MCP call, one JSON line, a typed `Refusal` (read the heads of `slack-post.ts` and `jira-transition.ts`). Retries and the queue are cpd's: a Jira transition is queued in `jira_transitions` in the same transaction as the post and performed later by engine E19 (`docs/contract.md` line 373); how E19 retries I did not read, unverified. Chord offers no steps, child workflows or checkpointed retries (verified, existing section "Task, job, workflow, schedule, retry, checkpoint? No"). pi-durable's `defineTask` gives durable tasks, but they live in a pi-durable conversation's storage (`packages/durable/src/tasks.ts`, `src/types.ts`), so using them for these helpers would add a second queue beside cpd's SQLite store and run an LLM harness for a deterministic call. Today's helpers are exit-code simple and tested with a mock `CallTool`. Verdict: **no**.
3. **(c) The adapter as a Chord plugin or host.** Component `pi/extension.ts`, `pi/mcp-extension.ts`, `pi/lib/follower.ts`. API that would be used: `createHost`, `remote(...)` contracts, `webSocketLink`/`acceptWebSockets`, `replicatedState`. Gain: typed push of state to a viewer instead of `cpd events follow`. Cost: cpd's Rust core would have to be a Chord peer or sit behind a Node host (a Rust-to-Node boundary and a second source of truth); pi's coding agent does not import chord (`rg "@earendil-works/chord" packages --glob '!chord' --glob '!durable'` finds no other source import, existing section), so pi would never consume those contracts; and pro-d109 (cpd is a CLI; the adapter is one interface, cited in `docs/research/pi-durable.md`) would invert. The contract is a document, not code, and `cpd events follow` already gives ordered delivery (`pi/lib/delivery.ts`). Verdict: **no**.
4. **(d) Rust store and engine against chord state and SQLite.** Components `core/src/store/`, `engine/src/watch/`. Chord's SQLite modules are a Node driver plus migration runner (`src/sqlite/*`) and chord stores nothing durable itself (existing section "Storage chord itself ships"). Using them would mean a second store beside Rust's `rusqlite` one, a Node engine duplicating E1.. and migrations frozen by `scripts/check-migrations-frozen.sh` ceding to `applySqliteMigrations`. Verdict: **no**.
5. **(d) A Rust client (cpd-tui, the cli) speaking Chord's wire.** Verified: frames are JSON and validated by one TypeBox union (`src/wire.ts`, `FrameSchema`); `docs/spec.md` §9 lists them in prose; the link layer sends text frames, and a binary WebSocket message `[u32 BE envelope length][envelope JSON][deflated payload]` for payloads above 64K characters (`src/link.ts:108-218`). **Not language-neutral and not independently specified:** the only description besides prose is TypeScript source, the spec says "Where it and the code disagree, the disagreement is a bug" (`docs/spec.md:3`), `PROTOCOL_VERSION` is 4 and peers "do not negotiate" (`docs/spec.md` §9), it rose in 1.3.0 and fields changed with it, and a client must reimplement contract tokens, `coverage` checks, a 64-bit non-cryptographic fingerprint and the delta op decoder (`src/delta/README.md` is the contract for the latter; not read). I found no Rust, Python or Go source in the `v1.4.0` clone (`grep -rIl chord --include=Cargo.toml --include=*.rs --include=*.py --include=*.go`: none), so I infer no non-TypeScript implementation exists. It would not help the cockpit: `tui/` already reads the store and Events through `cpd` (unverified: I did not read the TUI source or `docs/interface.md`), and a native chat UI needs only cpd's session Events. Verdict: **no**.
6. **(e) Telemetry.** Components `core/src/metrics/mod.rs` (Rust; sums `usage` from the Worker's pi session file per Attempt). `./telemetry-sqlite` stores spans and ten-second metric intervals in its own DB (default 7 days of retention; README and `src/telemetry-sqlite/index.ts` head, not line by line). pi-durable does not import it (existing section), and pi-durable keeps usage in its own storage (`packages/durable/src/harness/usage.ts`, name only; content unread, unverified). It would add a second metrics store that Rust cannot read except by opening the file. Verdict: **no**; the durable runner's end/usage report should read pi-durable's usage, not chord telemetry.
7. **(f) Cost side.** Maintenance and coupling: pi-durable pins chord exactly (`"@earendil-works/chord": "1.4.0"`, `packages/durable/package.json:153`), so cpd's direct pin must move in lockstep or npm installs two copies (guess: npm would nest a second copy when ranges differ). Cadence: 19 versions in 47 days, breaking changes in minors 1.2.0, 1.3.0 and 1.4.0 (`CHANGELOG.md`, existing section). Node: chord and pi-durable need >=22.19.0, cpd declares >=22.18 (`package.json`): adopting either means raising cpd's `engines.node`, a change this PR does not make. Roadmap risk: chord v2 on `main` (`packages/chord/src/next`, jobs and a persisted store) is unreleased and design-stage, so anything built on 1.4.0 chord abstractions may move; and pi-durable's own README calls itself Experimental. Axioms: "tests only test authored code" means chord is mocked in cpd tests, so a direct chord use should be one thin seam. Verdict: costs are real but small for the `./context` import and large for anything else.

## Recommendations

1. **Take chord only as pi-durable's dependency, plus one direct import.** When Plan `pi-durable-worker` starts: add `@earendil-works/chord` to cpd's `package.json` at the same exact version as pi-durable's pin (1.4.0 today), import only `@earendil-works/chord/context` (`BACKGROUND_CONTEXT`, `withAbortSignal`), in one module. Why: pi-durable's API takes a `Context` and its README imports `BACKGROUND_CONTEXT` from there; the rest of chord is unused by the runner (case a). Would change if: pi-durable re-exports its own root context, or an implementer finds the runner can call pi-durable without ever constructing a `Context`; then import nothing.
2. **Do not import chord's host runtime, contracts, replicated state, delta, SQLite or telemetry in cpd.** Why: no cpd job needs them (cases a, b, d, e) and each adds a second source of truth. Would change if: chord v2 ships persisted jobs and a documented, language-neutral protocol, and a concrete cpd need (for example a durable multi-step helper beyond the `jira_transitions` queue) is shown.
3. **Raise cpd's `engines.node` to `>=22.19.0` in the same change that adds the dependency (Plan worker-contract or pi-durable-worker), not in this PR.** Why: chord and pi-durable declare `>=22.19.0`; cpd declares `>=22.18` (`package.json`). Would change if: the runner is shipped as a separately installed package with its own `engines`.
4. **Do not move `pi/lib/slack-post.ts`, `slack-react.ts` or `jira-transition.ts` onto chord or pi-durable tasks.** Why: chord has no durable steps; pi-durable tasks would duplicate cpd's queue and tie a deterministic call to an LLM harness (case b). Would change if: E19's retry handling is shown (read `engine/src/watch/` and `core/src/pr/jira_sync.rs`) to lose steps in a way a durable task would fix.
5. **Do not build the adapter as a Chord plugin or host, and do not write a Rust Chord client.** Why: pi does not consume chord; the wire is TypeScript-defined, version 4, no negotiation, one implementation (cases c, d). Would change if: Earendil publishes a protocol spec or a second-language implementation, and the cockpit needs live push the Events path cannot give.
6. **Re-check chord at each pi-durable upgrade.** Why: breaking minors every few weeks and an exact pin. Add a line to the pi-durable Worker Plan's upgrade step: diff `packages/chord/CHANGELOG.md` and confirm the pin still equals pi-durable's. Would change if: chord's releases slow and keep semver.

## Open questions

- **How E19 retries and what happens to a half-done queue step.** Not read: `engine/src/watch/` E19 and `core/src/pr/jira_sync.rs`. Verify by reading them and by checking recommendation 4's "would change" condition.
- **Whether pi-durable can run without the runner touching `Context`.** Not verified: a prototype would show it. Verify by writing the smallest program that opens `NodeSqliteStorage` and submits a message, outside the repo.
- **Whether `src/delta/README.md` and the delta encoder make the wire usable from Rust.** Not read. Verify by reading it; it would not change the verdict unless the protocol also gets a published spec.
- **What chord v2 will ship and when.** Only `main` (`33385f95`) design docs were read; no release exists. Verify with `npm view @earendil-works/chord dist-tags` at the Worker Plan's start.
- **Whether `tui/` reads anything chord could serve.** The TUI source was not read. Verify by reading `tui/src` data access.
- **What pi-durable's `usage.ts` records.** Name only. Verify when the end/usage report is designed.

## Sources

- **Git:** `https://github.com/earendil-works/pi`, tag `v1.4.0`, commit `4a080252f06caf63a525d7873c5d94d3c23887e0` (committed 2026-10-21 18:29 EDT),
  cloned with `git clone --depth 1 --branch v1.4.0` into a temp directory. `main` for diffs: commit `33385f9572303e79bfb3811cc92cc8dd65e22261`
  (2026-10-28 14:53 EDT), `git fetch --depth 50 origin main`, `git diff v1.4.0 origin/main --stat -- packages/chord`.
- **npm commands run:** `npm view @earendil-works/chord versions dist-tags time --json`; `npm view @earendil-works/chord@1.4.0 --json`;
  `npm pack @earendil-works/chord@1.4.0` (outside the repo; `tar tzf`). The version inspected is `1.4.0` (`dist-tags.latest`).
- **Files read at `v1.4.0` (`packages/chord`):** `package.json`, `README.md`, `CHANGELOG.md` (through 1.2.0), `docs/guide.md` (in full), `docs/spec.md`
  (in full), `src/index.ts`, `src/declare.ts`, `src/registry.ts`, `src/loader.ts`, `src/node.ts`, `src/publish.ts`, `src/websocket.ts`,
  `src/context/index.ts` (in full), `src/sqlite/{database,migrations,schema}.ts` (in full), `src/sqlite/node.ts` (first 60 lines),
  head comments of `cell.ts`, `cluster.ts`, `wire.ts`, `link.ts`, `contract.ts`, `manager.ts`, plus keyword searches in `src` for retry, schedule,
  checkpoint, workflow, durable, persist, and `task` in `object.ts`.
- **Files read at `v1.4.0` (`packages/durable`, `packages/env`):** `package.json`, `README.md` (head and the lines cited), every `@earendil-works/chord`
  import (script), `src/harness/scheduler.ts` (head), `src/storage/sqlite/{schema,migrations,node}.ts` (head), `src/tasks.ts`, `src/types.ts` (grep).
- **Files read on `main`:** `packages/chord/CHANGELOG.md` (Unreleased), `package.json`, `README.md` (diff), `src/index.ts` (diff),
  `src/next/jobs/README.md`, `src/next/jobs/definition.ts` (grep), `src/next/host/routines.ts` (head, grep), `src/next/host/backoff.ts`,
  `src/next/store/README.md` (head), `src/next/documents/README.md` (head), `docs/next/design.md` (head and section 24), `docs/next/slices.md` (head),
  `docs/next/explainer/src/10-routines.md` (head).
- **Not read (so no claim above depends on them):** `src/object.ts`, `host.ts`, `connection.ts`, `remote.ts`, `reload.ts`, `cluster.ts`,
  `manager.ts`, `state.ts`, `cell.ts`, `delta/tracker.ts` beyond comments and keyword hits; `src/telemetry-sqlite/*` and `src/output.ts` (README and
  size only); `test/**` (only file names listed); `docs/tutorial.md`; `src/delta/README.md`; `CHANGELOG.md` before 1.2.0; the whole of `docs/next/*` beyond the
  parts named; the `dist/` output (module list only); any benchmark or test run (none was run).
- **Added by the verdict Job (2026-10-28 14:58 EDT).** Re-cloned `v1.4.0` (`git rev-parse HEAD`: `4a080252f06caf63a525d7873c5d94d3c23887e0`, same as above; npm `@earendil-works/chord` 1.4.0). Re-read and relied on: `packages/chord/src/wire.ts` (head, `PROTOCOL_VERSION`, frame schema), `src/link.ts` (packing and WebSocket framing, lines 108-218), `docs/spec.md` §9 and line 3, `package.json` engines, `packages/durable/package.json` (chord pin at line 153, engines `>=22.19.0`), `src/telemetry-sqlite/index.ts` (first 40 lines), `README.md` (entry points and WebSocket sentences). Grepped the clone for Rust, Python and Go sources mentioning chord: none.
- **cpd paths read, `origin/main` `d943c49cc72e81264f613120995c37e625ea5c0f` (2026-10-28 14:56 EDT):** existence of every path in the table of "Chord against cpd" (`git ls-tree`); heads of `pi/lib/slack-post.ts`, `pi/lib/jira-transition.ts`, `pi/lib/follower.ts`, `core/src/metrics/mod.rs`, `core/src/store/mod.rs`; `docs/contract.md` line 373 and keyword hits; `package.json`. A `git grep` for retry/backoff in `core/src/pr` and `engine/src/watch` only matched Attempt and liveness code.
- **Corrections to the facts sections:** none of the verified facts was wrong. Changed: the title and the opening paragraph (they said facts only), and the telemetry sentence, which already said it was unverified, stays so.
- **Not read for the comparison (so no claim depends on them):** `engine/src/watch/*` beyond names and a grep, `core/src/pr/jira_sync.rs`, `core/src/driver/*`, `tui/src`, `docs/interface.md`, `docs/implementing.md`, `docs/commands.md`, `pi/extension.ts`, `pi/mcp-extension.ts`, `pi/lib/slack-react.ts`, `pi/lib/reload.ts`, the guards, `src/delta/README.md`, `packages/durable/src/harness/usage.ts`.
- **Sibling research:** `docs/research/pi-durable.md` (origin/main), cited for the dependency graph, storage list and extension API; not repeated.
