# CPD WebUI

The browser interface for CPD. This README covers WebUI onboarding and manual
verification. [CONTRACT.md](CONTRACT.md) owns WebUI-specific behavior;
[the core contract](../docs/contract.md) owns shared domain behavior. The
[runtime profile](../docs/contract.md#37b-the-serve-process-runtime-and-port-supervision)
owns production and UI-only preview behavior; [WebUI access](../docs/architecture/webui-access.md)
owns binding and authentication. The [UI design contracts](src/ui/DESIGN.md)
document presentation details and point to their focused chat, pane and composer
contracts.

Production uses port 3333; the UI-only preview uses 33333 and forwards to the
same host's production engine. The preview opens no store and starts no agent.
CPD.app and Electron are retired. Browser proof belongs in [Manual verification](#manual-verification).

## Test conventions

- Run one file: `timeout 300 pnpm --filter cpd-web-client exec vitest run web/src/<path>.test.ts` (repo-root-relative path). Never the whole suite outside the Gate.
- Only code we wrote is tested. Everything external (`cpd`, `git`, sockets, the file system and its watcher, timers, the network, the browser transport) is an injected boundary, faked in the test. No test uses real time, a real socket, a real file watcher or a real process, and none leaks a handle or timer. Time goes through the injected `Scheduler`.
- UI tests render with `react-dom/client` and `act` from `react` under `// @vitest-environment jsdom`; no testing-library.
- No dependency is added or removed (`package.json` and `pnpm-lock.yaml` stay as they are).

## Manual verification

Use the production or UI-only preview profile and its configured access policy;
old phase checklists are [historical evidence](../docs/research/webui-verification-history.md).
Record the reviewed commit, browser, runner and pass/fail evidence. An empty
checklist is not a passing result.

1. Check startup, connection loss and recovery on the intended host. A preview
   must use that host’s production backend without starting another engine.
2. Switch Sessions and load older transcript pages; verify stable selection,
   scroll position, streaming text, tools and attachments against the contract.
3. Exercise supported model/effort controls, unknown values and refused saves.
4. Answer a normal question and a Plan review. Exercise each approval purpose:
   explicit Allow once authorizes; other outcomes deny.
5. Exercise stopped-session display and the explicit Resume action, including
   failure reporting and the distinction between pi and durable wake behavior.
6. Queue actions while disconnected, then reconnect. Check successful replay,
   visible non-retryable failures, User retry/discard and deduplication.
7. Verify unknown/mismatched protocol warnings and the manual recovery action.

For each failed case, record what was observed and retain the reproduction.
Performance measurements require an agreed workload and recorded results;
archived numerical targets are not evidence that a build meets them.
