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

## Server behaviour and pi sessions

- **Every cpd command runs as the User.** The server removes `CPD_SESSION`, `PI_SESSION_ID`, `CPD_SESSION_ID` and `CPD_ATTEMPT_ID` from the environment of the cpd processes it starts. A message sent from the web chat is therefore recorded with no sender (`data.from` null).
- **Which session-messages are user bubbles.** A session-message whose `details.data.unblocks` is `User message from the web chat` is a user bubble, even when `data.from` is a session id. Any other session-message with a sender is a `Message from <id>` fold.
- **Images in pi sessions are accepted.** The server writes each image to `<tmpdir>/cpd-webui-attachments/<sessionId>-<ms timestamp>-<index>.<png|jpg|gif|webp>` and appends a line `[Attached image: <path>]` per image, after a blank line, to the text sent by `cpd session send`. The pi agent's read tool reads local image paths.
- **Plan reviews of pi sessions are answerable.** A pending `cpd_plan_review` is answered from the WebUI. Approve runs `cpd plan decide <review> --approve --plan <plan>`. Reject runs `cpd plan decide <review> --reject --plan <plan>`. Change writes the feedback to `<CPD_HOME>/run/<plan>/feedback-<review>.md` and runs `cpd plan decide <review> --reject --feedback-file <that path> --plan <plan>`. The card closes when pi records the review's new status.
- **Asks of pi sessions are answerable.** A pending `ask_user_question` of a pi session is answered from the WebUI. The answer is written to `<cpdHome>/run/ask/<sessionId>.json`, which the pi extension `pi/ask-user-question` reads. If the extension has not read it after 350 ms, the server sends `Escape` to the session's tmux pane and, for an answer, posts it with `cpd session send` (CONTRACT 11.4).

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
