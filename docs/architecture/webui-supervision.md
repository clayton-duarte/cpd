# WebUI supervision by the engine

Decided by pro-d541 (the production server is a supervised service under the cpd engine, on `0.0.0.0:3333`) and pro-d542 (port in use: lsof with ss fallback, refuse to start, name the process and PID, no fallback). The port map, the browser entry and the origin rule are in `docs/architecture/webui-network-ports.md`. The normative rules are in `web/CONTRACT.md` §15.1, "Engine supervision of the fixed ports". This page states how the engine supervises the two servers.

Status: decided. As of this document the code does not do this. `cpd watch` (`engine/`) starts, probes and restarts no web server, and `preview-web.sh` runs the preview server in a tmux window without supervision (scout: `run/ports-everywhere/explore/scout-engine-supervise.md`). The implementing change brings the code in line.

## The two supervised services

| Service | Port | Command | Owner | Working tree |
|---|---|---|---|---|
| `webui-prod` | 3333, `0.0.0.0` | the production WebUI server, `web/src/server/serve.ts`, serving `web/dist` and `/cpd-ws` | the engine, on every machine, headless included | not fixed by a decision record (open) |
| `webui-preview` | 33333, `0.0.0.0`, `strictPort` | the Vite dev server of the preview worktree (`web/vite.config.ts`) | the engine, when the preview worktree exists | the preview worktree |

The engine runs each service as a child node process.

`cpd install` sets up the permanent supervision on every machine: the production service always, the preview service when the preview worktree exists. A machine without the preview worktree runs no preview service.

## Child process supervision lifecycle

1. **Start.** A service that is not running is started. Before the child is spawned, the port check (below) runs. A refused start leaves the service stopped and raises an `error` Event naming the port, process and PID.
2. **Running.** The engine probes the child on each tick by its pid (`kill(pid, 0)`, then the zombie state). A live child is left alone.
3. **Crash.** A child that is no longer alive has crashed. The engine restarts it, within the restart bound below.
4. **Payload install.** `cpd install` writes the restart marker `run/install-restart.json` (planned restart). The engine restarts each supervised service once after the marker, so the service serves the new payload. The marker is the trigger; the mechanism that restarts a server in place is the implementing change's to specify.
5. **Stop.** A service stopped by the engine is not restarted by the engine.

## Restart bounds

- A crash restarts the service at most **3 times in any 10-minute window**. The fourth crash inside the window latches the service `stopped` and raises an `error` Event.
- A restart after a payload install is not a crash and is not counted in the window.

Assumption: no decision record fixes these bounds for the WebUI servers. The bound adopts the one the engine already applies to durable hosts (`engine/src/watch/hosts.rs`, `MAX_RESTARTS = 3`, `WINDOW = 10 min`). The Lead has not confirmed it.

## Port checking

Before a service starts, its port is checked for a holder.

1. **lsof.** Run `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fpcn`. If it names the holder, use it.
2. **ss.** If lsof names no holder, run `ss -H -ltnp 'sport = :<port>'`, and parse the `users:(("<name>",pid=<n>,...))` field.
3. **Refuse.** If a holder is listening, the service does not start: it exits non-zero and never falls back to another port. The message names the holder by process name and PID: `Port <port> is in use by <process> (PID <pid>). Exiting.` If neither tool names the holder, the message still names the port: `Port <port> is in use by an unknown process. Exiting.`

The production server already has this check: `web/src/server/serve.ts` calls `formatPortInUseMessage` from `web/src/server/port-holder.ts` on `EADDRINUSE`. The engine's check is a requirement on the engine, not yet implemented.

The preview server relies on Vite's `strictPort: true`, which fails on a taken port and never moves. The engine's check applies to the preview service too, so the holder is named the same way.

## Not an access control

Supervision is about process lifetime and ports only. The origin rule that governs `/cpd-ws` on both ports is in `web/CONTRACT.md` §15.1.
