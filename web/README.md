# CPD WebUI

The browser interface for CPD. Start with the [WebUI contract](CONTRACT.md)
for interaction requirements and the [core contract](../docs/contract.md) for
domain behavior. The [runtime profile](../docs/contract.md#37b-the-serve-process-runtime-and-port-supervision)
owns production and UI-only preview behavior; [WebUI access](../docs/architecture/webui-access.md)
owns binding and authentication.

Production uses port 3333; the UI-only preview uses 33333 and forwards to the
same host's production engine. The preview opens no store and starts no agent.
CPD.app and Electron are retired. Browser proof belongs in [VERIFY.md](VERIFY.md).

## Test conventions

- Run one file: `timeout 300 pnpm --filter cpd-web-client exec vitest run web/src/<path>.test.ts` (repo-root-relative path). Never the whole suite outside the Gate.
- Only code we wrote is tested. Everything external (`cpd`, `git`, sockets, the file system and its watcher, timers, the network, the browser transport) is an injected boundary, faked in the test. No test uses real time, a real socket, a real file watcher or a real process, and none leaks a handle or timer. Time goes through the injected `Scheduler`.
- UI tests render with `react-dom/client` and `act` from `react` under `// @vitest-environment jsdom`; no testing-library.
- No dependency is added or removed (`package.json` and `pnpm-lock.yaml` stay as they are).
