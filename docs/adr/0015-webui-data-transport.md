---
status: accepted
---

# ADR 0015: The WebUI data transport

**Accepted, 2026-11-07 (air-d46).** The audit of the whole WebUI data path was the deliverable of Plan `webui-data` (air-d30); this ADR is the target design it asked for. At the ADR review the User picked **option B** (air-d46): one WebSocket transport shared by the Electron desktop and the browser build, with a ping/pong heartbeat, plus the serve-stream watchdog on top. The audit (below) and the comparison criteria stand as the recorded baseline; the frame-timeout/seq-watchdog requirement (Failure modes, point 2) is decided with them. The build is Plan `webui-ws`; §The plan (accepted) is its concrete file plan.

## Context

The WebUI backend (`web/src/server/`) runs `cpd cockpit serve` (commands.md §11a, docs/commands.md:3917; contract §5.5, docs/contract.md:3471) and carries its frames to the renderer's store. Two transports exist today and the path was audited end to end (air-d30; the failure analysis rests on the toast-fix root-cause artefacts, air-d32). Four findings shape this ADR: the path works, but nothing detects a stall; a live-but-silent serve child silently freezes every pane (air-n13, air-n14, air-n15); a stale pane update is dropped silently; and WebSocket is an option, not a requirement (air-d31), while "the same WebUI on a remote machine over ssh" is a comparison criterion (air-d35).

### The current path, end to end

One NDJSON pipe and two client transports:

1. `cpd cockpit serve` is spawned once by the Electron main process (`realBoundaries.stream`, web/src/server/plugin.ts:248-275, args `["cockpit","serve"]` at web/src/server/cockpit/stream.ts:33, spawn in `launch` at stream.ts:148-154) and, on the dev-server path, shared with the Vite plugin (web/src/server/plugin.ts:94-158). Its frames are `hello`, `snapshot`, `event`, `plan_show`, `error` (web/src/server/cockpit/stream.ts:39-44), parsed one line to one typed frame by `parseFrame` (stream.ts:47-78) and dispatched by `onFrame` (stream.ts:108-137); `hello` resets failures and sets ready (stream.ts:117).
2. The cockpit service (`createCockpitService`, web/src/server/cockpit/events.ts:73) turns snapshots and `cpd events follow` lines (the fallback, args from web/src/server/cockpit/cli.ts:53-55) into pane updates and emits them (`emit`, events.ts:92-95). Pane cursors come from the store: `mapTree` computes the minimum of the four panes' `events_cursor` (web/src/server/cockpit/readmodel.ts:425-430).
3. `HubConnection.subscribe` sends a full `cockpit.snapshot` and forwards each update as `cockpit.patch` or `cockpit.error` (web/src/server/hub/connection.ts:69-80). One `newConnection(send)` factory backs every transport (web/src/server/plugin.ts:94,157).
4. Transport to the renderer: Electron main and renderer talk over the `cpd:c2s`/`cpd:s2c` IPC channels (web/src/electron/channels.ts:12-13; one hub per `event.sender` at web/src/electron/cockpit-ipc.ts:29-31, listener at :43; preload bridge at web/src/electron/bridge.ts:34-35). The renderer's desktop transport is `createElectronTransport` (web/src/electron/transport.ts:5-38); the dev-server path is `createHotTransport` over `import.meta.hot` (web/src/main.tsx:8-39), dev-only — it throws without `import.meta.hot` (main.tsx:41-42). There is no production browser server, no HTTP server and no WebSocket server in `web/` today.
5. The renderer depends only on the `Transport` interface `{ connect, send, close, onMessage, onStatus }` (web/src/protocol/runtime.ts:10-15). `createConnection` (web/src/state/connection.ts:95) parses each raw frame against a `REQUIRED` shape table (connection.ts:170-204), coalesces transcript messages per 16 ms, and dispatches them into the store (`reduceServer`, web/src/state/store.ts:219-268; `reduceSession`, store.ts:124-202; store at store.ts:368-389, wired in web/src/ui/App.tsx:44-48).

### Failure modes

1. **No stall is visible today.** The snapshot `seq` is parsed and validated — "snapshot without an integer seq" (stream.ts:57) — carried on the snapshot (stream.ts:60) and then never read again: no comparison, no gap check, no idle timer. There is no heartbeat and no frame timeout anywhere on the path. The only loud failures are child exit (`onExit`, stream.ts:156-166: ready off, pending requests resolve `"down"`, one error, backoff relaunch 500 ms doubling, cap 10 s — stream.ts:31-32), malformed frames (stream.ts:112-115) and `error` frames (stream.ts:128-137).
2. **A live-but-silent serve child silently freezes every pane** (the air-n13/n14/n15 freeze). While the stream is considered ready, every resync path is disabled: the `events follow` fallback early-returns (events.ts:157), the 30 s safety tick refreshes the live panes only when the stream is *not* ready (events.ts:147), and the follow process is killed on every snapshot (events.ts:225) and restarted only on a not-ready transition (events.ts:253-257) — which never happens for a child that never exits. `ready()` stays `true` from the hello (stream.ts:117, `ready` at stream.ts:183), so `onExit` never fires. Evidence: air-n15 (recorded 2026-11-07 13:37 EST) — the WebUI tree frozen at a snapshot from about 13:20 EST while `cockpit serve` (pid 61683, started 13:19:43 EST) was still running; the mitigation was a `SIGUSR1` reload (web/src/electron/boot.ts:25-32). The root cause is recorded in the toast-fix artefacts as air-n13, a serve child or read-model that omits or stalls a pane (air-d32); the mechanism above is consistent with it and explains why nothing resynced.
3. **A lower-cursor pane update is dropped silently.** The stale-update guard `apply` (events.ts:99-101) applies a pane only when its computed min-of-four cursor is >= the held one; a regressed cursor is dropped with no error and no emit, so a pane can freeze indefinitely with nothing on any error path.
4. **A serve re-exec with `seq` restarting at 1 is safe.** On a new install `cockpit serve` re-execs in place with the same pid and pipes, says `hello` and writes snapshot 1 (docs/contract.md:3486); "`seq` counts this process's snapshots from 1; a new process starts at 1 again" (docs/commands.md:3942, docs/contract.md:3482). The web side cannot lose updates to this: `seq` is discarded after validation (stream.ts:57,60) and the cursor guard uses the store's `events_cursor`, not `seq` (events.ts:99; readmodel.ts:425-430) — the fresh post-restart snapshot's cursor equals the highest committed event id, which is >= anything held. On the web side a re-exec surfaces as a fresh `hello`, which resets failures and pending (stream.ts:117), or a child exit with backoff relaunch (stream.ts:156-166).

## Options compared

All options keep the same server-side protocol seam — the `ClientMessage`/`ServerMessage` wire union (web/src/protocol/wire.ts:6-22,35-61), `HubConnection.handle` parsing each client frame once (web/src/server/hub/connection.ts:189) and the one `newConnection` factory (web/src/server/plugin.ts:157) — so the comparison is apples-to-apples at that boundary, and keep the `cpd cockpit serve` pipe behind it (air-d31: the question is only the renderer seam).

### Option A — keep the NDJSON pipe + Electron IPC (the current desktop path)

The status quo. Main runs the services in-process and pushes to the renderer over IPC (web/src/electron/cockpit-ipc.ts:29-31,43); the browser path stays the dev-only Vite socket (web/src/main.tsx:41-42).

### Option B — one WebSocket shared by the Electron desktop and the browser build

Keep the protocol and the `Transport` interface; add one production WebSocket endpoint wrapping the same `newConnection` (web/src/server/plugin.ts:157) and one client transport `createWebSocketTransport` used by both the desktop and the browser build. Node has no built-in WS server; take `ws` pinned (<https://github.com/websockets/ws>), not hand-rolled RFC 6455 framing (<https://www.rfc-editor.org/rfc/rfc6455>; browser API <https://developer.mozilla.org/en-US/docs/Web/API/WebSocket>).

### Option C — SSE down + POST up

Same protocol seam over plain HTTP: server frames as `text/event-stream` (`EventSource`, <https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events>), client frames as POSTs. No upgrade handshake, no framing to own; no new dependency (`node:http`).

## Comparison on the criteria

Criteria per air-d30/air-d35: correctness (no lost or stale updates, ordering, resync after a serve re-exec with `seq` restarting at 1, reconnect), simplicity, failure visibility (a stalled stream must be seen, never silent), testability with mocks, and remote reach over ssh in the two models R2 and R3 (air-d35): **R2** — backend stays on the User's machine, the remote machine reaches it by spawning the cpd children over ssh and forwarding the durable socket (`connectHost` takes any socket path, web/src/server/durable/client.ts:94); **R3** — backend runs on the remote machine, the desktop is a thin client connected through `ssh -L` (<https://man.openbsd.org/ssh#L>).

| criterion | A: pipe + IPC | B: one WebSocket | C: SSE + POST |
|---|---|---|---|
| correctness: lost/stale updates, ordering | ordered, in-process; cursor guard unchanged (events.ts:99-101) | TCP ordering; same guard | ordered per stream; same guard |
| correctness: resync after serve re-exec (seq → 1) | safe: `seq` discarded (stream.ts:57,60); guard uses `events_cursor` (readmodel.ts:425-430); fresh `hello` resets (stream.ts:117) | same | same |
| correctness: reconnect | **none** — IPC never emits `"closed"`; `createElectronTransport` emits only `"open"` (transport.ts:18-25), so the renderer's backoff-and-resubscribe branch (connection.ts:205-224) is dead on the desktop | socket `close`/error maps to `onStatus("closed")`, which runs the existing backoff and re-subscribe for a fresh full snapshot (connection.ts:205-224; hub/connection.ts:69-80) | close is visible too, but the client is two channels glued to one status |
| simplicity | best: already built; two near-duplicate client transports (transport.ts:5-38 vs main.tsx:8-39) | one client transport removes the duplication; adds a WS server + pinned dependency, and the desktop gains a server process it never needed | simplest server (no dependency), most bespoke client (`EventSource` + `fetch`, correlated by hand) |
| failure visibility | child exit, malformed and error frames are loud (stream.ts:112-137); a stalled stream is silent — no heartbeat, no timeout, `seq` never compared (stream.ts:57,60); IPC cannot carry a heartbeat | best of the three: `close` is a first-class event (RFC 6455 §5.5, <https://www.rfc-editor.org/rfc/rfc6455>), and ping/pong is the natural heartbeat hook | close visible; SSE comment heartbeats must be built; POST failures need explicit surfacing |
| testability with mocks | done, and the pattern is hand-written fakes behind injected boundaries: the serve child (web/src/server/cockpit/stream.test.ts:6,30,42), the stream and follow whole (web/src/server/cockpit/events.stream.test.ts:11,19), the IPC bridge (web/src/electron/transport.test.ts:4) | as good: the `Transport` interface is unchanged (runtime.ts:10-15); the new server socket is injected like the durable client's `SocketFactory` (web/src/server/durable/client.ts:94) | same pattern with fake `EventSource` + fake `fetch`; no test opens a real port |
| remote R2 (ssh children + forwarded socket) | works | works | works |
| remote R3 (backend remote, thin client over `ssh -L`) | **impossible**: Electron IPC is a channel between main and its own renderer (channels.ts:12-13; <https://www.electronjs.org/docs/latest/tutorial/ipc>), not a network endpoint | **the intended fit**: the WS server runs on the remote machine, the desktop connects to `ws://127.0.0.1:<port>` through `ssh -L` (<https://man.openbsd.org/ssh#L>) | works: plain HTTP on a port forwards the same way |

## Recommendation (proposed)

**Option B: one WebSocket transport for both the desktop and the browser build, with a ping/pong heartbeat.** Reasons:

1. **It closes the reconnect hole with code that already exists.** The renderer reconnects on a `"closed"` status with backoff and re-subscribes for a fresh snapshot (connection.ts:205-224; hub/connection.ts:69-80); the desktop transport never emits `"closed"` (transport.ts:18-25), so that machinery is dead today. A WebSocket emits `close` for free.
2. **It removes the duplicated client transports** (transport.ts:5-38 and main.tsx:8-39 implement the same `Transport` semantics twice) and is the least bespoke option that reaches R3 (option C reaches R3 too, with a two-channel client); it also keeps R2 open (air-d35).
3. **It gives the heartbeat a natural hook**, so a stalled transport is seen, never silent — the failure-visibility criterion the current path fails.

The cost is one pinned dependency and a server process the desktop did not have; the protocol seam and every mock pattern survive unchanged. If R3 were dropped and only R2 wanted, option A is the smaller change — air-d35 keeps R3 on the table, so B is the proposal.

**A separate frame-timeout/seq-watchdog is needed on top of any transport.** No transport change fixes a serve child that stays alive and stops emitting: the heartbeat sees the socket, not the pipe. The web side must watch the stream boundary itself — a frame timeout, or a check that the snapshot `seq` it already parses (stream.ts:57,60) advances — and must not gate on `ready()`, because while the stream is ready both existing resync paths are disabled (the safety tick skips pane refresh, events.ts:147, and the follow fallback early-returns, events.ts:157). This watchdog is a later Job in this Plan, not part of the transport pick.

## Consequences

- The renderer keeps depending only on `Transport` (runtime.ts:10-15); whichever option is picked, no store or reducer change is implied by the transport alone.
- The audit findings above are the acceptance baseline for the build Jobs: after the build, a stalled serve child must be seen (watchdog), a dropped transport must reconnect (whichever transport), and the silent lower-cursor drop (events.ts:99-101) must be surfaced or removed.
- The build is a later Workflow of Plan `webui-data` (air-d30): nothing here is implemented, no dependency is added, and docs/contract.md §5.5 and docs/commands.md §11a are amended by the build's docs Job, not here.

## Rejected alternative

**Keep option A unchanged and only add the watchdog.** Rejected as a whole answer: the watchdog fixes invisibility but not the reconnect hole — IPC never emits `"closed"`, so the renderer's reconnect path stays dead on the desktop (transport.ts:18-25; connection.ts:205-224) — and leaves the two client transports duplicated and R3 unreachable (air-d35). The watchdog is needed on top of any transport, not instead of one.
