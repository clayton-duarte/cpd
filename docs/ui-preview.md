# Previewing a UI change

A prototype is a quick way to preview a visual change in the live cockpit before building the full feature.

## The loop

(hs-d504, hs-d323, hs-d341, hs-n264)

1. **Prototype.** The Lead adds a throwaway Workflow to the Plan with a Worker Job that makes the visual change. The Workflow is never opened; it is cancelled after the preview.

2. **Load.** From the Workflow's Lead worktree, run:

   ```
   sh scripts/preview-tui.sh on <worktree>
   ```

   This builds the worktree in release mode into `~/.cache/cpd-preview`, then swaps the cockpit's view panes (tree, plan, widgets, statusbar, chat slot) to run the preview build, while the engine and Leads stay on the installed cpd. The status line shows a yellow `preview: <worktree> <sha>` pill (contract §6.1.1). Only the views read the store; the preview binary cannot write.

3. **Ask.** The Lead calls `ask_user_question` to ask the User approve or reject, then does nothing else until the answer arrives. (The prototype exists only to show the change; changes are not built into main this way.)

4. **Off.** The Lead runs:

   ```
   sh scripts/preview-tui.sh off
   ```

   This puts the installed cpd back in the views. Always run it, even after a reinstall's `cpd tui --respawn`, since the preview build can stay in the views.

5. **Real implementation.** An approved change is built through the TDD template on main's code: docs, red tests that pin the exact rows, implementation, then open (hs-d341).

## Previewing the web UI

(pro-d34)

`scripts/preview-web.sh` is the sibling of `scripts/preview-tui.sh`. Where the TUI preview swaps the cockpit's views, the web preview runs the Vite dev server of a preview worktree, so the WebUI can be seen on `localhost:5173` while the installed cpd keeps the engine and the Leads.

```
sh scripts/preview-web.sh on [<worktree>] | off | status | reload [<worktree>]
```

The same four subcommands are run by `cpd app --preview`, `cpd app --reload`, `cpd app --off` and `cpd app --status` (commands.md §14.4a).

- **`on [<worktree>]`** creates the git worktree on branch `preview/web` from `origin/main` when it is missing. The default path is `${CPD_WEB_PREVIEW_WORKTREE:-<repo parent>/cpd.preview.web}`. It links `node_modules` and `web/node_modules` from the repo when they are absent. It starts the Vite dev server, or respawns it when it is running, with `pnpm --dir <worktree> --filter cpd-web-client run dev --host` on port 5173, in the window `web` of the cockpit's work session, without the session identity variables. It builds Electron (`build:electron`) when `<worktree>/web/dist-electron/main.js` is missing, and leaves an existing bundle alone (`reload` always builds). It sets the tmux option `@cpd_web_preview` to `<worktree name> <sha>` and writes `web-state` in `${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}`. It prints the command that points Electron at the server: `CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html pnpm --dir <worktree>/web run electron`. The URL ends in `/electron.html`: bare `http://localhost:5173` is the browser UI (`index.html`), not the Electron renderer. UI/CSS edits hot-reload through Vite React Fast Refresh with no rebuild. Main, preload and server changes still need `preview-web.sh reload` (SIGUSR1). It also starts the watchdog that rolls back a crashing build (see [Crash safety](#crash-safety-pro-d64)), and that watchdog relaunches Electron with `CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html` set, so a relaunch loads the dev server too. It never starts or kills Electron.
- **`off`** respawns the window `web` on the primary checkout, unsets the option `@cpd_web_preview`, removes `web-state` and stops the watchdog.
- **`reload [<worktree>]`** typechecks and builds the worktree's Electron output, then sends SIGUSR1 to the running Electron main process. It exits 1 without building or signalling when the typecheck fails. Details are in [Crash safety](#crash-safety-pro-d64).
- **`status`** prints the worktree and the sha, or `no web preview`.

If loading the dev URL fails (for example the server is down), boot logs the error and loads the built renderer (`renderer/electron.html`) instead of exiting. The failed load is still recorded as a crash for its epoch (see [Crash safety](#crash-safety-pro-d64)).

`preview-web.sh` itself never runs `git checkout`, `git reset` or `git clean`. The Electron sync does a scoped `git checkout HEAD -- <files>` in the preview worktree before it fast-forwards (see [Electron app](#electron-app)). The matching scripts in `web/package.json` are `preview:on`, `preview:off` and `preview:reload`.

## Electron app

(pro-d37, pro-d38, pro-d39)

The Electron app (`web/src/electron/main.ts`) shows the same WebUI in a window. `cpd app` launches it, or reloads the one that runs (commands.md §14.4a; `web/ELECTRON.md`). Four behaviours:

- **Load order (pro-d37).** The window loads only after `startAutoUpdate` returns and `startServices` has run, so the IPC handlers `ghostty:wasm` and `webui:send` exist before the renderer loads. Accepted cost: the window stays blank during a first-run CLI bootstrap; `start.ts` waits for it.
- **Name (pro-d38).** The app is named CPD. `productName` is `CPD` in `web/package.json` and `web/electron-builder.yml`. `app.setName('CPD')` runs before `app.whenReady()`, and the BrowserWindow title is `CPD`. `<title>CPD</title>` is in `web/electron.html` and `web/index.html`. `cpd app` and the `electron` script patch `CFBundleName` and `CFBundleDisplayName` to `CPD` in `Electron.app/Contents/Info.plist` with `plutil` on darwin.
- **Reload on SIGUSR1 (pro-d39, pro-d46).** The main process reloads the WebUI in place on `SIGUSR1`, so a new build applies without a restart. Send the signal with `kill -USR1 <pid>` to the Electron main process. Live patches reload without closing or focusing the window. The handler runs two steps, in this order:
  1. **Server (pro-d46).** The Electron build emits the server entry next to the main process. `web/vite.electron.config.ts` gives Rollup two inputs, `main` (`src/electron/main.ts`) and `server` (`src/server/plugin.ts`), with `entryFileNames: "[name].js"`, so the output directory holds `main.js` and `server.js`. The handler imports `server.js` again from its file URL with a fresh `?t=<Date.now()>` query, which evaluates the entry as a new module. It then disposes the running WebUI services and starts those of the new module: the WebUI IPC handlers, the cockpit service and the attention watch. The PTY handlers are not restarted. The import runs before the dispose, so a failed import leaves the running services in place.
  2. **Renderer.** It reloads the window with `reloadIgnoringCache`, when the webContents is not destroyed.

  If the import throws, the error is logged and the renderer still reloads. The `?t=` query re-evaluates only the `server.js` entry: a module that `server.js` imports, including one the build puts in a shared chunk, keeps its cached instance until the app restarts. `server.js` exists only in an Electron build output.
- **Live sync (pro-d39).** `syncCheckout` (`web/src/electron/update/checkout.ts`) syncs a live preview worktree when `origin/main` advances:
  - It no longer requires the branch `main`: any branch whose HEAD is an ancestor of `origin/main` fast-forwards.
  - It no longer skips a dirty tree.
  - Before the fast-forward it runs `git checkout HEAD -- <files changed in HEAD..origin/main>`. Landed fixes replace their temporary live patches; other live patches survive.
  - A fast-forward that cannot apply (a changed file with a conflicting local edit that survived) is reported `skipped: diverged`.
  - The sync reasons are `fetch-failed` | `diverged`. The `branch` and `dirty` reasons are removed.

## Crash safety (pro-d64)

A crashing WebUI build must not leave the preview dead. The main process records each load of the WebUI as a reload epoch in a health file. A supervisor, the watchdog, rolls the files back to the last build that proved healthy and reloads Electron. Renderer crashes, load failures and a dead process all count.

With a dev URL (`CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html`), the watchdog's relaunch carries the same variable. A failed dev-URL load is recorded as a `did-fail-load` crash, and boot then loads `renderer/electron.html` from the built output, which starts a new epoch. The crash clears before `crashDebounceMs` when that load starts in time, so the watchdog does nothing.

### Health file and crash log

Three files sit in `${CPD_HOME:-<home>/.cpd}/run/`, where `<home>` is the home directory:

| File | Format | Written by |
| --- | --- | --- |
| `preview-health.json` | one JSON object, written atomically (tmp file, then rename) | the main process, through the health recorder |
| `preview-crash.log` | JSON lines, append-only | the main process (crash lines) and the watchdog (event lines) |
| `preview-watchdog.log` | watchdog stdout | the watchdog started by `preview-web.sh on` |

The health record (`web/src/electron/health.ts`) is one of four shapes, chosen by `status`. Times are ISO-8601 strings. `reloadEpoch` and `pid` are positive integers, and the epoch starts at 1.

| `status` | Fields |
| --- | --- |
| `loading` | `status`, `reloadEpoch`, `pid`, `startedAt` |
| `healthy` | `status`, `reloadEpoch`, `pid`, `startedAt`, `healthyAt` |
| `crashed` | `status`, `reloadEpoch`, `pid`, `startedAt`, `crashedAt`, `source`, `error`, `stack` (optional) |
| `exited` | `status`, `reloadEpoch`, `pid`, `startedAt`, `exitedAt` |

`source` is a `CrashSource`: `error-boundary`, `window-error`, `unhandledrejection`, `render-process-gone` or `did-fail-load`. The renderer reports only the first three, as a `CrashReport` (see [Renderer signals](#renderer-signals)).

`health.ts` exports the types `HealthRecord`, `CrashSource` and `CrashReport`, the class `HealthParseError`, and `parseHealth(text: string): HealthRecord`. `parseHealth` throws `HealthParseError` on malformed JSON, an unknown `status` or a wrong field type.

`createHealthRecorder` in the same file takes the files, the pid and the clock, and returns the five writers:

```ts
createHealthRecorder(deps: {
  file: string; crashLog: string; pid: number; now(): Date;
  fs: { readFile(f): Promise<string>; writeFile(f, data): Promise<void>;
        rename(a, b): Promise<void>; appendFile(f, data): Promise<void>;
        mkdir(dir): Promise<void> };
}): { init(): Promise<void>; begin(): Promise<void>; healthy(): Promise<void>;
      crashed(source: CrashSource, error: string, stack?: string): Promise<void>;
      exited(): Promise<void> }
```

Epoch rules:

- `init()` reads the previous file for its epoch. A missing or unparseable file gives epoch 0. The app's own file never blocks boot.
- `begin()` increments the epoch and writes `loading`. If the status is already `loading`, it is a no-op.
- `healthy()` writes `healthy`, unless the current epoch is already `crashed`. A crash is sticky within its epoch.
- `crashed()` writes `crashed` and appends one crash line to `preview-crash.log`.
- `exited()` writes `exited`.
- Writes are serialised through one promise chain, so the file never goes backwards.

A crash line is `{at, reloadEpoch, pid, source, error, stack?}`. A watchdog line is `{at, event, reason, reloadEpoch, pid}`, where `event` is `rollback`, `rollback-failed`, `rollback-skipped` or `relaunch`.

### Renderer signals

- **`ErrorBoundary`** (`web/src/ui/ErrorBoundary.tsx`) is a class component: `ErrorBoundary({name: string; onCrash?: (r: CrashReport) => void; fallback?: (ctx: {name; error; retry(): void}) => ReactNode; children})`. On a throw it calls `onCrash({source: 'error-boundary', error: message, stack})` once per caught error. It renders the default fallback: `role='alert'`, the name, the message, and a `Retry` button that clears the error and renders the children again.
- **Crash signals** (`web/src/electron/crash-signals.ts`): `installCrashSignals(target: {addEventListener, removeEventListener}, onCrash): {errors(): number; dispose(): void}`. An `error` event is reported as source `window-error`. An `unhandledrejection` event is reported as source `unhandledrejection`, with the reason's message or `String(reason)`. `errors()` counts every one.
- **`HealthBeacon`** (`web/src/ui/HealthBeacon.tsx`): `HealthBeacon({root: () => HTMLElement | null; errors: () => number; onHealthy: () => void})`. It renders nothing. Its mount effect calls `onHealthy` once, and never again on re-render, when `root()` has at least one child element and `errors()` is 0.
- **`Root`** (`web/src/ui/Root.tsx`): `Root(props: AppProps & {onCrash?; onHealthy?; errors?; reload?})`. It is an `ErrorBoundary` named `App` around `App`. Its fallback is a full-window alert with a `Reload` button that calls `reload` (default `window.location.reload`). After `App` it renders a `HealthBeacon` with `root` set to `document.getElementById('root')`.
- **Panes.** `App` wraps `ContextTree`, `PlanView`, `ChatPane` and `Inbox` each in its own `ErrorBoundary`, named `ContextTree`, `PlanView`, `ChatPane` and `Inbox`. `App` takes an optional `onCrash` and passes it down, so one pane's throw leaves the others mounted.
- **Electron mount.** `mountWebui(root, bridge)` in `web/src/electron/webui.tsx` installs the crash signals on `window` with `bridge.reportCrash`. It renders `Root` with `onCrash={bridge.reportCrash}`, `onHealthy={bridge.reportHealthy}` and `errors={signals.errors}`. `web/src/main.tsx` (Vite dev, no Electron) renders `Root` with none of them.
- **Bridge** (`web/src/electron/health-bridge.ts`): `interface HealthBridge {reportHealthy(): void; reportCrash(info: CrashReport): void}` and `createHealthBridge(ipc: {send(channel: string, ...args: unknown[]): void}): HealthBridge`. `preload.ts` spreads it into `window.cpdElectron`, as it does `UpdateBridge`. The channels in `web/src/electron/channels.ts` are `healthOk` = `'health:ok'` and `healthCrash` = `'health:crash'`.

`CrashReport` is the renderer-to-main payload: `{source: 'error-boundary' | 'window-error' | 'unhandledrejection'; error: string; stack?: string}`.

### Main wiring

`attachHealth(deps: {recorder; webContents: {on(event, listener)}; ipcMain: {on(channel, listener)}; process: {on(signal, listener)}; app: {on(event, listener)}})` in `web/src/electron/health-main.ts` maps events to the recorder:

| Source | Event | Recorder call |
| --- | --- | --- |
| attach | none | `recorder.begin()` |
| webContents | `did-start-loading` | `recorder.begin()` |
| process | `SIGUSR1` | `recorder.begin()` |
| ipcMain | `health:ok` | `recorder.healthy()` |
| ipcMain | `health:crash` | `recorder.crashed(report.source, report.error, report.stack)`. A payload that is not a valid `CrashReport` is logged with `console.error` and ignored. |
| webContents | `render-process-gone` | `recorder.crashed('render-process-gone', details.reason)`, unless `details.reason` is `clean-exit` |
| webContents | `did-fail-load` | ``recorder.crashed('did-fail-load', `${errorDescription} (${errorCode}) ${validatedURL}`)``, only when `isMainFrame` is true and `errorCode` is not `-3` (aborted) |
| app | `before-quit` | `recorder.exited()` |

`main.ts` calls `attachHealth` where it first receives the `BrowserWindow`, at `startUpdate(window)`, before the load. It passes the real `node:fs/promises`, `CPD_HOME` (default `<home>/.cpd`) and `process.pid`.

### Watchdog

The watchdog is a supervisor that runs outside Electron. `preview-web.sh on` starts it (see [preview-web.sh](#preview-websh)). Its core is `createWatchdog(ports, options?)` in `web/src/electron/watchdog.ts`, which returns `{tick(): Promise<void>}`. The core reaches the outside world only through its ports:

- `now(): number`
- `readHealth(): Promise<HealthRecord | null>`: `null` when the file is missing. A malformed file throws `HealthParseError`.
- `pidAlive(pid: number): boolean`
- `signalReload(pid: number): void`: sends SIGUSR1.
- `relaunch(): Promise<void>`
- `restore: RestorePoints`
- `log(entry: object): Promise<void>`

Defaults, all overridable by a flag:

| Option | Flag | Default | Meaning |
| --- | --- | --- | --- |
| `stabilizeMs` | `--stabilize-ms` | 3000 | healthy time before a candidate is promoted |
| `crashDebounceMs` | `--crash-debounce-ms` | 1000 | crashed time before a rollback |
| `loadingGraceMs` | `--loading-grace-ms` | 10000 | loading time, from `startedAt`, before a rollback |

The script calls `tick()` every 500 ms. `tick` never sleeps.

Rules:

1. **First sight.** The first time an epoch is seen (any status; the first tick, or `reloadEpoch` changed), the watchdog calls `restore.captureCandidate()`, which captures the files as they were loaded. It replaces any older candidate.
2. **Promotion.** A candidate is promoted (`restore.promoteCandidate()`) when its epoch has stayed `healthy` for `stabilizeMs` since first seen healthy. It is never promoted when the epoch crashed or was loading too long.
3. **Rollback trigger.** Any of: status `crashed` for `crashDebounceMs`; status `loading` for `loadingGraceMs` from `startedAt`; or the pid is dead while the status is not `exited`. A crash that clears (a new epoch appears) before the debounce does nothing. `exited` never triggers.
4. **Rollback.** When `restore.hasLastGood()`, the watchdog logs `rollback` with `reason`, `reloadEpoch` and `pid`, then calls `restore.restoreLastGood()`, then `signalReload(pid)` when the pid is alive, else `relaunch()` (logged as `relaunch`). A restore error is logged as `rollback-failed` and does not stop the watchdog. Without a last good, it logs `rollback-skipped` once per epoch.
5. **One attempt per restore point.** When the epoch that follows a rollback also triggers a rollback, the watchdog logs `rollback-skipped` with reason `already rolled back`, and does nothing more until a candidate is promoted.
6. **Malformed health.** A malformed health file is logged once per distinct text and ignored.

The supervisor is `scripts/preview-watchdog.mjs`:

```
node scripts/preview-watchdog.mjs --worktree <dir> --home <cpd-home> [--stabilize-ms n] [--crash-debounce-ms n] [--loading-grace-ms n] --relaunch <cmd> [args...]
```

- `--relaunch` comes last. It takes the command and every argument after it.
- It imports `./watchdog.ts`, `./restore-points.ts` and `./health.ts` as siblings. `preview-web.sh` copies the four files into one frozen directory outside the worktree and outside `dist-electron`, so live patches cannot change a running supervisor.
- Real ports: `node:fs/promises`; `execFile` from `node:child_process` for git; `process.kill(pid, 0)` for `pidAlive`; `process.kill(pid, 'SIGUSR1')` for `signalReload`. `relaunch` is a detached spawn of the `--relaunch` command, with cwd set to the worktree and stdio to `preview-watchdog.log`.
- Ticks run every 500 ms and never overlap. The supervisor exits 0 on SIGTERM.
- The relaunch never builds, because a build would overwrite the restored `dist-electron`. `preview-web.sh` passes `pnpm --dir <worktree>/web exec electron .`.

### Restore points

`web/src/electron/restore-points.ts` exports `createRestorePoints(deps: {worktree; exec(file, args, opts: {cwd; env}): Promise<{stdout}>; fs: {cp; rm; rename; exists}; ref?})`. It returns `{captureCandidate, promoteCandidate, hasLastGood, restoreLastGood, discardCandidate}`. `exec` rejects on a non-zero exit. `ref` defaults to `refs/cpd/preview-last-good`.

The directories, all under `<worktree>/web/`:

| Path | Holds |
| --- | --- |
| `dist-electron` | the build the preview runs |
| `dist-electron.candidate` | the candidate: the files as loaded, at first sight of an epoch |
| `dist-electron.last-good` | the last promoted build |
| `dist-electron.restore` | staging for the swap back to last good |

Every git run has cwd = worktree and `GIT_INDEX_FILE=<git-dir>/cpd-preview-index`, where `<git-dir>` comes from `git rev-parse --absolute-git-dir`. The code never runs `checkout`, `reset`, `clean` or `stash`, and never runs any command that moves HEAD or rewrites the real index. `checkout-index` writes files from the temporary index only.

- **`captureCandidate`** runs `git read-tree HEAD`, `git add -A` and `git write-tree` into the temporary index, and keeps the tree SHA in memory. It then copies `dist-electron` to the candidate dir, replacing an old one.
- **`promoteCandidate`**, when the candidate tree differs from the ref's tree (`git rev-parse --verify -q <ref>^{tree}`), runs `git commit-tree <tree> -p HEAD -m 'cpd preview last good'` and then `git update-ref <ref> <commit>`. The author and committer are `cpd-preview-watchdog` with email `cpd-preview-watchdog@localhost`, set through `GIT_AUTHOR_NAME`, `GIT_COMMITTER_NAME` and the email variables. Then it removes the last-good dir and renames the candidate dir to it.
- **`restoreLastGood`** copies the last-good dir to staging, removes `dist-electron` and renames staging to `dist-electron`, an atomic swap. It then builds a temporary index of the current files (`read-tree HEAD`, `add -A`, `write-tree`). It lists the files added since the last good tree with `git diff-tree -r -z --name-only --diff-filter=A <last-good-tree> <current-tree>` and removes them, joined to the worktree. Last, it runs `git read-tree <ref>` into the temporary index and `git checkout-index -a -f`.
- **`hasLastGood`** is true when the ref resolves and the last-good dir exists.

Why the real index is never touched: the real index holds what the User has staged. `git add -A` into it would stage every build file, and `read-tree` would replace the staging. The temporary index gives git the same tree-building with no effect on staging. Why HEAD is never moved: the last-good commit has HEAD as its parent, and `update-ref` moves only `<ref>`.

Why a candidate, then a promotion: the candidate is taken at first sight of an epoch, from the files the renderer loads, not at healthy time (assumption A3). Files edited after a reload but before the load never enter last good. Promotion waits until the epoch proves healthy, so a build that crashes never becomes last good.

### preview-web.sh

Usage: `sh scripts/preview-web.sh on [<worktree>] | off | status | reload [<worktree>]`.

- **`reload [<worktree>]`** uses the worktree from line 1 of `web-state`, else the existing default. It runs `pnpm --dir <worktree> --filter cpd-web-client run typecheck`. On failure it prints the failure and exits 1, before any build or signal. It then runs `pnpm --dir <worktree> --filter cpd-web-client run build:electron`. It reads the pid from `${CPD_HOME:-$HOME/.cpd}/run/preview-health.json` (`sed` on `"pid":N`, no `jq`) and runs `env kill -USR1 <pid>`. With no pid, or when `env kill -0 <pid>` fails, it prints a message and exits 1.
- **`on`** also installs the frozen watchdog. It copies `scripts/preview-watchdog.mjs` and `web/src/electron/{health,watchdog,restore-points}.ts`, from the repo that holds the script, into `${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}/watchdog/`. It starts the watchdog detached with `nohup`, appends its stdout and stderr to `${CPD_HOME:-$HOME/.cpd}/run/preview-watchdog.log`, and writes its pid to `${CPD_PREVIEW_DIR}/watchdog.pid`. A previous watchdog is stopped first, with SIGTERM through `env kill`. A dead pid in the file is removed.
- **`off`** stops the watchdog and removes the pid file. With no watchdog running it still succeeds.

### Limits

- **One preview per machine.** There is one health file, `preview-health.json`, under `${CPD_HOME:-<home>/.cpd}/run/`. Two previews on one machine write to it together.
- **Non-atomic vite build.** `build:electron` writes `dist-electron` in place. A capture that runs while a build writes can take a mix of old and new files, and a last good copied from that candidate keeps the mix.
- **Shared refs in linked worktrees.** `refs/cpd/*` is shared by every linked worktree of a repository. `refs/cpd/preview-last-good` is one ref, so a promotion in one worktree replaces the last good of the others, and a rollback in one worktree can restore a tree that another worktree built.
- **A rollback restores files but not HEAD.** `dist-electron` is gitignored, so the candidate tree holds the rest of the worktree. A rollback writes every file of the last-good tree into the worktree (`checkout-index -a -f`) and removes files added since. Edits to tracked files made after the capture are overwritten. HEAD and the real index stay where they were, so `git status` shows the restored files as changed, and the restored build need not match HEAD.

## Limits

A preview costs one release build of the worktree. The engine's pi chat (pi/lib/render.ts) is not reached by the view, so messages there don't appear in the preview. A branch missing a breaking migration of main is refused by the install guard; merge main first.
