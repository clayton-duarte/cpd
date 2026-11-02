# The webui host-socket / WebSocket path, and the cpd and pi SDK exposure: an audit

Sources: `docs/research/web-socket-audit-measurements.md` (all numbers); `pi/durable-host/protocol.ts`, `pi/durable-host/cursor.ts`, `pi/durable-host/ring.ts`, `pi/durable-host/server.ts`, `pi/durable-host/main.ts`; `web/src/api/live-server.ts`, `web/src/api/host.ts`, `web/src/api/live.ts`, `web/vite.config.ts`, `web/README.md`; `docs/architecture/sdk.md` and sibling architecture files; pi's own `rpc.md`/`sdk.md` (1.3.0 release tree).

Status: this is a research document, not a contract. It audits the webui host-socket / WebSocket path and the cpd and pi SDK exposure as the code stands, faithful to the evidence: every protocol/SDK/code claim cites `path:line`, every number cites `docs/research/web-socket-audit-measurements.md` (or is labelled `unmeasured`), and the open questions are named as questions, not decided. It is input for the bridge Workflow; nothing here is implemented.

## 1. The host socket protocol vs webui needs

TODO.

## 2. The bridge shape

TODO.

## 3. The cpd SDK

TODO.

## 4. The pi SDK

TODO.

## 5. Performance, measured

TODO.

## Verdict

TODO.

## Not measured

TODO.
