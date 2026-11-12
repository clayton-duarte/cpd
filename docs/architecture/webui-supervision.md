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

1. **Start.** A service that is not running is started. Before the child is spawned, the port check (below) runs. A holder that is the same service is taken over first (the takeover section below). A start that the port check refuses, or whose takeover fails, leaves the service unstarted but **not latched**: it is checked again at every tick, and it raises an `error` Event naming the port, process and PID (the rule of `docs/contract.md` §3.7).
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
3. **Refuse.** If a holder is listening and it is not the same service (below), the service does not start and never falls back to another port. The message names the holder by process name and PID: `Port <port> is in use by <process> (PID <pid>). Exiting.` If neither tool names the holder, the message still names the port: `Port <port> is in use by an unknown process. Exiting.`

The production server already has this check: `web/src/server/serve.ts` calls `formatPortInUseMessage` from `web/src/server/port-holder.ts` on `EADDRINUSE`. The engine's check is a requirement on the engine, not yet implemented.

The preview server relies on Vite's `strictPort: true`, which fails on a taken port and never moves. The engine's check applies to the preview service too, so the holder is named the same way.

## Takeover of port holders

Decided by the Lead's answer to Job `webui-adopt-docs` (escalation `webui-adopt-docs-a1-q2`, option (b)). The normative text is `web/CONTRACT.md` §15.1, "Takeover of a same-service port holder". This section states the steps. Status: the code does not do this yet; `webui-adopt-impl` brings it in line.

A holder is **the same service** when its command line names that service:

| Service | Port | Holder that is the same service |
|---|---|---|
| `webui-prod` | 3333 | a command line naming `web/src/server/serve.ts` (the engine's own command) or `serve-prod.mjs` (the hand-started stopgap) |
| `webui-preview` | 33333 | a command line running `vite` whose working directory is the preview worktree |

The command line is read with `ps -o command= -p <pid>`. The lsof output (`-Fpcn`) carries only the process name, which is not enough to tell the same service from another.

When a service is to start, after the port check finds its holder:

1. **No holder.** The service spawns, as before.
2. **The same service.** The engine sends the holder `SIGTERM` and waits up to 10 seconds, checking the port, for it to be free. Once free, the service spawns. If the port is still held after the wait, or the signal is refused, the takeover fails: nothing more is killed (no `SIGKILL`, no adoption), an `error` Event names the holder, and the next tick repeats the steps. The 10-second wait is an assumption, not a decision record.
3. **Any other holder,** or a holder whose command line cannot be read, is not terminated. The service does not start, and the `error` Event names it by process name and PID, or the port alone when neither tool names it (the refusal of the port check, above).

A successful takeover writes one line to `run/watch.log` naming the replaced PID and its command line, and no Event.

A service refused in step 3 or failed in step 2 is not latched: it is checked again at every tick. Its `error` Event follows the rule of `docs/contract.md` §3.7: one per code until it clears, repeated at most once per `watch.error_repeat_seconds`. The crash-loop latch under "Restart bounds" is the only latch.

Not specified here: a health check beyond the child's liveness. A process that is alive but does not serve its port is not detected by these rules.

## Not an access control

Supervision is about process lifetime and ports only. The origin rule that governs `/cpd-ws` on both ports is in `web/CONTRACT.md` §15.1.
