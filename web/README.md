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

The mock is the default and stays the default (hs-d780). The **dev live adapter** is a dev-mode-only path that reads a **real cpd Session** into the Chat and sends to it through the cpd CLI. It lives entirely in the Vite **dev server layer** plus a browser-side API; the production surface is untouched: `docs/research/web-client-api.md` and the typed `CpdApi` are unchanged, no production API contract changes, and the real HTTP server remains a later Plan. The adapter is runner-agnostic: a **pi** Session is read from its pi session file, a **durable** Session is read through `cpd session tail` — the CLI, not the host socket. Reading a durable Session's real frames over its host socket is the **`chat-host` follow-up**; when it lands, this PR's browser API (`LiveApi`, the snapshot-frame payload) does not change.

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
  /** A durable Session's transcript: the full stdout of `cpd session tail <id>`. Called only for runner "durable". */
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

The handler is pure wiring: token check first (401 before anything), then the routes of the table above, reading through `deps` only. A pi Session's transcript is `parsePiSession(readPi(id))`; a durable Session's is `parseTailOutput(tail(id))`. The frames stream re-sends its snapshot when the transcript changes: for `runner: "pi"` it polls `stat(id)` and re-reads on an mtime change; for `runner: "durable"` it re-runs `tail(id)` and re-sends when the messages differ.

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

### Fixtures

- `web/src/api/pi-session.fixture.jsonl` — a sanitized excerpt of a **real** pi session file (at least one user and one assistant text message; secrets removed), the parser tests' input.
- `web/src/api/tail.fixture.txt` — a `cpd session tail` sample, its first line a comment labeling it **captured** or **derived from contract §6.20** (no durable Session was live when this was written, so the committed fixture is 6.20-derived). Lines before the first prefixed line are ignored by `parseTailOutput`, so the labeled fixture parses as it is.

### The dev-window security boundary (accepted)

This is a **dev window**, accepted as such: a LAN holder of the dev token may **read any transcript** of any lead or manager Session and **send to any Lead or Manager**. The adapter runs in the dev server process on hs against the real `$CPD_HOME`, read-only for reads. The **only mutation anywhere in the adapter is `cpd session send`** — no store write, no other command, no other route mutates anything. This holds only for the dev window; the production server is a later Plan with its own contract.

