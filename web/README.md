# cpd web client

The browser UI for cpd with cockpit parity (hs-d757): Tree, Plan, Inbox, Logs and read-only Chat, with approvals answered and the Escalation count (hs-d776). React + Vite + shadcn/ui stock components only (hs-d770, hs-d783), built **against the typed mock of the decided API** before the core and server land (hs-d780). The wire surface, mock contract, fold semantics and architecture are the contract of `docs/research/web-client-api.md` — read it first.

## Run the dev server

From the repo root:

```
pnpm install
pnpm --filter cpd-web-client run dev
```

The Vite dev server serves the client **against the mock** on **port 5173** (plan web-client A2). The API layer attaches the token to every request; the mock refuses token-less requests (hs-d781, A4).

## Where the pieces live (`web/src/`)

| Path | Holds |
|---|---|
| `api/` | the typed wire surface, the one `CpdApi` interface, the token attachment, typed POST actions, the WebSocket live channel |
| `api/mock/` | the typed mock implementing the interface (served by the Vite dev server) |
| `fold/` | the TS chat fold: apply each `AgentEvent` of an `events` frame in order; whole message at `message_start`, replaced at `message_end`; a `snapshot` replaces the whole view; cursor epoch/seq resume (durable-leads §4.3, hs-d772) |
| `parity/` | the read-model mirror types (plan list/show, session list, inbox list JSON → TS) and the **parity skeleton**: a test that, when W1 lands, imports the core `Serialize` read-model and asserts CLI output equals the mirror (hs-d771, hs-d779) |
| `views/` | the cockpit views — Tree, Plan, Inbox, Logs, Chat, all read-only (A7) |
| `app/` | the app shell: routing, the live connection, the token bootstrap |
| `components/` | shadcn/ui stock components only (hs-d783) |

P1.6/P1.8 shapes that are still open are flagged `@open(P1.6/P1.8)` in the type source and are never guessed (hs-n596).

## The dev live adapter (dev mode only)

The mock is the default and stays the default (hs-d780). The **dev live adapter** is a dev-mode-only path that reads a **real cpd Session** into the Chat and sends to it through the cpd CLI. It lives entirely in the Vite **dev server layer** plus a browser-side API; the production surface is untouched: `docs/research/web-client-api.md` and the typed `CpdApi` are unchanged, no production API contract changes, and the real HTTP server remains a later Plan. The adapter is runner-agnostic: a **pi** Session is read from its pi session file, a **durable** Session is read over its **host socket** when a live host answers and through `cpd session tail` otherwise (the `chat-host` read, below). The browser API (`LiveApi`, the snapshot-frame payload) does not change either way.

There is no approvals UI here: no tree, inbox or plan wiring, no `ui_response` answering — the adapter reads transcripts and sends messages, nothing else.

### Activation and the token

- The routes mount **only with `CPD_WEB_LIVE=1`** in the dev server's environment (exactly the value `1`; any other value, including unset, changes nothing — the existing dev and build flows are exactly as before).
- The token is **`CPD_WEB_TOKEN`**, default **`dev-token`**. Every `/__cpd/` request must carry it as the `x-cpd-token` header **or** the `?token=` query parameter. A request without the token — or with the wrong one — is refused **401** `{"ok":false,"error":"missing or wrong token"}` before any read or send runs.
- `GET /__cpd/health` is the probe: `main.tsx` calls it (with the token) before mounting and uses `createLiveApi` **only when it answers `ok`**. When the probe fails — adapter off, dev server restarted without the flag, anything — **the mock remains the default**: `createMockApi()` and the existing Chat render exactly as today, and the mock's tests are unchanged.

### The `/__cpd/` routes

| Route | Method | Request | Response |
|---|---|---|---|
| `/__cpd/health` | GET | token | 200 `{"ok":true}` |
| `/__cpd/sessions` | GET | token | 200 `{"ok":true,"sessions":[<LiveSession>...]}` — lead and manager Sessions |
| `/__cpd/sessions/<id>/transcript` | GET | token | 200 `{"ok":true,"session":"<id>","messages":[<TranscriptMessage>...]}`; an unknown id is 404 `{"ok":false,"error":"session not found"}` |
| `/__cpd/sessions/<id>/frames` | GET (SSE) | token | `text/event-stream`; one `data: <SnapshotFrame JSON>\\n\\n` event per snapshot — the current snapshot at once, then a re-sent snapshot (seq + 1) whenever the transcript changes; the client closing ends the stream |
| `/__cpd/sessions/<id>/send` | POST | token; JSON body `{"text":string,"unblocks":string}` | 200 `{"ok":true}`; the body's limits are 10.13's (text 1–20 000 characters, unblocks 1–200, else 400) |

The frames stream's epoch is `"live"` and its `seq` counts the snapshots sent on one connection from 1.

### The TypeScript contract

These interfaces are the pin the test and implementation Jobs build against. Nothing here may drift without amending this document.

**`web/src/transcript.ts`** — the transcript parsers and the snapshot mapping:

```ts
/** One rendered transcript message of a real Session. */
export interface TranscriptMessage {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  /** The tool name of a `tool` message (the tail grammar's `<toolName>`). */
  name?: string;
  text: string;
  /** A failed tool result — the tail grammar's ` (error)` suffix. */
  error?: true;
}

/** Parses a pi session file's JSONL text into messages (rules below). */
export function parsePiSession(jsonl: string): TranscriptMessage[];

/** Parses `cpd session tail` output in the grammar of contract §6.20 (rules below). */
export function parseTailOutput(output: string): TranscriptMessage[];

/** Maps messages onto one fold `snapshot` frame (fold rule 3): `snapshot: { messages }`. Defaults: epoch "live", seq 1. */
export function toSnapshotFrame(messages: readonly TranscriptMessage[], epoch?: string, seq?: number): SnapshotFrame;
```

The pi JSONL parse rules — the real field names of a pi session file (verified against a real file under `~/.pi/agent/sessions`):

1. Parse every line as JSON. A line whose `type` is not `"message"` is skipped (`session`, `session_info`, `model_change`, `thinking_level_change`, `custom`, …).
2. Of a `message` line, keep only `message.role` `"user"` or `"assistant"`; `"system"` is skipped.
3. `message.content` is either a plain string — the text as is — or an array of blocks: join every block with `type === "text"` by its `text` field (an empty join when there is none). `"thinking"` and `"toolCall"` blocks are skipped.
4. The message's `id` is the line's `id` field; one message per line, in file order. The transcript is completed history: never pending.

The tail parse rules — the plain-line grammar of `docs/contract.md` §6.20, what `cpd session tail <id>` prints:

1. A line beginning `user: `, `assistant: `, `system: ` or `tool <toolName>: ` starts a message; the role maps by prefix, and `tool <toolName>:` gives role `"tool"` with `name` = the tool name. The text is the rest of the first line.
2. A following line with no prefix is a source newline of the same message's text — the grammar prints one line per source line with the prefix on the first line only — so continuation lines join with `\n`.
3. A `tool` line ending ` (error)` is a failed tool result: `error: true`, and the marker is consumed — the text is what precedes it.
4. Lines before the first prefixed line are ignored, so a fixture may carry a header label. The grammar carries no ids: messages take the synthetic ids `tail-1`, `tail-2`, … in output order.

`toSnapshotFrame` produces `{ type: "snapshot", epoch, seq, snapshot: { messages } }` — the frame the existing fold's rule 3 reads (`web/src/fold/fold.ts`), so the live transcript enters the Chat through the same rendering engine, unchanged. The snapshot's `messages` are `TranscriptMessage[]`: the fold passes a snapshot's messages through as they are, and the view renders the role badge from them (`system` and `tool` included).

**`web/src/api/live-server.ts`** — the dev-server route handler, framework-free and dependency-injected:

```ts
/** One Session the picker offers. */
export interface LiveSession {
  id: string;
  role: "lead" | "manager";
  runner: "pi" | "durable";
  title?: string;
  state?: string;
}

/** Every external boundary is injected; the handler itself touches nothing real. */
export interface LiveDeps {
  /** The lead and manager Sessions, from `cpd session list --role lead --role manager`. */
  list(): Promise<readonly LiveSession[]>;
  /** A pi Session's session file, as its full JSONL text; undefined when there is none (a durable Session, or an unknown id). */
  readPi(id: string): Promise<string | undefined>;
  /** A durable Session's transcript: the full stdout of `cpd session tail <id>`. Called only for runner "durable", and only as the fallback of the host-socket read (below). */
  tail(id: string): Promise<string>;
  /** One send: `cpd session send <id> <text> --unblocks <reason>`. The only mutation there is. */
  send(id: string, text: string, unblocks: string): Promise<void>;
  /** A pi Session's file mtime in ms, undefined when absent: the frames stream's change signal. */
  stat(id: string): Promise<number | undefined>;
}

/** Handles one request; resolves true when it matched a /__cpd/ route (the caller stops), false to pass through. The node types are `import type { IncomingMessage, ServerResponse } from "node:http"`. */
export type LiveHandler = (req: IncomingMessage, res: ServerResponse) => Promise<boolean>;

export function createLiveHandler(deps: LiveDeps): LiveHandler;
```

The handler is pure wiring: token check first (401 before anything), then the routes of the table above, reading through `deps` only. A pi Session's transcript is `parsePiSession(readPi(id))`; a durable Session's is `parseTailOutput(tail(id))`. The frames stream re-sends its snapshot when the transcript changes: for `runner: "pi"` it polls `stat(id)` and re-reads on an mtime change; for `runner: "durable"` it follows the host-socket preference of the next section — the live socket when a host answers, else re-runs `tail(id)` — and re-sends when the messages change.

`web/vite.config.ts` builds the real `LiveDeps` (it reads the store and the pi sessions directory and runs the cpd CLI) and mounts `createLiveHandler` in the dev server's middleware — only when `CPD_WEB_LIVE=1`; without the flag the config does nothing new. The real `readPi` resolves a Session's pi session file under `~/.pi/agent/sessions` (the per-cwd directories, `<timestamp>_<uuid>.jsonl`); how a cpd Session id locates its exact file is the implementation Job's, wired from the real layout and proven by the fixture.

**`web/src/api/live.ts`** — the browser-side API:

```ts
export interface LiveApi {
  sessions(): Promise<readonly LiveSession[]>;
  /** Subscribes to one Session's snapshot frames; the returned closure closes the stream. */
  subscribe(id: string, onFrame: (frame: SnapshotFrame) => void): () => void;
  /** Sends one message; always posts the fixed one-line reason below. */
  send(id: string, text: string): Promise<void>;
}

/** createLiveApi's injected boundaries: fetch and EventSource are replaced in tests; never real in a test. */
export function createLiveApi(deps: {
  token: string;
  /** Same origin by default (""). */
  baseUrl?: string;
  fetch?: typeof fetch;
  EventSource?: typeof EventSource;
}): LiveApi;
```

- Every request carries the token as `x-cpd-token`.
- `sessions()` GETs `/__cpd/sessions` and resolves the `sessions` array.
- `subscribe()` opens `new EventSource(<baseUrl>/__cpd/sessions/<id>/frames?token=<token>)` — an `EventSource` cannot set headers, so the query carries the token — parses each `data:` event's JSON as a `SnapshotFrame`, and calls `onFrame` with it; the returned closure closes the `EventSource`.
- `send()` POSTs `/__cpd/sessions/<id>/send` with `{"text":<text>,"unblocks":"sent from the cpd web chat"}` — the **fixed one-line reason** every browser send carries, within 10.13's limits; a non-ok response rejects.

**The UI wiring** — `web/src/views/live-chat.tsx`, `web/src/app/App.tsx`, `web/src/main.tsx`:

```ts
/** The live Chat: a session picker, the folded transcript, a send box. */
export interface LiveChatProps {
  /** The live API; absent when the probe failed — then the mock's Chat renders instead. */
  api?: LiveApi;
  token?: string;
}
```

`LiveChat` lists `api.sessions()` in a picker (lead and manager Sessions — the route returns nothing else), subscribes to the selected Session, folds each snapshot frame onto a `ChatView` through the existing fold (rule 3: a snapshot replaces the whole view), renders the messages and sends the prompt's text through `api.send(id, text)`. The chat surface is the **shadcn AI Elements** conversation composition — **`Conversation`, `Message`, `PromptInput`** (or the registry's current ids; the wiring Job pins the exact ids it adds at run time and records them in the PR body) — added as **stock copy-in components** under `web/src/components`, the same pattern as the existing `web/src/components/ui` primitives: no new runtime beyond what the entries declare.

`main.tsx` probes `GET /__cpd/health` with the token before mounting: on `ok` it builds `createLiveApi({ token: DEV_TOKEN })` and hands it to `App` as a new optional **`live?: LiveApi`** prop; otherwise it builds `createMockApi()` exactly as today. On the `/chat` route, `App` renders `<LiveChat api={live} token={token}/>` when `live` is set, and the existing `<Chat …/>` otherwise. The mock, its API and its tests are unchanged.

### The chat surface (shadcn AI Elements)

> Skeleton — filling in the pinned AI Elements surface.

### The durable host-socket read (`chat-host`)

A durable Session's transcript is read from its **host socket** — the real frames of `docs/contract.md` §6.14 — not only by shelling the CLI. The preference rule, pinned: for a durable Session the dev server **connects to the host socket when a live host answers** (a socket file that accepts and completes the handshake) and **falls back to `cpd session tail` otherwise** — no socket file, a refused connect, a refused handshake. A pi Session keeps its session-file read; the browser API is unchanged either way: the SSE `/frames` stream still emits the same `SnapshotFrame`s of `toSnapshotFrame`, and `LiveApi` is untouched. The send path stays `cpd session send`: the socket is **read-only** here, the client sends `hello` and `attach` and nothing else. The activation flag, the token and the security boundary above are unchanged — the host-socket read runs in the same dev server, behind the same token check.

#### The socket

- **Path** `$CPD_HOME/run/host/<session-id>.sock` (`$CPD_HOME` defaults to `~/.cpd`; on this machine `/home/clayton/.cpd/run/host/<id>.sock`). A path over 107 bytes is refused (`socket_path_too_long`) before anything opens.
- **Permissions**: the `run/host/` directory is `0700` owned by the user, the socket file `0600` — same-user trust, no TCP listener anywhere.
- **Framing**: **NDJSON**, one JSON object per line, UTF-8, terminated `\n`. Limits (`pi/durable-host/protocol.ts` `LIMITS`): a client→host line at most 1 048 576 bytes, a host→client line at most 67 108 864, and 5 000 ms from connect to `hello`.

#### The handshake and the frames a reader meets

The host sends nothing first. A read-only reader sends two frames and consumes everything else:

1. → `{"type":"hello","protocol":1,"min_protocol":1,"client":"<name>"}` — `protocol` is the highest version the client speaks, `min_protocol` the lowest; the host picks the highest common (today `1`), or refuses `protocol_mismatch` (details `{"host":[1],"client":[min,protocol]}`) and closes.
2. ← `welcome` — `{"type":"welcome","protocol":1,"host_version":"1","pi_durable":"<v>","session":"<id>","role":"lead"|"manager","epoch":"<uuid>","client_id":"c1","cwd":"<path>","limits":{"client_line":1048576}}`. The `epoch` names the host process.
3. → `{"type":"attach","req":"r1","cursor?":{"epoch":"<uuid>","seq":N}}` — `conversation` absent or `"root"`, anything else refused `unsupported`. The cursor is absent on a first attach (the reconnect rule below sends it).
4. ← `attached` — `{"type":"attached","req":"r1","mode":"snapshot"|"resume","epoch":"<uuid>","from_seq":N,"head_seq":M,"client_id":"c1"}` — then, in `seq` order with none skipped, the **stream frames**: in `snapshot` mode a base `{"type":"snapshot","epoch":"<uuid>","seq":1,"snapshot":<pi-durable SnapshotEvent, passed through unchanged>}` followed by `events` frames `{"type":"events","epoch","seq":2,3,…,"events":[<AgentEvent…>]}` (one commit batch, unchanged); in `resume` mode exactly the `events` frames with `seq > cursor.seq`. Then one `host_state`. **The reader is caught up when it has applied the frame whose `seq` equals `attached.head_seq`.**

- `seq` counts stream frames only (`snapshot` and `events`), starts at 1 per host process, and grows by 1 — a gap is a defect. Frames outside the seq space (`host_state`, `notice`, `status`, `ui_request`, `ui_resolved`) are never replayed.
- Nothing else is sent — no `submit`, `abort`, `detach`, `ping` — so the write-side refusals below never reach this reader.

#### `web/src/api/host.ts` — the socket client

```ts
/** A frame the host sent: protocol.ts's ToClient, the subset a hello/attach-only reader receives. */
export type HostFrame =
  | { type: "welcome"; session: string; role: "lead" | "manager"; epoch: string }
  | { type: "attached"; req: string; mode: "snapshot" | "resume"; epoch: string; from_seq: number; head_seq: number }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "host_state"; state: "idle" | "working" | "waiting" | "blocked" }
  | { type: "error"; code: string; message: string }
  | { type: "bye"; reason: string; reconnect: boolean };

/** The one external boundary: the node socket (node:net in production, replaced in tests). */
export type HostConnect = (path: string) => Promise<net.Socket>;

/** The read-only socket client: one connect + hello + attach per attach() call, frames out, reconnecting per the rule below. */
export interface HostClient {
  /** Attaches to the Session's host socket and calls onFrame per received frame until the returned closure closes the socket. */
  attach(id: string, onFrame: (frame: HostFrame) => void): () => void;
}

export function createHostClient(connect: HostConnect): HostClient;
```

`attach(id)` resolves the socket path from `$CPD_HOME` (`env.CPD_HOME` or `~/.cpd`), connects, sends `hello`, reads `welcome`, sends `attach` — with the saved cursor of the last applied frame when it has one — and hands every parsed line to `onFrame`, `error` and `bye` frames included. It never opens the store and never sends a write.

#### `web/src/translate.ts` — the frame translation

The translation folds host frames onto the **same `TranscriptMessage[]` the CLI path produces** and maps them onto the same snapshot frame through `toSnapshotFrame` (above), so a durable Session read over the socket renders exactly like one read through `cpd session tail`:

```ts
/** The fold state: the transcript so far and the cursor of the last applied frame. */
export interface HostState {
  messages: TranscriptMessage[];
  cursor: { epoch: string; seq: number } | undefined;
}

/** Applies one frame; pure: returns the next state. */
export function applyHostFrame(state: HostState, frame: HostFrame): HostState;
```

The rules are `docs/contract.md` §6.20's fold, in TS, over the wire's unchanged pi-durable shapes:

1. A `snapshot` frame **replaces the whole state**, never merges — the base snapshot, a rebase, and a new epoch's snapshot are the same operation. Its `SnapshotEvent` entries render like the tail's: an entry whose `kind` carries model messages yields one `TranscriptMessage` per `Message` (the role from the message type, the text from its text blocks joined; thinking blocks skipped); an entry with no model messages is bookkeeping and yields nothing. A `toolResult` message gives role `"tool"` with `name` = `toolName` and `error: true` when `isError`.
2. An `events` frame applies its `AgentEvent`s in order: a completed message (`message_end`, or a `run_end` ending an in-flight message) appends its `TranscriptMessage`s; every other event updates nothing the transcript shows. The short-answer rule holds: a message arrives whole, the fold never waits for deltas.
3. The cursor advances only with stream frames: a `snapshot` sets it to `(epoch, seq)`, an `events` frame likewise once its events are applied. `attached` fixes the mode and touches nothing else.
4. `host_state`, `notice`, `status`, `ui_request`, `ui_resolved` and `error` leave the state unchanged.

The frames stream calls `toSnapshotFrame(state.messages, epoch, seq)` — the CLI path's exact frame — and re-sends whenever the fold's messages change.

#### The reconnect rule and the refusals

**Reconnect**: keep the cursor `(epoch, seq)` of the last applied frame. On `bye {"reconnect":true}` (reasons `shutdown`, `restart`, `storage_failed`, `slow_client`) or any socket loss (error or EOF): close, reconnect with back-off (100 ms doubling to a 1 s cap over a 30 s deadline; past it, give up), `hello`, and `attach` with the saved cursor. Same epoch → `resume`: the host sends exactly the frames with `seq > cursor.seq` — **no message lost, none duplicated**. New epoch → `snapshot` mode: the fold replaces its state whole (rule 1 above), so the stream never repeats a line it already sent. `bye {"reconnect":false}` (`detached`, unreachable for a reader that never detaches) ends the stream.

The **refusals** a read-only reader can meet, each an `error` frame with a `code` of `protocol.ts`:

| Code | When | Connection |
|---|---|---|
| `protocol_mismatch` | no common version; details `{"host":[1],"client":[min,protocol]}` | closed |
| `hello_timeout` | no `hello` within 5 000 ms | closed |
| `hello_required` | a frame other than `hello` (or `ping`) first | kept |
| `bad_frame` | a line that is not JSON, not an object, or has no string `type` | kept, line skipped |
| `unknown_type` / `bad_args` | a type the host does not know / a field missing or mistyped | kept |
| `unsupported` | `attach.conversation` other than absent or `"root"` | kept |
| `shutting_down` | a connect after the host shut down | closed |
| `internal` | a host-side error | kept |

Every other code (`not_attached`, `busy`, `frame_too_large`, `refused`, `ui_unknown`, `storage_failed`) answers a write the reader never sends.

#### The fixture

- `web/src/api/host.fixture.ndjson` — the **real captured** frames of a live durable host (four NDJSON lines: `welcome`, `attached`, `snapshot`, `events`; captured 2026-11-01 EDT from the live host of an archived durable Lead), verbatim from the scout's capture `~/.cpd/run/web-chat-live/explore/host-proto-fixture.ndjson`; nothing was redacted because nothing needed it. The host-client and translation tests replay it through a fake `HostConnect` — no test touches a real socket.

### Fixtures

- `web/src/api/pi-session.fixture.jsonl` — a sanitized excerpt of a **real** pi session file (at least one user and one assistant text message; secrets removed), the parser tests' input.
- `web/src/api/tail.fixture.txt` — a `cpd session tail` sample, its first line a comment labeling it **captured** or **derived from contract §6.20** (no durable Session was live when this was written, so the committed fixture is 6.20-derived). Lines before the first prefixed line are ignored by `parseTailOutput`, so the labeled fixture parses as it is. The `chat-host` read's fixture is `web/src/api/host.fixture.ndjson` (above).

### The dev-window security boundary (accepted)

This is a **dev window**, accepted as such: a LAN holder of the dev token may **read any transcript** of any lead or manager Session and **send to any Lead or Manager**. The adapter runs in the dev server process on hs against the real `$CPD_HOME`, read-only for reads. The **only mutation anywhere in the adapter is `cpd session send`** — no store write, no other command, no other route mutates anything. This holds only for the dev window; the production server is a later Plan with its own contract.

