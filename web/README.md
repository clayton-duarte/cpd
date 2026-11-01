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
