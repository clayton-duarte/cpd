# cpd serve: the production server and the UI-only preview

Status: **decided** by pro-d566 (no containers), pro-d567 (separate hosts, no login, the browser is the official interface) and pro-d570 (the preview on 33333 is UI-only and proxies the host's production engine on 3333; it has no preview store, binary or engine). The ports and the browser entry are pro-d541, pro-d542 and pro-d552 (`docs/architecture/webui-network-ports.md`).

**As of this document the code does not do this.** There is no `cpd serve` command: the `Command` enum in `cli/src/cli.rs` has no serve variant. The engine (`cpd watch`) supervises a separate Node server, `web/src/server/serve.ts`, on `0.0.0.0:3333` (`engine/src/watch/webui.rs:210-212`, `:321`), and a Vite dev server on `0.0.0.0:33333` with `strictPort` (`web/vite.config.ts:105`). The implementing change brings the code in line. Where this page and another page disagree, the later decision stands (pro-d570 supersedes the preview statements listed in §7).

The contract of the command is in `docs/commands.md` §12a. The runtime and port invariants are in `docs/contract.md` §3.7b.

## 1. Decisions in one place

1. **One production process per host.** `cpd serve` runs the engine's tick loop (contract §3.7) and the WebUI server (the built browser build over HTTP, and the WebSocket `/cpd-ws` to the core engine, contract §5.8) in one OS process, bound to `0.0.0.0:3333`.
2. **No containers** (pro-d566). Each host runs the cpd built from its own checkout, installed by `cpd install` (commands.md §14.4). Nothing is packaged as an image.
3. **Separate hosts, no login** (pro-d567). A host's engine serves its own store only. There is no token, no TLS and no login. The Origin rule of `web/CONTRACT.md` §15.1 limits which pages may connect; it is not access control.
4. **The browser is the official interface** (pro-d567). The WebUI served on 3333 is the interface. CPD.app is not retired by this page: that is out of this page's scope.
5. **The preview on 33333 is UI-only** (pro-d570). It serves one branch's WebUI and forwards `/cpd-ws` to the production engine on 3333 of the same host. It opens no store, runs no `cpd` binary and runs no engine (§5).
6. **Ports are fixed.** 3333 and 33333 are never configurable and never move. A server that finds its port held exits non-zero and names the holder (pro-d542; §4).

## 2. Architecture

```
browser on the LAN or localhost
  │  http://<host>:3333/             built WebUI (index.html, main.tsx)
  │  ws://<host>:3333/cpd-ws         core engine WebSocket (contract §5.8)
  ▼
cpd serve                             one process, bound 0.0.0.0:3333
  ├─ WebUI: static files of the built browser build
  ├─ /cpd-ws: WebSocket to the core engine
  └─ engine: tick loop (contract §3.7), store $CPD_HOME/cpd.db, tmux, pi

browser on the LAN or localhost
  │  http://<host>:33333/          the branch's WebUI (preview)
  │  ws://<host>:33333/cpd-ws      forwarded unchanged ─┐
  ▼                                                     │
preview server                        one process,      │
  (no store, no cpd binary,            bound 0.0.0.0:33333
   no engine)                                           │
  └─ proxy /cpd-ws ─────────────────────────────────────┘
        to 127.0.0.1:3333/cpd-ws on the same host (the production engine)
```

### 2.1 The production process

- **One per host.** The process holds the engine lock `locks/watch.lock` (contract §6.6, commands.md §12) and binds 3333. A second `cpd serve` on the host is refused by whichever of the two it reaches first. Either refusal leaves the running process alone.
- **Same engine.** It runs the engine exactly as `cpd watch` does: the same steps, the same store, the same migration and refusal rules (contract §3.7, §4.5). Nothing about the engine changes because it shares a process with the WebUI.
- **The store** is the host's `$CPD_HOME/cpd.db`, read and written only by this process and the cpd commands it serves (contract §4.2, §4.3).
- **The WebUI assets** are the built browser build of the installed checkout (`web/dist`, entry `index.html` which loads `main.tsx`; `docs/architecture/webui-network-ports.md`). Assumed, not decided: the process reads them from the installed checkout and does not embed them in the binary (§6, open point 3).

### 2.2 The preview process

- **UI only.** It serves the WebUI of one branch's worktree, as a Vite dev server with `strictPort` (the mechanism of `docs/ui-preview.md`). It is the only thing on 33333.
- **Proxy, not engine.** Its `/cpd-ws` endpoint forwards each WebSocket to `127.0.0.1:3333/cpd-ws` on the same host, and forwards the `Origin` header unchanged. The production process then applies its usual Origin rule (`web/CONTRACT.md` §15.1); `http://<host>:33333` is an accepted origin there.
- **No engine of its own.** If the production process is down, the preview's WebSocket fails and the page shows the disconnected state. The preview never starts an engine to stand in for it: a second engine would be a second store writer.

## 3. Current code and the gap (verified by reading, not run)

| Target | Today | Source |
|---|---|---|
| `cpd serve`, one process: engine and WebUI on 3333 | no `serve` command; the engine is `cpd watch` and runs a child | `cli/src/cli.rs` `Command` enum (no serve variant); `engine/src/watch/webui.rs:210-212` |
| The WebUI server is in the production process | a separate Node process, `web/src/server/serve.ts`, run as a script by the engine; it serves `web/dist` on `0.0.0.0:3333` | `engine/src/watch/webui.rs:321`; `web/src/server/serve.ts:15-16`, `:43-54` |
| Port 3333 is checked before start, the holder named | the engine checks with `lsof`, falling back to `ss`, and refuses on a holder | `engine/src/watch/webui.rs:57-111`; the message contract in `docs/architecture/webui-supervision.md` |
| Preview on 33333 is UI only, `/cpd-ws` forwards to 3333 | the Vite dev server on `0.0.0.0:33333`, `strictPort: true`, with its own `/cpd-ws` backend on its httpServer | `web/vite.config.ts:105`; `web/src/server/plugin.ts:378-384` |
| The preview has no store and no engine | the Vite process runs the WebUI backend itself (`cpdWebPlugin`), which serves the cockpit from the cpd CLI | `web/src/server/plugin.ts:355-397` (scout, read not run) |

## 4. Ports and supervision

- **Fixed ports.** Production 3333 and preview 33333, both on `0.0.0.0`. No config key, no flag, no fallback port (pro-d541, pro-d542).
- **Port held.** A server that cannot bind its port exits non-zero, with a message that names the holder by process name and PID, or says the holder is unknown. It never tries another port. The message texts are in `docs/architecture/webui-supervision.md`, "Port checking".
- **Permanent supervision.** `cpd install` sets up permanent supervision of the production process on every host, and of the preview when its worktree exists (`docs/architecture/webui-network-ports.md`). The mechanism is open (§6, open point 1).
- **Restart bound.** A crash restarts the process at most 3 times in any 10-minute window; the fourth latches it stopped and raises an `error` Event. This is the bound `docs/architecture/webui-supervision.md` adopts from the engine's durable hosts. The Lead has not confirmed it for the WebUI.

## 5. The preview: what it may and may not do

It **may**:

- serve the WebUI of one branch on 33333, with `strictPort`, failing loud when 33333 is held;
- forward `/cpd-ws` to `127.0.0.1:3333` on its own host, unchanged, including the `Origin` header.

It **may not**:

- open `$CPD_HOME`, `cpd.db` or any file under `run/`;
- start `cpd`, the engine, a pi session or a tmux server;
- hold `locks/watch.lock`;
- forward to any host other than its own `127.0.0.1:3333`.

A preview on one host and a production engine on another is not a configuration this page supports.

## 6. Open points (for the Lead)

1. **The supervisor of `cpd serve`.** Which mechanism restarts it on a crash and at install (`cpd install` is said to set it up; the mechanism is not chosen here).
2. **`cpd watch` as a name.** Whether `cpd watch` stays as an alias of `cpd serve`, or is removed. `docs/commands.md` §12a states `cpd serve` alone.
3. **Asset source.** The process is assumed to read `web/dist` from the installed checkout rather than embed it in the binary.
4. **Proxy mechanism.** Whether the preview's proxy is the Vite server's own proxy or a small forwarder. The page requires only the forwarding and the unchanged Origin.

## 7. Earlier statements this page supersedes

Recency wins (pro-d570 is later than the statements below). They are not edited here, because only three files are in this Job's scope; each needs a follow-up edit.

- `docs/ui-preview.md`, "WebUI ports": the preview's `/cpd-ws` is the Vite dev server's own backend on 33333. Under pro-d570 it is a forward to the production engine.
- `docs/architecture/webui-network-ports.md`, "Preview on 33333" row: the preview is run by `preview-web.sh` with its own backend. Under pro-d570 it is UI only.
- `docs/contract.md` §5.5, "The WebUI transport": "the Vite dev server hosts the same endpoint same-origin on its httpServer". Under pro-d570 the preview forwards to 3333 and hosts no engine endpoint.
