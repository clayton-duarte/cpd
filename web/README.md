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
- `GET /__cpd/health` is the probe: `main.tsx` calls it (with the token) before mounting and uses `createLiveApi` **only when it answers `ok`**. When the probe fails — adapter off, dev server restarted without the flag, anything — `main.tsx` builds `createMockApi()` and hands `App` no `live` API; the single-screen chat then shows its explicit no-adapter state (see “The chat surface (shadcn AI Elements)”). The mock's API and tests are unchanged.

### The `/__cpd/` routes

| Route | Method | Request | Response |
|---|---|---|---|
| `/__cpd/health` | GET | token | 200 `{"ok":true}` |
| `/__cpd/sessions` | GET | token | 200 `{"ok":true,"sessions":[<LiveSession>...]}` — lead and manager Sessions |
| `/__cpd/sessions/<id>/transcript` | GET | token; query `limit`, `before` | 200 `{"ok":true,"session":"<id>","messages":[<TranscriptMessage>...],"more":<bool>}`; an unknown id is 404 `{"ok":false,"error":"session not found"}` (“The live frame is the newest turns; history is paged”, below) |
| `/__cpd/sessions/<id>/frames` | GET (SSE) | token | `text/event-stream`; one `data: <SnapshotFrame JSON>\\n\\n` event per snapshot — the current snapshot at once, then a re-sent snapshot (seq + 1) whenever the transcript changes; the snapshot carries the newest turns only, within `SNAPSHOT_TAIL` and `SNAPSHOT_BUDGET_BYTES` (“The live frame is the newest turns; history is paged”, below); the client closing ends the stream |
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
  /** The turn's parts, in source order — every block the parse kept (pi parse rules 3–4, below). */
  parts: readonly TranscriptPart[];
  /** The join of the `text` parts' text — what the plain-text paths carry. */
  text: string;
  /** A failed tool result — the tail grammar's ` (error)` suffix. */
  error?: true;
}

/** One part of a turn, in source order — the kinds the Chat renders (“The Chat column renders a whole turn”). */
export type TranscriptPart =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool"; name: string; title: string; arguments: Record<string, unknown>; result: string; durationMs?: number; error?: true }
  | { type: "image"; dataUrl: string };

/** Parses a pi session file's JSONL text into messages (rules below). */
export function parsePiSession(jsonl: string): TranscriptMessage[];

/** Parses `cpd session tail` output in the grammar of contract §6.20 (rules below). */
export function parseTailOutput(output: string): TranscriptMessage[];

/** Maps messages onto one fold `snapshot` frame (fold rule 3): `snapshot: { messages }`. Defaults: epoch "live", seq 1. */
export function toSnapshotFrame(messages: readonly TranscriptMessage[], epoch?: string, seq?: number): SnapshotFrame;
```

The pi JSONL parse rules — the real field names of a pi session file (verified against a real file under `~/.pi/agent/sessions`):

1. Parse every line as JSON. A line whose `type` is not `"message"` is skipped (`session`, `session_info`, `model_change`, `thinking_level_change`, `custom`, …) — except a `custom_message` line, the cpd record that “A message delivered into a Session is the person's turn” (below) rules.
2. Of a `message` line, keep `message.role` `"user"`, `"assistant"` and `"system"` — a `"system"` line is a notice, and the Chat renders it as a `Marker` (“The Chat column renders a whole turn”, below).
3. `message.content` is either a plain string — one `text` part — or an array of blocks, kept in order as the turn's `parts`: a `text` block is a `text` part (`text` stays the join of the `text` parts, an empty join when there is none), a `thinking` block is a `thinking` part (its text under `thinking`), a `toolCall` block is a `tool` part (`id`, `name`, `arguments`), and an `image` block (`data`, `mimeType`) is an `image` part. No block is skipped.
4. A following line with `message.role` `"toolResult"` is no turn of its own: paired by `toolCallId` with the `toolCall` block it answers, its `text` blocks become the `tool` part's `result`, its `image` blocks become `image` parts of the same turn, and its `durationMs` and `isError` land on the part (`error: true`).
5. The message's `id` is the line's `id` field; in file order. The transcript is completed history: never pending.

The tail parse rules — the plain-line grammar of `docs/contract.md` §6.20, what `cpd session tail <id>` prints:

1. A line beginning `user: `, `assistant: `, `system: ` or `tool <toolName>: ` starts a message; the role maps by prefix, and `tool <toolName>:` gives role `"tool"` with `name` = the tool name. The text is the rest of the first line.
2. A following line with no prefix is a source newline of the same message's text — the grammar prints one line per source line with the prefix on the first line only — so continuation lines join with `\n`.
3. A `tool` line ending ` (error)` is a failed tool result: `error: true`, and the marker is consumed — the text is what precedes it.
4. Lines before the first prefixed line are ignored, so a fixture may carry a header label. The grammar carries no ids: messages take the synthetic ids `tail-1`, `tail-2`, … in output order.

A tail message's `parts` are one `text` part — the plain grammar has nothing else; the full part kinds come from the pi parse (above) and the socket read (below).

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
  /** One page of the transcript: the whole one with no `page`, else the page above `page.before` ({ messages, more }). */
  transcript(id: string, page?: { limit?: number; before?: string }): Promise<{ messages: readonly TranscriptMessage[]; more: boolean }>;
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
- `transcript(id, page?)` GETs `/__cpd/sessions/<id>/transcript` — `page`'s `limit` and `before` as the query — and resolves `{ messages, more }` (“The live frame is the newest turns; history is paged”, below).
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

`LiveChat` lists `api.sessions()` in a picker (lead and manager Sessions — the route returns nothing else), subscribes to the selected Session, folds each snapshot frame onto a `ChatView` through the existing fold (rule 3: a snapshot replaces the whole view), renders the messages and sends the prompt's text through `api.send(id, text)`. The chat surface is the **shadcn AI Elements** composition pinned in “The chat surface (shadcn AI Elements)” below — **Chat**, **Message** and **Prompt Input** on Message Scroller, Message, Bubble, Input Group, Code Block and Empty — added as **stock copy-in components** under `web/src/components/ui`, the same pattern as the existing primitives: no new runtime beyond what the entries declare.

`main.tsx` probes `GET /__cpd/health` with the token before mounting: on `ok` it builds `createLiveApi({ token: DEV_TOKEN })` and hands it to `App` as a new optional **`live?: LiveApi`** prop; otherwise it builds `createMockApi()` and hands no `live`. `App` renders only the single-screen chat (`LiveChat`), which shows its explicit no-adapter state when `live` is absent — the full composition, the message render and that state are pinned in “The chat surface (shadcn AI Elements)” below. The mock's API and its tests are unchanged; it is simply no longer rendered.

### The chat surface (shadcn AI Elements)

`web/src/views/live-chat.tsx` is the whole screen and it is the only thing rendered: no app
header, no nav, no Tree/Plan/Inbox/Logs panes, no mock Chat. It is built from the shadcn **AI
Elements** patterns below, copied in as stock components under `web/src/components/ui` — the
same hand-copied pattern as the existing primitives, no package, no runtime beyond what each
entry declares.

#### The patterns it uses, and the ones it does not

| Pattern | Doc URL | The part it plays |
|---|---|---|
| Chat | https://ui.shadcn.com/docs/ai-elements/base/chat | the thread: Message turns in a Message Scroller |
| Message | https://ui.shadcn.com/docs/ai-elements/base/message | one turn: the person's Bubble, the agent's Markdown |
| Prompt Input | https://ui.shadcn.com/docs/ai-elements/base/prompt-input | the field: an Input Group with a textarea and a send button |

Reasoning, Tool, Chain of Thought, Sources and Suggestion
(`/docs/ai-elements/base/reasoning`, `/tool`, `/chain-of-thought`, `/sources`, `/suggestion`)
are **not used**. `Trace` (`/docs/components/base/trace`), `Marker` (`/docs/components/base/marker`)
and the `item` and `collapsible` primitives **are** vendored: the turn now renders every part,
not only its text (“The Chat column renders a whole turn”, below), and an `ask_user_question`
call renders the registry's Confirmation shape (“An ask renders as a question card”, below).
A durable Session read over the socket pairs its `toolResult`s the same way (“The Chat column
renders a whole turn”, below); the `cpd session tail` fallback keeps its plain `tool` message
with a name and text and no structured input, and nothing carries citations or suggestions.

#### The stock components, their doc pages and their extra deps

| Component | Doc URL | The exports this chat uses | Declares |
|---|---|---|---|
| `message-scroller` | https://ui.shadcn.com/docs/components/base/message-scroller | `MessageScrollerProvider`, `MessageScroller`, `MessageScrollerViewport`, `MessageScrollerContent`, `MessageScrollerItem` | `@shadcn/react` |
| `message` | https://ui.shadcn.com/docs/components/base/message | `Message`, `MessageContent` | — |
| `bubble` | https://ui.shadcn.com/docs/components/base/bubble | `Bubble`, `BubbleContent` | `radix-ui` (already present) |
| `input-group` | https://ui.shadcn.com/docs/components/base/input-group | `InputGroup`, `InputGroupTextarea`, `InputGroupButton` | — |
| `code-block` | https://ui.shadcn.com/docs/components/base/code-block | `CodeBlock`, `CodeBlockHeader`, `CodeBlockTitle`, `CodeBlockActions`, `CodeBlockCopyButton`, `CodeBlockContent`, `CodeBlockLine` | `shiki` (through `hooks/use-highlighted-code`) |
| `empty` | https://ui.shadcn.com/docs/components/base/empty | `Empty`, `EmptyHeader`, `EmptyMedia`, `EmptyTitle`, `EmptyDescription` | — |

**Markdown** is not a registry entry. The docs' `/docs/ai-elements/base/message` imports a
local `@/components/markdown`; this repo keeps its existing `react-markdown` and adds
`web/src/components/markdown.tsx`, a `Markdown` wrapper that renders the reply with
`react-markdown` (plus `remark-gfm`) and maps `pre` to a `CodeBlock` (the `MarkdownPre` shape of
the chatbot template's `components/markdown-code.tsx`), so fenced code gets a language title and
a copy button instead of a bare `<pre>`.

#### Vendoring into THIS repo (verified)

The repo has **no `web/components.json`**. `npx shadcn@latest init` is not the path: it first
fails without a tsconfig path alias, and once it runs (checked with the default base preset) it
rewrites `src/index.css`, `ui/button.tsx` and `lib/utils.ts` and adds an Inter font import — it
takes over the theme. The steps below were run against a copy of this repo's `web/`; with the
new entries imported, `tsc --noEmit` (`pnpm run typecheck`) and `vite build`
(`pnpm run build`) both pass.

1. **Import alias.** Add to `web/tsconfig.json`'s `compilerOptions`:

   ```json
   "paths": { "@/*": ["./src/*"] }
   ```

   No `baseUrl` — TypeScript 7 removed it. Vite 8 reads the tsconfig `paths`, so
   `web/vite.config.ts` needs no alias of its own. The copied entries import
   `@/components/ui/*`, so this must be in place **before** the add.

2. **`web/components.json`.** The AI Elements pages render the `base-rhea` style, which pulls
   `@base-ui/react`. The radix twin carries the same components on the repo's existing
   `radix-ui`, so pin **`radix-rhea`**:

   ```json
   {
     "$schema": "https://ui.shadcn.com/schema.json",
     "style": "radix-rhea",
     "rsc": false,
     "tsx": true,
     "tailwind": { "config": "", "css": "src/index.css", "baseColor": "neutral", "cssVariables": true, "prefix": "" },
     "iconLibrary": "lucide",
     "aliases": { "components": "@/components", "utils": "@/lib/utils", "ui": "@/components/ui", "lib": "@/lib", "hooks": "@/hooks" }
   }
   ```

3. **Add the entries.** From `web/`:

   ```
   npx shadcn@latest add message-scroller message bubble input-group code-block empty -o -y
   ```

   It creates `web/src/components/ui/{message-scroller,message,bubble,code-block,empty}.tsx`,
   `web/src/hooks/use-highlighted-code.ts` and `web/src/lib/highlighter.tsx`, and (`-o`) it
   rewrites the shared primitives `ui/{button,input,textarea,input-group}.tsx` to the same
   `radix-rhea` preset, so the whole `ui/` directory is one style.

   The Chat column's turn rendering (“The Chat column renders a whole turn”, below) adds four
   entries under the same rule — `trace`, `marker`, `item` and `collapsible` (the Confirmation
   shape's `button` and the badges' `badge` are already vendored):

   ```
   npx shadcn@latest add trace marker item collapsible -o -y
   ```

   It creates `web/src/components/ui/{trace,marker,item,collapsible}.tsx`.

4. **The extra deps.** The add writes `@shadcn/react`, `cn` and `shiki` into
   `web/package.json`; `radix-ui` is already there. The Markdown wrapper adds `remark-gfm`
   (`react-markdown` is already a dep). Nothing here needs `@base-ui/react`: the vendored
   entries are the `radix-rhea` twins, which run on the repo's existing `radix-ui` — including
   the `collapsible` and `trace` entries the Chat column's turn rendering adds (above).

#### The full-screen composition

One root fills its column (“The shell never scrolls the document”, in “The resizable
three-column shell”, below); the message list is the only scrolling region and the prompt is a
sibling below it, outside that region, so it never scrolls away:

```tsx
<MessageScrollerProvider>
  <div className="flex h-full min-h-0 flex-col">
    {/* the session picker (below) */}
    <div className="flex items-center gap-2 border-b p-2 text-sm">…</div>

    <MessageScroller className="min-h-0 flex-1">
      <MessageScrollerViewport>
        <MessageScrollerContent className="pb-8">
          {messages.map((message) => …)}
        </MessageScrollerContent>
      </MessageScrollerViewport>
      {/* the authored jump-to-latest control, rendered only while following is released */}
    </MessageScroller>

    {/* the prompt input: a sibling of the scroller, pinned below the viewport */}
    <form className="border-t p-2" onSubmit={…}>
      <InputGroup>
        <InputGroupTextarea placeholder="Send a message" />
        <InputGroupButton type="submit">…</InputGroupButton>
      </InputGroup>
    </form>
  </div>
</MessageScrollerProvider>
```

**Scroll behaviour.** The vendored `MessageScroller` is the layout shell with its own
`autoScroll` off; where the viewport sits in the thread is the authored rule of “The thread
follows its live edge”, below (`web/src/hooks/use-thread-scroll.ts`). No turn carries
`scrollAnchor`.
Enter sends, Shift+Enter adds a line.

#### How one message renders

Every message is a `MessageScrollerItem` keyed by `message.id`, carrying `messageId`. The turn
renders **part by part** — the full contract is “The Chat column renders a whole turn”, below —
and the text is **never** dumped as a bare `{message.text}` (a `system` notice renders as the
collapsed `Notice` row of “A cpd notice collapses to its summary line”, below):

- The **person** is `Message align="end"` with `Bubble align="end" variant="muted"` and the
markdown through `BubbleContent`.
- The **agent** (and every non-`user` role) is `Message align="start"` and its parts render in
order: text through `Markdown` (a fenced block becomes a `CodeBlock` with its language title
and copy button), `thinking` and `tool` as `Trace` rows, an `image` inline, and a `system`
notice as the collapsed `Notice` row of “A cpd notice collapses to its summary line”, below.

#### Sessions, the live wiring and the no-adapter state

The picker, the fold and the send are the existing `LiveApi` wiring unchanged: on mount
`api.sessions()` lists the lead and manager Sessions and the first is selected; the selected id
`api.subscribe(id, onFrame)`s and every `SnapshotFrame` folds onto the view through the existing
`fold` (rule 3: a snapshot replaces the whole view); the composer's text goes out through
`api.send(id, text)`. The picker is a `<select>` over `session.title ?? session.id` with the
`state`/`role` in a `Badge`. The dev adapter and its routes are untouched.

When no live adapter answers — the `/__cpd/health` probe failed, so `main.tsx` hands `App` no
`live` API — the same single screen shows an explicit **Empty** state (an `Empty` block titled
“Live adapter off” with the one line “Start the dev server with `CPD_WEB_LIVE=1` to read a real
Session”), with the picker and the prompt disabled. It does not fall back to the mock Chat.

#### What leaves the render path

`web/src/app/App.tsx` renders the chat and nothing else: the shell header, the nav and the
Tree/Plan/Inbox/Logs routes (and the mock `/chat` route) leave the render path. The cockpit view
files stay where they are; they are simply no longer rendered.

The old AI SDK Elements regression under `web/src/components/ai-elements/`
(`conversation.tsx`, `message.tsx`, `prompt-input.tsx`) is **deleted** — nothing but
`live-chat.tsx` imported it. With it go the deps only those files used: `use-stick-to-bottom`
(`conversation.tsx`), `nanoid` and the `ai` SDK (`prompt-input.tsx`, `message.tsx`).

### The resizable three-column shell

The single-screen chat of “The chat surface (shadcn AI Elements)” becomes the **centre column
of a resizable three-column shell** — **Context | Chat | Inbox** — decided by the User on
2026-11-01. Sizing comes from `react-resizable-panels`, pinned **exactly `4.15.3`**, through
the shadcn **`resizable`** wrapper (`PanelGroup`, `Panel`, `PanelResizeHandle`).

| Column | Filled by |
|---|---|
| Context (left) | the shadcn **`sidebar-03`** block (“a sidebar with submenus”), copied verbatim |
| Chat (centre) | the existing `web/src/views/live-chat.tsx` — no longer unchanged: it has lost its Session picker (the rail selects, next section) and takes the selected `session` from `App` |
| Inbox (right) | the **Inbox rail** — the mail list of the shadcn **`sidebar-09`** block, vendored 1:1 (“The Inbox rail in the right column”, below) — the rail renders the live Inbox (“The live Inbox in the right rail”, below); the Tree/Plan panes stay out of scope (the ui-parity Plan) |

**Sizing.** The Context column **defaults to `20rem`**; dragging its handle resizes it;
**double-clicking its handle restores `20rem`** (checked in a real browser: initial 320 px,
drag to 450 px, double-click back to 320 px). The three panels’ `defaultSize`s over-constrain
the group, so the library scales them down on first layout (~311 px at 1400 px wide); the
`20rem` default is re-applied once on mount through the panel ref to land it exactly.

**Containing the fixed rail.** `sidebar-03`’s desktop rail is `position: fixed`, so it escapes
its panel box: the Context panel gets a `transform` wrapper (establishing a containing block)
with `overflow-hidden`, and `SidebarProvider` gets `--sidebar-width: 100%` so the rail and its
gap spacer match the panel without its content adding a panel scrollbar.

**The shell never scrolls the document** (hs-d834). The Chat column’s root takes its own
column’s height — `flex h-full min-h-0 flex-col` in `web/src/views/live-chat.tsx`, never a
viewport unit (`h-dvh`) — so the Chat can never be taller than its panel. **Both** rails’
`SidebarProvider` carry `h-full min-h-0` (`web/src/app/App.tsx`): the Inbox’s already did, the
Context column’s now does too. And `web/src/index.css` clamps the document — `html, body,
#root { height: 100% }` and `overflow: hidden` on `body` — so the page itself never scrolls and
every scrollable thing is a column’s own list: the rails’ `SidebarContent` and the Chat’s thread
viewport, each with the themed scrollbar (“Themed scrollbars”, below). The reason, plainly: a
viewport-unit height inside a panel, or an unbounded rail, pushes the document taller than the
shell and scrolls the whole page.

Within that column, the Chat thread’s content box reserves **2rem** below its newest line —
`pb-8` on the `MessageScrollerContent` in `web/src/views/live-chat.tsx` (hs-d835) — so rendered
text never touches the prompt input.

**Dark is the default.** The `<html>` element carries `class="dark"` in `web/index.html`.

#### The registry rule: new-york-v4 only

The shadcn registry ships two styles whose files differ by Tailwind version; verified against
the registry JSON (fetched 2026-11-01):

- **`new-york-v4`** — Tailwind v4 syntax (`w-(--sidebar-width)`); use it.
- **`new-york`** — Tailwind v3; its `w-[--sidebar-width]` has no effect under this repo’s
  Tailwind v4 (4.3.3): Tailwind emits `width: --sidebar-width`, an invalid declaration the
  browser drops, so the rail collapses to its content width, with the panel gaining a
  scrollbar (seen and fixed on 2026-11-01).

The `new-york-v4` items already import the unified `radix-ui` package (`Slot.Root`, matching
`web/src/components/ui/button.tsx`) and lucide icons, so the only rewrites when vendoring are
the registry paths (`@/registry/new-york-v4/…`) and `cn`: an item imports `cn` from `"cn"`,
this repo’s is `web/src/lib/utils.ts`, imported **relatively** like every ui component here.
No `@` alias is added to `tsconfig.json` or `vite.config.ts`.

**Vendored paths** — the `sidebar` item, its registry dependencies not already vendored, and
the `sidebar-03` block:

```
web/src/components/ui/sidebar.tsx     (registry: sidebar)
web/src/components/ui/resizable.tsx   (registry: resizable)
web/src/components/ui/breadcrumb.tsx
web/src/components/ui/separator.tsx
web/src/components/ui/sheet.tsx
web/src/components/ui/skeleton.tsx
web/src/components/ui/tooltip.tsx
web/src/hooks/use-mobile.ts
sidebar-03 block: components/app-sidebar.tsx + page.tsx
```

New dep: `react-resizable-panels` exactly `4.15.3`; `radix-ui` and `lucide-react` are already
in `web/package.json`.

### The Context tree in the left rail selects the Chat

The left rail stops being the `sidebar-03` block's sample menu and becomes the **Context tree** — one root row per Context (its manager) and one nested row per Lead — decided by the User on 2026-11-01 from the working dev preview on :5173. Every statement below is measured, not designed.

**The tree.** `web/src/components/app-sidebar.tsx` (the vendored `sidebar-03` block) stops rendering the registry's sample rows and renders the live Context tree:

- The Sessions arrive from the dev adapter's `GET /__cpd/sessions` — the same route and the same lead-and-manager list the old picker read.
- They are grouped by their `context`; each Context's manager (role `"manager"`) is the root row, and each of that Context's leads (role `"lead"`) is a nested `SidebarMenuSub` row under it.
- Contexts sort by name.
- A root row's label is the manager's `name`, falling back to the context; a nested row's label is the lead's `name`, falling back to its id.
- The rail's header carries `cpd` with the Context count beneath it (`1 context`, `4 contexts`, `No contexts` at zero) — never the `sidebar-03` block's `Documentation` / `v1.0.0`.

The rail cannot be collapsed: by the User's decision of 2026-11-01 it no longer renders `sidebar-03`'s `SidebarRail` — the 4px strip at the rail's edge, which an accidental click collapsed the whole rail through — so it has no collapse affordance at all.

**The ids.** Every row is a real `<button>` — not a link — carrying `data-session-id` (the full uuid) and `title` (the same uuid), and showing the id's first 8 characters muted beside the label.

**The selection.** The rail is the Chat's only navigation:

- `web/src/app/App.tsx` holds the selected id, hands the matching `LiveSession` to `LiveChat`, and hands the rail `selectedId` plus `onSelectSession`.
- Clicking a row selects it, and the selected row carries `isActive`.
- Nothing is selected on first render — the Chat shows its no-Session state until a row is clicked.
- The callback is named `onSelectSession`, **not** `onSelect`: `React.ComponentProps<typeof Sidebar>` (a `div`) already declares the DOM's own `onSelect`, and the two collide in TypeScript.

**The picker is gone.** `web/src/views/live-chat.tsx` no longer lists Sessions and no longer renders its `Session` `<select>` or its state/role badges. It takes the Session it is given (prop `session`), subscribes to it exactly as before, and with no Session renders "Pick a manager or a lead in the left rail to see its chat."

**The adapter carries the tree's fields.** `web/src/api/live-server.ts`'s `LiveSession` gains `context` and `name`, which the dev adapter's `realDeps().list()` in `web/vite.config.ts` fills from each `cpd session list` row's `context` and `name`.

### The Inbox rail in the right column

The right column stops being a placeholder and becomes the **Inbox rail** — the mail list of the shadcn **`sidebar-09`** block, decided by the User on 2026-11-01 from the working dev preview (`preview/chat-shell`, `1624176c7`). It is vendored 1:1 into `web/src/components/inbox-rail.tsx` under the same registry rule as the Context rail (**`new-york-v4` only**, above): `cn` imported relatively from `web/src/lib/utils.ts`, no `@` alias.

**What the block is.** `sidebar-09` ("Collapsible nested sidebars", https://ui.shadcn.com/r/styles/new-york-v4/sidebar-09.json) is two sidebars side by side: an icon column — Inbox, Drafts, Sent, Junk, Trash under an Acme Inc header and the sample user — and, beside it, the mail list.

**What is vendored.** Only the block's second sidebar — its header (the title alone) and its mail list. Each row is an anchor and renders the name with its date pushed right (`ml-auto text-xs`), the subject in `font-medium`, then the teaser in `line-clamp-2 w-[260px] text-xs whitespace-break-spaces`, under the block's row classes `flex flex-col items-start gap-2 border-b p-4 text-sm leading-tight whitespace-nowrap last:border-b-0 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground` — rows and classes verbatim.

**What is deliberately not vendored** — the User's decision: the block's first sidebar (the icon column), the `Unreads` switch in the second sidebar's header, and the `Type to search...` input the block puts under the title — so the header carries the title `Inbox` alone and the rail has no input at all.

**The data.** The rail renders the **live Inbox** — the block's ten sample mails (`data.mails`) and the `Mail` interface they need are gone with this PR. The items and the row mapping are the next section's contract (“The live Inbox in the right rail”, below).

**The column.** The Inbox column defaults to `25rem` and is sized and resized like the Context column (above), double-clicking its handle restoring `25rem`. The panel wraps the rail in its own `SidebarProvider` whose `--sidebar-width` is `100%`, so the block's `Sidebar` fills the panel instead of the sidebar's `16rem` default.

**Why 25rem.** The teaser is a fixed `w-[260px]` and the block's own `page.tsx` sets `--sidebar-width: 350px`; at a narrower column the teaser's last characters are clipped mid-word (measured 2026-11-01: 257px of inner width against the 260px teaser). `25rem` gives the row room.

### The live Inbox in the right rail

The rail's data is the **real Inbox**: the open items `cpd inbox list` prints, one row per item, read-only. An item's fields come from that command's JSON row — its `kind`, `label`, `session`, `url` and `age_seconds` are what the row renders.

**Where the items come from.** `web/src/api/live-server.ts` gains a `/__cpd/inbox` route on the dev adapter, guarded by the same token check as every other `/__cpd/` route, returning `{ ok: true, items: [...] }`. Its `inbox()` dependency runs `cpd inbox list` and passes the items through unchanged. `web/src/api/live.ts` gains `inbox()` on `LiveApi`: a plain `GET /__cpd/inbox` with the token header, exactly as `sessions()` reads `/__cpd/sessions`.

**The row's parts, one by one.** The name is the **actor**: for the kind `review-requested` it is `peer review`; otherwise, when the item names a Session, it is that Session's `name` (falling back to the first 8 characters of its id); otherwise it is `cpd engine`. The date is the item's `age_seconds` rendered relative: `just now` under a minute; `1 minute ago`, `N minutes ago` under an hour; `1 hour ago`, `N hours ago`; `Yesterday` from 24 to 48 hours; `N days ago` under a week (the day form is plural only: `Yesterday` holds the 24 to 48 hour range, so a day is never one); `1 week ago`, `N weeks ago` under 30 days; `1 month ago`, `N months ago` after that — the plural of each unit only for N ≥ 2. The subject line is the item's `label`'s first line. The teaser is the label's second line when it has one (the pull request's title, for a `pr-ready`), and the row has no teaser line when it does not.

**What the row links to.** The row is an anchor: its `href` is the item's `url` when the item has one, and `#` when it does not (a `curate-docs` item has none).

**An empty Inbox.** No items renders an empty list under the header — no placeholder copy is invented.

**What does not change.** The rows keep the block's classes, the header keeps its title, the column keeps its `25rem` default, and the rail is read-only: nothing in it snoozes, dismisses, claims or answers an item.

### The Chat column renders a whole turn

The Chat column renders **every part of a turn** — decided by the User on 2026-11-01. Text alone left real Sessions with empty turns: `parsePiSession` kept only text, so thinking, tool calls, their results and images were dropped. One `TranscriptMessage` carries the turn's ordered `parts` (`TranscriptPart`, “The TypeScript contract”, above), and `web/src/views/live-chat.tsx` renders each in order, one `MessageScrollerItem` per turn keyed by its id:

| Part | Renders as | Held by |
|---|---|---|
| `text` (the person's turn) | a `Bubble` with its markdown through `BubbleContent` | `web/src/components/ui/bubble.tsx` |
| `text` (the agent's) | markdown through `react-markdown`; a fenced block through the `Code Block` | `web/src/components/markdown.tsx`, `web/src/components/ui/code-block.tsx` |
| `thinking` | a `Trace` row: closed, its title carries the part's word count; opened, the collapsible carries the thinking's text | `web/src/components/ui/trace.tsx` |
| `tool` | a `Trace` row (below) | `web/src/components/ui/trace.tsx` |
| `image` | inline, an `<img>` from its base64 data URL (`data:<mimeType>;base64,<data>`) | `web/src/views/live-chat.tsx` |
| `system` (the whole message) | a collapsed `Notice` row, not a Bubble and not a markdown turn (“A cpd notice collapses to its summary line”, below) | `web/src/views/live-chat.tsx` |

The `tool` row's title is the command or path the call names — the `arguments`' `command` of a `bash` call, its `path` of a `read`, `edit` or `write` call, else the tool's `name` — with the call's `durationMs` beside the title and the row in its error state when the result failed (`error`). Opened, the row shows the call's `arguments` and its `result` as `Code Block`s. A large result is never truncated: the Code Block carries it whole; only the live frame is bounded (next section).

The pairing behind a `tool` part is measured against a real pi session file under `~/.pi/agent/sessions`: an assistant message's content blocks are `thinking` (its text under `thinking`), `text` and `toolCall` (`id`, `name`, `arguments`), and the tool's result is the **following** line's `message.role` `"toolResult"` (`toolCallId`, `toolName`, `content` of `text` and `image` blocks, `isError`, `durationMs`). `parsePiSession` pairs a `toolCall` with its `toolResult` by `toolCallId`: the part carries the call's `arguments`, and the result's text, `durationMs`, `isError` and `image` blocks land on the same turn — the `toolResult` line yields no turn of its own.

### The thread follows its live edge

Where the viewport sits in the thread is an **authored rule, not the primitive's** (hs-d832). The
rule lives in **`web/src/hooks/use-thread-scroll.ts`**; the vendored `MessageScroller` stays as the
layout shell with its own `autoScroll` **turned off**:

- The thread **opens at its newest turn**.
- While the reader is **following**, every new frame keeps the view at the newest content.
- Following is **released** only when the reader scrolls more than **40% of the viewport height**
  above that newest content — and a released view **never moves on a frame**. No arrival drags it.
- Scrolling back **inside that margin re-engages following automatically**.
- A **jump-to-latest control**, shown only while following is released, re-engages it too.

While following, the viewport carries **`data-autoscrolling`** — the attribute the themed
scrollbar's rule reads to hide the thumb (“Themed scrollbars”, below).

History, kept because both earlier rules were measured and rejected: the registry recipe's anchor
(`scrollAnchor={message.role === "user"}`) settled the person's turn at the top of the viewport and
the primitive's state machine never left `anchored-to-message` for `following-bottom`, so new
output never arrived in view (dropped by the prototype commit `8597fd602` on `preview/chat-shell`).
The primitive's own follow rule was then measured and killed too:
`$CPD_HOME/run/web-chat-bugs/explore/scout-scroll.md` found an **8 px** end-edge released for good
by **one upward scroll** — any wheel tick releases it at all — while the newest agent turn measured
**~950 px against a 767 px viewport**: a reader reading the newest turn is never inside the 8 px
edge, so every later frame was ignored. That is why the rule is authored code, not the primitive's
`autoScroll`.

**The DOM contract the tests pin** (`web/src/views/live-chat.test.tsx` and
`web/src/components/inbox-rail.test.tsx`, a new file): the Chat's scrolling viewport is the
vendored **`[data-slot="message-scroller-viewport"]`** element; the jump-to-latest control is a
button carrying **`data-slot="thread-jump"`**, rendered only while following is released.

### A cpd notice collapses to its summary line

A `system` message — a cpd notice: a Context dump, an engine Event, an Escalation — renders
**collapsed by default** as **one summary line** (hs-d831): the notice's **first non-empty line,
truncated**, a **muted line count** beside it (`24 lines`) and an **expand control**. Expanded,
the whole text renders `whitespace-pre-wrap` inside a box capped at **24rem** (Tailwind
`max-h-96`) that **scrolls with the themed scrollbar** (“Themed scrollbars”, below), and it
collapses again. A notice of **one line** renders as one plain line: **no line count and no expand
control**. Everything else in a turn is unchanged: the person's turns and the agent's answer and
markdown, the Reasoning and Tool rows (already collapsed, closed by default) and the ask card.

The triage that decided it: `$CPD_HOME/run/web-chat-bugs/explore/scout-blocks.md` measured the
worst offender — one `cpd-context` dump rendered whole as a `Marker` reached **299 lines and
51,485 characters** (Session `62b1c19b`, JSONL line 3557) — about six viewport heights for one
transcript row — while the terminal cockpit caps the same content at **12 lines** with no
expansion (`tui/src/history/render.rs:48-50`). The whole-`Marker` rendering of a `system` message
is superseded by this rule.

The DOM contract the tests pin (`web/src/views/live-chat.test.tsx`): the notice's row carries
**`data-slot="notice"`**, its line count **`data-slot="notice-count"`** (text `N lines`, absent
for a one-line notice), its expand control is a button with **`aria-expanded`** (absent for a
one-line notice), and its body is **not in the DOM while collapsed**.

### Themed scrollbars

Every scrolling surface of the shell uses one authored Tailwind v4 utility, **`scrollbar-themed`**
(hs-d830), declared in `web/src/index.css` as:

```css
scrollbar-width: thin;
scrollbar-color: color-mix(in oklab, var(--foreground) 22%, transparent) transparent;
```

plus **`color-scheme: dark` on `.dark`** in `@layer base`, so the scrollbar corner and the native
controls follow the theme. It is applied to the three surfaces that scroll:

- the Chat's scrolling viewport — the vendored `[data-slot="message-scroller-viewport"]`
  (`web/src/components/ui/message-scroller.tsx`);
- `SidebarContent` (`web/src/components/ui/sidebar.tsx`) — both rails scroll there;
- the Code Block's scrolling content (`web/src/components/ui/code-block.tsx`).

The Chat viewport keeps the **`data-autoscrolling` thumb-transparent override**: the thumb hides
while the thread follows its live edge (“The thread follows its live edge”, above). The **Tree and
Plan panes are not in this contract** — the shell does not mount them. Pinned by
`web/src/views/live-chat.test.tsx` (the Chat viewport) and `web/src/components/inbox-rail.test.tsx`
(the rail).

### The Dango theme

The shell's dark theme is the **Dango v3 palette** (hs-d836, approved by the User on the dev preview, 2026-11-02). The shell **ships DARK only**: the palette is written in the **`.dark` block of `web/src/index.css`**, and only `.dark` carries Dango — the light theme (`:root`) is unchanged and is **not** Dango. The shell's `@theme inline` block maps the CSS custom properties to Tailwind's `--color-*` slots; it carries no colours of its own.

Every token is a Dango v3 value, mapped the way the TUI uses the palette. The mapping, one row per CSS custom property:

| CSS custom property | Hex |
|---|---|
| `--background` | `#1c1a20` |
| `--foreground` | `#d8d2cc` |
| `--card` | `#24222a` |
| `--card-foreground` | `#d8d2cc` |
| `--popover` | `#24222a` |
| `--popover-foreground` | `#d8d2cc` |
| `--primary` | `#7aa2ce` |
| `--primary-foreground` | `#1c1a20` |
| `--secondary` | `#2a2730` |
| `--secondary-foreground` | `#d8d2cc` |
| `--muted` | `#2a2730` |
| `--muted-foreground` | `#aba3af` |
| `--accent` | `#2a2730` |
| `--accent-foreground` | `#d8d2cc` |
| `--destructive` | `#e17f6e` |
| `--border` | `#5a504a` |
| `--input` | `#5a504a` |
| `--ring` | `#7aa2ce` |
| `--chart-1` | `#7aa2ce` |
| `--chart-2` | `#8da96e` |
| `--chart-3` | `#c29653` |
| `--chart-4` | `#ca88a3` |
| `--chart-5` | `#67ada7` |
| `--sidebar` | `#1c1a20` |
| `--sidebar-foreground` | `#d8d2cc` |
| `--sidebar-primary` | `#7aa2ce` |
| `--sidebar-primary-foreground` | `#1c1a20` |
| `--sidebar-accent` | `#2a2730` |
| `--sidebar-accent-foreground` | `#d8d2cc` |
| `--sidebar-border` | `#5a504a` |
| `--sidebar-ring` | `#7aa2ce` |

Plus the selection rule, a `.dark ::selection` of **`#5a504a` over `#d8d2cc`**.

**The palette's own rules**, quoted from its source: colour indices **0–8 are read-only and 9–15 clickable**; **0 (bamboo `#5a504a`) draws lines and sticks**; **7 (`#aba3af`) is secondary text**; **8 (`#87808c`) is dim text**; and **15 (shiro `#f3ead9`) is the brightest**. The accent the cockpit's own theme calls `accent` is **index 4, ramune** — `#7aa2ce`, what the table writes to `--primary`, `--ring` and `--sidebar-primary`.

**Source of truth**: the Ghostty theme `~/.config/ghostty/themes/Dango` (v3) and the pi theme `dango.json` (`~/.local/share/cpd/pi/themes/dango.json`). The web theme **MUST agree with the table above**, so the test that pins it can fail when either drifts: the palette is pinned in `web/src/index.css`'s `.dark` block, and the test that pins it is the Dango assertions of `web/src/views/live-chat-shell.test.tsx` alongside the `index.css` clamp test there.

### The markdown a turn renders

A turn's text is markdown, and the Chat parses it as **CommonMark plus GFM**. `react-markdown` alone parses CommonMark only, so a markdown table — which a Lead's status message is largely made of — rendered as a paragraph of pipe characters, and a task list, a strikethrough and a bare URL were lost. The prose parser therefore runs through **`remark-gfm`, pinned `4.0.1` and declared in `web/package.json`**; the mapping lives in `web/src/components/markdown.tsx` (the same file that maps a fenced block onto the Code Block). What a turn's text may contain, measured on the preview branch, 2026-11-01:

- A **table** renders as a real `<table>` with its header row and its cells — a table of 3 rows and 7 cells rendered whole. Tailwind's preflight strips a table's borders and cell padding, so the mapping styles them: the table sits in its own `overflow-x-auto` wrapper, so a wide table scrolls instead of widening the turn; each row carries a rule; each cell gets padding; and a header cell does not wrap.
- A **task list** renders its checkboxes (`type="checkbox"` inputs — 2 measured).
- A **strikethrough** renders as `<del>`.
- A **bare URL** becomes a link (an anchor around it).
- **Inline code** stays inline.
- A **fenced block** renders through the Code Block, with its language as the title and a copy action (`web/src/components/ui/code-block.tsx`).

### A message delivered into a Session is the person's turn

A cpd record arrives in a pi session file as a `custom_message` line, and the parser (`parsePiSession`, `web/src/transcript.ts`) filed every one of them as an engine notice — measured against 1 500+ records across 13 real Sessions of Context building-cpd, the Chat showed the person's own words back as a cpd notice. The rule, measured: the record whose `customType` is `"cpd"` and whose `details.kind` is `"session-message"` (595 of those records) is a message **delivered into the Session** — the person's own turn. Every other record is an engine notice: the `customType`s `cpd-context`, `cpd-plan-summary`, `cpd-plan-decision` and `cpd-skills`, and the `"cpd"` kinds `escalation`, `error`, `pr-feedback`, `job-merged`, `job-failed` and `plan-done`. A record with `display: false` is skipped, whichever kind it carries.

The delivered turn's text is `details.data.text` — the person's words, unwrapped — not cpd's wrapper: the record's `content` is cpd's wrapper `Message from <sender> (<time>): <body>`, and only its `<body>` is the fallback when `details.data.text` is absent. The wrapper's sender may itself carry parentheses (`Message from web-chat-live (14bd6551-…) (13:02 EST): …`), so the `<time>` is the last parenthesised group before the colon.

The delivered turn renders as a `Bubble` aligned to the end (`web/src/views/live-chat.tsx`), with a muted line above it carrying the time cpd stamped and the recorded reason (`details.data.unblocks`, e.g. `sent from the cpd web chat`); the full `Message from <sender> (<time>)` header is the line's `title`.

### The live frame is the newest turns; history is paged

The `frames` route sent the whole transcript as one SSE event — 20 450 029 bytes for a real Session (`97de2471…`) — and Chrome's `EventSource` never fired `onmessage` for an event that large, so the Chat rendered nothing at all. It sends the newest turns only, bounded by two constants in `web/src/api/live-server.ts`:

- `SNAPSHOT_TAIL` — **40** turns.
- `SNAPSHOT_BUDGET_BYTES` — **1 000 000** bytes of serialized frame.

Turns drop oldest first until both bounds hold, and the frame is never nothing: the newest turn always ships, even when it alone exceeds a bound. The budget is the measurement, not a guess — on Chrome, 2026-11-01: an SSE event of 1 MB and one of 2 MB were delivered; events of 4 MB, 8 MB and 20 MB never fired `onmessage`.

History is paged through the transcript route. `GET /__cpd/sessions/<id>/transcript` takes `limit` (capped at **500**) and `before` (the oldest turn id the reader already holds) and answers `{"messages":[<TranscriptMessage>...],"more":<bool>}` — `more` says whether an older page exists; with no `limit` it still answers the whole transcript. The client side is a `transcript(id, page?)` method on `LiveApi` (`web/src/api/live.ts`), a plain GET with the token, exactly like `sessions()`.

The Chat shows a **Load earlier messages** control at the top of the thread (`web/src/views/live-chat.tsx`): it asks for the page above the oldest turn it holds (`before` = that turn's id) and prepends the answer, and it disappears when `more` is false. A frame's turns and the loaded pages are deduplicated by turn id — a turn is held once, whichever path delivered it.

### An ask renders as a question card

An `ask_user_question` call does not render as a JSON blob: it renders as the registry's **Confirmation** shape — the AI Elements pattern of `docs/ai-elements/base/confirmation`, built on the `item` (`web/src/components/ui/item.tsx`) and `button` primitives — in the `tool` part's place, in `web/src/views/live-chat.tsx`. The card is read-only, like the whole adapter: it shows what was asked and what was answered, it asks nothing.

The call's `arguments` hold `questions`: each question's `header`, `question`, `options` of `label`/`description`, and `multiSelect` when several options may be picked. The result's `details.answers` hold, per question: `questionIndex`, `question`, `kind` (`option`, `custom` or `multi`), `answer` — the picked label, the person's typed words, or null — and, for `multi`, `selected` (the picked labels).

The card shows one `Item` per question: the question's `header` as a `Badge`, the `question` text, every option with its `label` and `description`, the taken one's label marked, and a typed answer quoted as the person wrote it. A `Badge` names the outcome: the picked label for an `option` answer, **`N of M picked`** for a `multi` one, **`Typed answer`** for a `custom` one, **`No answer`** when `answer` is null. When the result records a refusal (`cancelled: true` with no answers — measured: the result carries the text “User declined to answer questions”), the card shows **`Refused`** and the refusal text in the destructive colour. A question matches its answer by `questionIndex` — never by the question's text, which pi may restate (the answer's own `question` is pi's restatement and is not used for matching).

### The durable host-socket read (`chat-host`)

A durable Session's transcript is read from its **host socket** — the real frames of `docs/contract.md` §6.14 — not only by shelling the CLI. The preference rule, pinned: for a durable Session the dev server **connects to the host socket when a live host answers** (a socket file that accepts and completes the handshake) and **falls back to `cpd session tail` otherwise** — no socket file, a refused connect, a refused handshake. A pi Session keeps its session-file read; the browser API is unchanged either way: the SSE `/frames` stream still emits the same `SnapshotFrame`s of `toSnapshotFrame`, and `LiveApi` keeps its shape (the `transcript()` page method of the Chat column's contract, above, is the one method added since). The send path stays `cpd session send`: the socket is **read-only** here, the client sends `hello` and `attach` and nothing else. The activation flag, the token and the security boundary above are unchanged — the host-socket read runs in the same dev server, behind the same token check.

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

1. A `snapshot` frame **replaces the whole state**, never merges — the base snapshot, a rebase, and a new epoch's snapshot are the same operation. Its `SnapshotEvent` entries render like the tail's: an entry whose `kind` carries model messages yields one `TranscriptMessage` per `Message` — the role from the message type, the blocks becoming the message's ordered `parts` exactly as the pi parse rules 3–4 (above) — and an entry with no model messages is bookkeeping and yields nothing. A `toolResult` message pairs with the `toolCall` it answers, by `toolCallId`: its text becomes the `tool` part's `result`, its `image` blocks become `image` parts of the same turn, and it carries `name` = `toolName`, `durationMs` and `error: true` when `isError`. (The `cpd session tail` fallback keeps its plain grammar: a `tool` message with a name, text and no structured input.)
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

