# cpd web

The browser UI for cpd: a 3-pane cockpit (Context tree, Plan view, Attention Inbox) around a web-native chat that attaches to a cpd Session, either a durable-host session over its unix socket or a pi-runner session over its session JSONL. Phase 1 ("make it work") is a **dev bench**: it runs only under `vite serve`, talks to the browser over Vite `server.ws` custom events (`cpd:c2s` / `cpd:s2c`), and has no auth beyond Vite's localhost bind. Visuals are minimal and functional; the design comes in Phase 3.

**The contract is [`CONTRACT.md`](./CONTRACT.md)**: scope and decisions, domain types, the wire protocol, every module's signatures, behaviours, error codes and the durable-vs-pi capability table. Read it before changing anything. The repo-wide contract is [`docs/contract.md`](../docs/contract.md). Manual browser proof is recorded in [`VERIFY.md`](./VERIFY.md).

## Run the dev server

From the repo root, with the `cpd` binary on `PATH` (or `$CPD_BIN`) and a running cpd (`$CPD_HOME`, default `~/.cpd`):

```
pnpm install
pnpm --filter cpd-web-client run dev
```

Vite serves on <http://localhost:5173>. The cpd plugin (`src/server/plugin.ts`) runs in the dev server only; `vite build` output has no server and is not a supported way to run this UI.

Other scripts: `pnpm --filter cpd-web-client run typecheck`, `pnpm --filter cpd-web-client run build`, `pnpm --filter cpd-web-client run test`.

## Layout (`web/src/`)

| Path | Holds |
|---|---|
| `protocol/` | `domain.ts` (types, `WebError`), `wire.ts` (client/server messages and their parsers), `cockpit.ts` (read models and actions), `runtime.ts` (`Scheduler`, `Transport`, `Disposable`, `SessionBackend`) |
| `state/` | the client store: pure reducers (`transcript.ts`, `store.ts`) and the browser connection (`connection.ts`) |
| `server/cockpit/` | the only code that runs `cpd`: read models, one `cpd events follow`, typed actions |
| `server/durable/` | durable-host socket client, the pi-durable 1.3.0 frame dialect, the durable `SessionBackend` |
| `server/pi/` | session JSONL normalisation, bounded window reads, append watching, the pi `SessionBackend` |
| `server/hub/`, `server/plugin.ts` | one `Connection` per browser, the session registry, the Vite plugin that wires the real boundaries |
| `ui/cockpit/` | Context tree, Plan view, Inbox, 3-pane Shell |
| `ui/chat/` | transcript, tool rows, folds, header, footer |
| `ui/composer/` | composer, attachments (picker, paste, drag-and-drop), tier/model/thinking selector |
| `ui/dialogs/` | `AskCard`, `PlanReviewCard` |
| `ui/` (`App.tsx`, `ChatPane.tsx`, `app.css`), `main.tsx` | the composed app and the browser entry |
| `testing/` | test helpers (`FakeScheduler`, `FakeTransport`, `render`) |

## Test conventions

- Run one file: `timeout 300 pnpm --filter cpd-web-client exec vitest run web/src/<path>.test.ts` (repo-root-relative path). Never the whole suite outside the Gate.
- Only code we wrote is tested. Everything external (`cpd`, `git`, sockets, the file system and its watcher, timers, the network, the browser transport) is an injected boundary, faked in the test. No test uses real time, a real socket, a real file watcher or a real process, and none leaks a handle or timer. Time goes through the injected `Scheduler`.
- UI tests render with `react-dom/client` and `act` from `react` under `// @vitest-environment jsdom`; no testing-library.
- No dependency is added or removed (`package.json` and `pnpm-lock.yaml` stay as they are).
