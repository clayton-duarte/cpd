# Historical WebUI implementation audit: b8021cd90

Historical evidence only; current runtime requirements are in [the core contract](../contract.md#37b-the-serve-process-runtime-and-port-supervision). This report does not establish present implementation status.

This page states how the checkout of each WebUI server is found, and how the engine supervises the production server on port 3333 and the preview server on port 33333. It describes the code as read at base commit `b8021cd90`. Where the code differs from a decided rule, the difference is listed in "Divergences" below and is not resolved here.

The rules are normative elsewhere: the port map and origin rule in `docs/architecture/webui-access.md` and `web/CONTRACT.md` §15.1; the decided supervision rules (takeover, restart bound, port check) in `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`.

## Ports

| Port | Server | Bind | Source of the port |
|---|---|---|---|
| 3333 | production WebUI server, `web/src/server/serve.ts` | `0.0.0.0` | `PROD_PORT` in `engine/src/watch/webui.rs` and `web/src/server/serve.ts` |
| 33333 | preview Vite dev server | `0.0.0.0`, `strictPort: true` | `PREVIEW_PORT` in `engine/src/watch/webui.rs`; `web/vite.config.ts` |

## Checkout resolution

Two different lookups exist. They do not share a source.

### app-root (read by the shell and `cpd desktop`, not by the engine)

- Path: `<home>/.local/share/cpd/app-root`, where `<home>` is `$HOME` (`core/src/app/mod.rs`, `root_file`). This is the only path the code reads. No `<home>/app-root` file is read anywhere in the code.
- Format: one line, the absolute path of the checkout, with a trailing newline. The reader takes the first line, trimmed.
- Writer: `cpd install` (`core/src/install/mod.rs` calls `write_root` in `core/src/app/mod.rs`), by a temporary file then a rename.
- Readers:
  - `cpd desktop` (`cli/src/app.rs` calls `read_root` in `core/src/app/mod.rs`). A missing or empty file is `app_not_installed`, and the message says to run `scripts/install-cpd.sh` from a checkout.
  - The shell (`web/src/electron/main.ts`, `web/src/electron/update/checkout.ts`).
- **The engine does not read app-root.** `engine/src/watch/webui.rs` contains no reference to it. The supervisor does not know the checkout path from app-root.

### Production server (engine)

- The engine supervises prod only when `<CPD_HOME>/web/src/server/serve.ts` exists. `CPD_HOME` defaults to `~/.cpd` (`core/src/home.rs`).
- It starts `node web/src/server/serve.ts` with the working directory `<CPD_HOME>`.
- So the prod checkout the engine uses is the CPD home root, not the app-root checkout. Under the default `CPD_HOME`, no prod service is supervised unless `CPD_HOME` is itself a checkout. Whether any install sets `CPD_HOME` to a checkout is not verified.

### Preview server (engine)

The engine takes the first candidate that exists (`engine/src/watch/webui.rs`, `preview_worktree_path`):

1. `$CPD_WEB_PREVIEW_WORKTREE`, when set and it exists.
2. The checkout's parent plus `cpd.preview.web`, when a checkout is found.
3. `<CPD_HOME>/projects/cpd.preview.web`
4. `<CPD_HOME>/../cpd.preview.web`
5. `<CPD_HOME>/projects/cpd.preview`
6. `<CPD_HOME>/run/preview-web`

When a candidate is found, the preview service is supervised. It starts `pnpm run dev`. The working directory is set to the candidate; if `web/package.json` exists in the candidate, the working directory is set to `<candidate>/web` instead, so `pnpm run dev` runs the dev script from the web subdirectory.

`cpd install` creates the worktree at `$CPD_WEB_PREVIEW_WORKTREE`, or else at the checkout's parent plus `cpd.preview.web` (`core/src/install/worktree.rs`). With the default `CPD_HOME`, candidates 2 to 5 do not match that path. The engine finds the worktree only through `CPD_WEB_PREVIEW_WORKTREE`, if the engine's environment sets it. Whether it does in a live install is not verified.

## Supervision lifecycle (as coded)

- The supervisor is one step of every engine tick (`engine/src/watch/engine.rs`, the step list that includes `webui::run`). The first tick after an engine starts both supervised services. A new engine has no service state, so a server still holding its port is handled by the takeover rule below.
- On each tick, for each supervised service:
  1. Latched `stopped`: skip.
  2. `run/install-restart.json` present and a child pid known: send `SIGTERM` to the child, then start it again. This is not counted as a crash.
  3. Child pid alive (`kill(pid, 0)`): leave it alone.
  4. Child pid dead: crash path (below).
  5. No child pid: start.
- Start:
  1. Find the port holder with `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fpcn`, falling back to `ss -H -ltnp 'sport = :<port>'`.
  2. If a holder exists, read its command line with `ps -o command= -p <pid>`. It is the same service when the command line contains `web/src/server/serve.ts`, `src/server/serve.ts`, or `serve-prod.mjs` (prod), or `vite` (preview). The same service gets a takeover. Anything else, or an unreadable command line, is refused.
  3. Spawn. Output goes to `run/webui-prod.log` or `run/webui-preview.log`.
- Takeover: send `SIGTERM` to the holder, then poll the port with 100 ms sleeps for 10 iterations. If the port clears, spawn. If it does not, raise a `port_in_use` error Event and do not spawn. Nothing is sent `SIGKILL`.
- Refusal: a non-matching holder raises a `port_in_use` error Event, naming the process and PID, and the message is `Port <port> is in use by <process> (PID <pid>). Exiting.` (or `... by an unknown process. Exiting.` when neither tool names the holder). The message describes the port refusal only; the engine process does not exit.
- Crash bound: at most 3 restarts in a 10-minute window. The window opens at the first crash and resets 10 minutes later. The 4th crash in a window latches the service `stopped` and raises a `webui_crash_loop` error Event. A restart after the install marker does not count.

## Restart marker

- `cpd watch --mark-restart` writes `run/install-restart.json` by rename, for the engine being retired (`engine/src/watch/engine.rs`).
- The supervisor step reads the marker on every tick but does not delete it.
- The engine deletes the marker once, at its own start (`consume_marker_with` in `engine/src/watch/engine.rs`).
- Consequence: a marker that appears while an engine is running makes every tick restart both services until that engine stops. The normal install path writes the marker just before retiring the engine, so the window is short. This is read from the code. It is not reproduced in a live install.

## Divergences

Each row is a decided or documented rule, and what the code does at base `b8021cd90`. None is fixed by this page.

| Rule (source) | Code |
|---|---|
| The engine resolves the preview worktree from app-root and the checkout, `${CPD_WEB_PREVIEW_WORKTREE:-<repo parent>/cpd.preview.web}` (`web/CONTRACT.md` §31.1, `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`) | The engine does not read app-root. It searches the six candidates above; candidate 2 is the checkout's parent when a checkout is found. |
| Prod and preview are supervised from the checkout (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`) | Prod runs from `<CPD_HOME>`, and is supervised only if `<CPD_HOME>/web/src/server/serve.ts` exists. |
| Takeover waits up to 10 seconds (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`, "Takeover of port holders") | The poll is 10 × 100 ms, so about 1 second. |
| The preview holder is the same service when it runs `vite` whose working directory is the preview worktree (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`) | Any command line containing `vite` matches. The working directory is not checked. |
| A port holder that is not the same service is not latched, and is checked again at every tick (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`, citing `docs/contract.md` §3.7) | The holder latches the service `stopped`, and it is not checked again. |
| After a payload install, each supervised service restarts once (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`) | The marker is not deleted by the supervisor, so while it exists each tick restarts both services (see "Restart marker"). |
| The preview is a prod build of the preview branch (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`, air-d96) | The engine runs `pnpm run dev`, the Vite dev server. |
| "`cpd watch` starts, probes and restarts no web server" (`docs/contract.md#37b-the-serve-process-runtime-and-port-supervision`, status line) | Stale: `engine/src/watch/webui.rs` does this, from the tick. |

## Not verified

- A live engine: no process was started. Everything above is read from the source at `b8021cd90`.
- Whether the engine's environment sets `CPD_WEB_PREVIEW_WORKTREE` in a real install.
- Whether any install sets `CPD_HOME` to a checkout.
- That the root `pnpm run dev` script starts Vite on port 33333.
