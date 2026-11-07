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

The same four subcommands are run by `cpd desktop --preview`, `cpd desktop --reload`, `cpd desktop --off` and `cpd desktop --status` (commands.md §14.4a).

- **`on [<worktree>]`** creates the git worktree on branch `preview/web` from `origin/main` when it is missing. The default path is `${CPD_WEB_PREVIEW_WORKTREE:-<repo parent>/cpd.preview.web}`. It links `node_modules` and `web/node_modules` from the repo when they are absent. It starts the Vite dev server, or respawns it when it is running, with `pnpm --dir <worktree> --filter cpd-web-client run dev --host` on port 5173, in the window `web` of the cockpit's work session, without the session identity variables and with `CPD_MODE=preview` and `CPD_CHECKOUT=<worktree>` (see [Production and preview modes](#production-and-preview-modes), item 9). It builds Electron (`build:electron`) when `<worktree>/web/dist-electron/main.js` is missing, and leaves an existing bundle alone (`reload` always builds). It sets the tmux option `@cpd_web_preview` to `<worktree name> <sha>` and writes `web-state` in `${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}`. It prints the command that points Electron at the server: `CPD_MODE=preview CPD_CHECKOUT=<worktree> CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html pnpm --dir <worktree>/web run electron`. The URL ends in `/electron.html`: bare `http://localhost:5173` is the browser UI (`index.html`), not the Electron renderer. UI/CSS edits hot-reload through Vite React Fast Refresh with no rebuild. Main, preload and server changes still need `preview-web.sh reload` (SIGUSR1). It also starts the watchdog that rolls back a crashing build (see [Crash safety](#crash-safety-pro-d64)). The watchdog runs with `CPD_MODE=preview`, `CPD_CHECKOUT=<worktree>` and `CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html`; its relaunch runs in production mode with no dev URL (`productionEnv`, item 8), so a relaunch loads the built renderer. It never starts or kills Electron.
- **`off`** respawns the window `web` on the primary checkout with `CPD_MODE=production` and `CPD_CHECKOUT` removed, unsets the option `@cpd_web_preview`, removes `web-state` and stops the watchdog.
- **`reload [<worktree>]`** typechecks and builds the worktree's Electron output, then sends SIGUSR1 to the running Electron main process. It exits 1 without building or signalling when the typecheck fails. Details are in [Crash safety](#crash-safety-pro-d64).
- **`status`** prints the worktree and the sha, or `no web preview`.

If loading the dev URL fails (for example the server is down), boot logs the error and loads the built renderer (`renderer/electron.html`) instead of exiting. The failed load is still recorded as a crash for its epoch (see [Crash safety](#crash-safety-pro-d64)).

`preview-web.sh` itself never runs `git checkout`, `git reset` or `git clean`. The Electron sync runs `git checkout HEAD -- <paths>` and `git clean -f -- <paths>` in the preview worktree, scoped to the dirty paths that `origin/main` supersedes, before it fast-forwards or rebases (see [Electron sync in preview](#electron-sync-in-preview-a7) and [Electron app](#electron-app)). The matching scripts in `web/package.json` are `preview:on`, `preview:off` and `preview:reload`.

## Serve-only mode and the View menu preview

(air-d41, air-d49, air-n19; the three in-place clients are documented in `web/ELECTRON.md`)

The View menu of `cpd desktop` carries **WebUI Preview**, a third in-place client: the same one process and one window switch to the dev renderer of a preview worktree. The worktree and its dev server are managed by two serve-only subcommands of `preview-web.sh`:

```
sh scripts/preview-web.sh serve [<worktree>]
sh scripts/preview-web.sh unserve
```

- **`serve [<worktree>]`** ensures the preview worktree exactly as `on` does (create on branch `preview/web` from `origin/main` when missing, default path `${CPD_WEB_PREVIEW_WORKTREE:-<repo parent>/cpd.preview.web}`, install and link `node_modules` when created) and starts, or respawns, the Vite dev server of the worktree on port 5173, in the window `web` of the cockpit's work session, without the session identity variables and with `CPD_MODE=preview` and `CPD_CHECKOUT=<worktree>` (see [Production and preview modes](#production-and-preview-modes), item 9). It starts no watchdog, writes no `web-state`, sets no `@cpd_web_preview` and prints no Electron command.
- **`unserve`** stops only that dev server (the `web` window). It does not touch the watchdog, `web-state` or `@cpd_web_preview`.
- **`on [<worktree>]`**, **`off`** and **`status`** keep exactly the behaviour documented above; they are the subcommands of `cpd desktop --preview`, `--off` and `--status` and are unchanged.

### The View menu client

Selecting **WebUI Preview** in the View menu of `cpd desktop`:

1. runs `serve [<worktree>]` for the preview worktree (default path as for `on`),
2. shows a built-in loading page in the window,
3. once the client health is healthy, loads the dev renderer at `CPD_ELECTRON_DEV_URL` (default `http://localhost:5173/electron.html`),
4. keeps the server until quit and stops it on quit (`unserve`).

The same Electron-main backend serves the preview renderer: the dev renderer talks IPC (`cpd:c2s` / `cpd:s2c`, see `web/ELECTRON.md`) to the services of the running main process, not to the Vite server's WebSocket backend, which serves only a plain browser tab at `http://localhost:5173`.

A start or load failure shows a native error dialog and the previous view returns. There is never a silent fallback to another view.

[Crash safety (pro-d64)](#crash-safety-pro-d64) stays true for `cpd desktop --preview`: its watchdog still rolls back a crashing build, and its rollback lands in the production WebUI (the built renderer, relaunched in production mode, item 8). The View menu preview runs no watchdog; its failure path is the native error dialog above.

## Production and preview modes

Production is the default; preview is opt-in. The contract below is the source every later Job follows literally. Accepted assumptions A1-A8 (User, 2026-11-06 EST; pro-d73, pro-d74) are stated inside the items. The primary checkout is a mirror of `origin/main` and is never a preview.

### The contract

1. **`web/src/electron/preview-mode.ts` (new).** `export type PreviewMode = { readonly mode: 'production' } | { readonly mode: 'preview'; readonly checkout: string; readonly devUrl: string | null }; export function readPreviewMode(env: Readonly<Record<string, string | undefined>>): PreviewMode`. CPD_MODE unset or empty = production (never carries a devUrl). 'preview' needs a non-empty CPD_CHECKOUT, else throws Error('CPD_MODE=preview needs CPD_CHECKOUT'); devUrl = non-empty CPD_ELECTRON_DEV_URL else null. Any other CPD_MODE throws Error(`CPD_MODE must be production or preview, got "<value>"`). Do not reuse the names UiMode, DEFAULT_MODE, readMode, writeMode of web/src/electron/mode.ts (tui|webui, unrelated).
2. **`web/src/electron/boot.ts`.** boot() calls readPreviewMode(deps.env) first, before app.setName: a throw rejects boot, no window is created. production: always deps.loadFile(win, 'renderer/electron.html') (a set dev URL is ignored). preview with devUrl: the existing loadDevUrl path with its loadFile fallback. preview without devUrl: loadFile.
3. **`web/src/electron/drift.ts` (new).** `export const DRIFT_STALE_MS = 86_400_000; export type DriftEntry = { readonly kind: 'dirty' | 'untracked' | 'commit'; readonly ref: string; readonly at: string }` (ref = repo-relative path, or the commit sha; at = ISO time); `export interface DriftSummary { readonly count: number; readonly oldestAt: string | null; readonly stale: boolean }`; `export interface DriftReport extends DriftSummary { readonly entries: readonly DriftEntry[] }`. `readDrift(deps: { exec: UpdateExec; mtimeMs(path: string): number | null; now(): number }, root: string): Promise<DriftReport>` (UpdateExec from web/src/electron/update/types.ts, called with { cwd: root }). Commands, exactly: `git status --porcelain --untracked-files=all` (each line is `XY path`; `??` = untracked, anything else = dirty; for a rename `old -> new` take new) and `git log --format=%H%x09%cI origin/main..HEAD` (one commit per line: sha, TAB, ISO commit date). A file entry's `at` is the ISO time of deps.mtimeMs(`${root}/${path}`); when that is null (file deleted) it is the date from `git log -1 --format=%cI HEAD`. count = entries.length; oldestAt = the smallest `at` or null; stale = oldestAt !== null && now() - Date.parse(oldestAt) >= DRIFT_STALE_MS. A non-zero git exit throws Error with its stderr. `rejectDrift(deps: { exec: UpdateExec; now(): number }, root: string): Promise<{ ref: string | null; stashed: boolean }>`: runs `git status --porcelain --untracked-files=all` and `git rev-list --count origin/main..HEAD`; clean and 0 ahead returns { ref: null, stashed: false } with no other git call. Else stamp = new Date(now()).toISOString() with every ':' and '.' replaced by '-'; when ahead: `git update-ref refs/cpd/preview-rejected/<stamp> HEAD` (ref = that name); when dirty or untracked: `git stash push --include-untracked -m preview-rejected-<stamp>` (non-zero: throw Error before any reset); finally `git reset --hard origin/main` (non-zero: throw). It never fetches.
4. **`scripts/preview-drift.mjs` (new).** Thin wiring like scripts/preview-watchdog.mjs, every decision in drift.ts: `node scripts/preview-drift.mjs status|reject [--worktree <dir>]`; the root is --worktree or CPD_CHECKOUT, none = exit 2 with usage; prints one JSON value (the DriftReport, or the rejectDrift result).
5. **`web/src/server/cockpit/status.ts`.** StatusDeps gains `env: Readonly<Record<string, string | undefined>>` and `drift(root: string): Promise<DriftReport>`; Status gains `readonly mode: 'production' | 'preview'` and `readonly drift: DriftSummary | null` (count, oldestAt, stale only, no entries). readStatus calls readPreviewMode(deps.env) (a throw rejects readStatus, loud): production = mode 'production', drift null, deps.drift never called; preview = deps.drift(checkout) and its summary. web/src/protocol/cockpit.ts StatusModel gains the same two fields; web/src/server/plugin.ts wires env: process.env and drift via readDrift with real git (child_process), fs.statSync mtime and Date.now.
6. **`web/src/ui/cockpit/StatusBar.tsx`.** its StatusModel gains `mode` and `drift: DriftSummary | null`; barStatus passes both through. The bar renders the version as `<Badge variant="ghost" data-part="version">` with the bare version text (no 'cpd: ' prefix; keep data-alert on hostAlert); when mode is 'preview' a `<Badge variant="outline" data-part="mode">` with the text `preview`, coloured sakura (text and border from the --sakura token of web/src/ui/app.css; use a Tailwind token if the theme maps one, else var(--sakura)), directly after the version Badge; when drift is not null and drift.count > 0 a `data-part="drift"` pill with the text `drift N` (N = count), `data-stale="true"` when drift.stale, the mitarashi tone when stale.
7. **`web/src/electron/update/checkout.ts`.** `syncCheckout(exec: UpdateExec, root: string, opts: { readonly rebase: boolean }): Promise<SyncResult>`; SyncResult's skipped reason gains 'rebase-conflict'. Order: `git fetch origin`; HEAD and origin/main equal = current. Then, before any merge or rebase: `git diff --name-only HEAD origin/main` = changed paths; `git status --porcelain --untracked-files=all` = dirty paths; of the changed paths that are dirty-tracked run `git checkout HEAD -- <those paths>` (only when any), of those untracked (`??`) run `git clean -f -- <those paths>` (only when any); other dirty paths are left alone. Then, if HEAD is an ancestor of origin/main (`git merge-base --is-ancestor HEAD origin/main` = 0): `git merge --ff-only origin/main` (non-zero = skipped 'diverged'). Else with opts.rebase: `git rebase --autostash origin/main`; non-zero runs `git rebase --abort` and returns skipped 'rebase-conflict'. Else skipped 'diverged'. updated = { kind: 'updated', from, to, lockfileChanged } as today. web/src/electron/update/start.ts passes { rebase: readPreviewMode(deps.env).mode === 'preview' }.
8. **`web/src/electron/watchdog.ts`.** WatchdogPorts gains `terminate(pid: number): Promise<void>` (resolves once the process is dead); `signalReload` is removed when nothing else uses it. rollback after restoreLastGood: when pidAlive(rec.pid) call terminate(rec.pid); then always log {event:'relaunch', ...} and call ports.relaunch(). New `export function productionEnv(env: Readonly<Record<string, string | undefined>>): Record<string, string>` = every defined variable, CPD_MODE='production', CPD_ELECTRON_DEV_URL removed. scripts/preview-watchdog.mjs: relaunch spawns with env productionEnv(process.env); terminate = SIGTERM, then SIGKILL after 5 s, resolved when process.kill(pid, 0) throws ESRCH (polled, bounded).
9. **`scripts/preview-web.sh`.** the dev server (serve) is started with CPD_MODE=preview and CPD_CHECKOUT=<the worktree> in its environment, and the watchdog (start_watchdog) with CPD_MODE=preview, CPD_CHECKOUT=<the worktree> and the existing CPD_ELECTRON_DEV_URL; every other place the script starts Electron or the dev server gets the same two. `off` is the one exception: it starts the dev server on the primary checkout with CPD_MODE=production and CPD_CHECKOUT removed from the environment (`env -u CPD_CHECKOUT`), so the primary checkout never shows a preview badge and drift reject never targets it.

Every process the script starts, by line of `scripts/preview-web.sh`:

| Line | Process | Environment under this contract |
| --- | --- | --- |
| 55-56 | dev server, `tmux respawn-pane` (on, the web window exists) | `on`: CPD_MODE=preview, CPD_CHECKOUT=<worktree>. `off`: CPD_MODE=production, CPD_CHECKOUT removed |
| 58-59 | dev server, `tmux new-window` (on, no web window) | same as 55-56 |
| 80-81 | watchdog, `nohup node preview-watchdog.mjs` | CPD_MODE=preview, CPD_CHECKOUT=<worktree>, CPD_ELECTRON_DEV_URL |
| 129 | the command printed for the User to start Electron by hand, `pnpm --dir <worktree>/web run electron` | the printed line carries `CPD_MODE=preview CPD_CHECKOUT=<worktree>` before `CPD_ELECTRON_DEV_URL=<url>` |

Electron is never started by the script. The watchdog's relaunch (`pnpm --dir <worktree>/web exec electron .`, item 8) starts it with `productionEnv(process.env)`. Lines 37 (tmux client), 89 (`pnpm typecheck`), 94 and 119 (`pnpm build:electron`), 114-116 (`git`, `pnpm install`), 123 (`git`) and 69, 102-106 (`env kill`) start no dev server and no Electron and get no preview variable.

### Drift, approval, rejection and the 24 h rule (A3-A5)

- **Drift (A3, preview only).** Drift is the dirty or untracked files, and the commits not in `origin/main`, in `CPD_CHECKOUT`. Each entry carries a time: a file's mtime (the HEAD commit date when the file is deleted), a commit's commit date. Production has no drift: `status` reports `drift: null` and never reads the checkout.
- **Stale after 24 h (A3).** `stale` is true when the oldest entry is `DRIFT_STALE_MS` (86_400_000 ms, a constant, no config) or more old. The StatusBar then shows the drift pill in the mitarashi tone with `data-stale="true"` (item 6).
- **Approve (A5, not code).** Approval is not a command. A Manager hands the drift to a Lead as a Plan/PR; the Lead lands it on main through the normal Plan flow.
- **Reject clears the preview (A4).** After the drift is handed over, `node scripts/preview-drift.mjs reject [--worktree <dir>]` (root = `--worktree`, else `CPD_CHECKOUT`) clears the preview. It never fetches. It keeps every change first: commits ahead of `origin/main` go to `refs/cpd/preview-rejected/<stamp>` (at HEAD); dirty or untracked files go to a stash with `git stash push --include-untracked -m preview-rejected-<stamp>`; then `git reset --hard origin/main`. A clean tree with nothing ahead does nothing and prints `{"ref":null,"stashed":false}`. The command prints the `rejectDrift` result as one JSON value.
- **Undo a reject.** The commits stay reachable from their ref (`git reset --hard refs/cpd/preview-rejected/<stamp>` restores them), and the stash stays in `git stash list` under its message (`git stash pop` restores the edits). Nothing is deleted by reject.

### Watchdog recovery (A6)

A crash, a stalled load or a dead process rolls the preview back, as in the rules of [Watchdog](#watchdog). Under this contract the rollback ends on the built renderer in production mode:

1. `restoreLastGood()` restores the last-good build and the files (as before).
2. When the pid is alive, `terminate(pid)` stops the live Electron: SIGTERM, then SIGKILL after 5 s, resolved once `process.kill(pid, 0)` throws ESRCH.
3. The watchdog always logs `{event: 'relaunch', ...}` and calls `relaunch()`, which spawns `pnpm --dir <worktree>/web exec electron .` with `productionEnv(process.env)`: `CPD_MODE=production` and no `CPD_ELECTRON_DEV_URL`. `CPD_CHECKOUT` stays in the environment.

`signalReload` (SIGUSR1 from the watchdog) is removed, since nothing else calls it. The Electron `reload` command of `preview-web.sh` still sends SIGUSR1 by hand.

### Electron sync in preview (A7)

`syncCheckout` (item 7) removes only the dirty or untracked paths that `HEAD..origin/main` supersedes, also paths that upstream adds; other local edits are kept. It then fast-forwards. In preview mode (`opts.rebase` true) a checkout with local commits is rebased onto `origin/main` with `--autostash`; a conflict aborts the rebase and skips with `rebase-conflict`. Production never rebases: a checkout that cannot fast-forward skips with `diverged`.


## Electron app

(pro-d37, pro-d38, pro-d39)

The Electron app (`web/src/electron/main.ts`) shows the same WebUI in a window. `cpd desktop` launches it, or reloads the one that runs (commands.md §14.4a; `web/ELECTRON.md`). Four behaviours:

- **Load order (pro-d37).** The window loads only after `startAutoUpdate` returns and `startServices` has run, so the IPC handlers `ghostty:wasm` and `webui:send` exist before the renderer loads. Accepted cost: the window stays blank during a first-run CLI bootstrap; `start.ts` waits for it.
- **Name (pro-d38).** The app is named CPD. `productName` is `CPD` in `web/package.json` and `web/electron-builder.yml`. `app.setName('CPD')` runs before `app.whenReady()`, and the BrowserWindow title is `CPD`. `<title>CPD</title>` is in `web/electron.html` and `web/index.html`. `cpd desktop` and the `electron` script patch `CFBundleName` and `CFBundleDisplayName` to `CPD` in `Electron.app/Contents/Info.plist` with `plutil` on darwin.
- **Reload on SIGUSR1 (pro-d39, pro-d46).** The main process reloads the WebUI in place on `SIGUSR1`, so a new build applies without a restart. Send the signal with `kill -USR1 <pid>` to the Electron main process. Live patches reload without closing or focusing the window. The handler runs two steps, in this order:
  1. **Server (pro-d46).** The Electron build emits the server entry next to the main process. `web/vite.electron.config.ts` gives Rollup two inputs, `main` (`src/electron/main.ts`) and `server` (`src/server/plugin.ts`), with `entryFileNames: "[name].js"`, so the output directory holds `main.js` and `server.js`. The handler imports `server.js` again from its file URL with a fresh `?t=<Date.now()>` query, which evaluates the entry as a new module. It then disposes the running WebUI services and starts those of the new module: the WebUI IPC handlers, the cockpit service and the attention watch. The PTY handlers are not restarted. The import runs before the dispose, so a failed import leaves the running services in place.
  2. **Renderer.** It reloads the window with `reloadIgnoringCache`, when the webContents is not destroyed.

  If the import throws, the error is logged and the renderer still reloads. The `?t=` query re-evaluates only the `server.js` entry: a module that `server.js` imports, including one the build puts in a shared chunk, keeps its cached instance until the app restarts. `server.js` exists only in an Electron build output.
- **Live sync (pro-d39).** `syncCheckout` (`web/src/electron/update/checkout.ts`) syncs a live preview worktree when `origin/main` advances:
  - It no longer requires the branch `main`: any branch whose HEAD is an ancestor of `origin/main` fast-forwards.
  - It no longer skips a dirty tree. Before the fast-forward it removes only the dirty paths that `origin/main` supersedes (`git checkout HEAD -- <tracked>`, `git clean -f -- <untracked>`). Landed fixes replace their temporary live patches; other live patches survive (A7).
  - In preview mode a checkout with local commits is rebased onto `origin/main` with `--autostash`; a conflict aborts and is reported `skipped: rebase-conflict` (item 7).
  - A fast-forward that cannot apply is reported `skipped: diverged`.
  - The sync reasons are `fetch-failed` | `diverged` | `rebase-conflict`. The `branch` and `dirty` reasons are removed.

## Crash safety (pro-d64)

A crashing WebUI build must not leave the preview dead. The main process records each load of the WebUI as a reload epoch in a health file. A supervisor, the watchdog, rolls the files back to the last build that proved healthy and reloads Electron. Renderer crashes, load failures and a dead process all count.

With a dev URL (`CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html`, honoured in preview mode only), the first launch loads it. The watchdog's relaunch runs in production mode with no dev URL (item 8), so it loads the built renderer. A failed dev-URL load is recorded as a `did-fail-load` crash, and boot then loads `renderer/electron.html` from the built output, which starts a new epoch. The crash clears before `crashDebounceMs` when that load starts in time, so the watchdog does nothing.

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
- **Panes.** `App` wraps `ContextTree`, `PlanView`, `ManagerView`, `ChatPane` and `Inbox` each in its own `ErrorBoundary`, named `ContextTree`, `PlanView`, `ManagerView`, `ChatPane` and `Inbox`. `App` takes an optional `onCrash` and passes it down, so one pane's throw leaves the others mounted.
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
- `terminate(pid: number): Promise<void>`: stops the process and resolves once it is dead (SIGTERM, then SIGKILL after 5 s). It replaces `signalReload`, which is removed.
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
4. **Rollback.** When `restore.hasLastGood()`, the watchdog logs `rollback` with `reason`, `reloadEpoch` and `pid`, then calls `restore.restoreLastGood()`, then `terminate(pid)` when `pidAlive(pid)`, then always logs `relaunch` and calls `relaunch()` (item 8). A restore error is logged as `rollback-failed` and does not stop the watchdog. Without a last good, it logs `rollback-skipped` once per epoch.
5. **One attempt per restore point.** When the epoch that follows a rollback also triggers a rollback, the watchdog logs `rollback-skipped` with reason `already rolled back`, and does nothing more until a candidate is promoted.
6. **Malformed health.** A malformed health file is logged once per distinct text and ignored.

The supervisor is `scripts/preview-watchdog.mjs`:

```
node scripts/preview-watchdog.mjs --worktree <dir> --home <cpd-home> [--stabilize-ms n] [--crash-debounce-ms n] [--loading-grace-ms n] --relaunch <cmd> [args...]
```

- `--relaunch` comes last. It takes the command and every argument after it.
- It imports `./watchdog.ts`, `./restore-points.ts` and `./health.ts` as siblings. `preview-web.sh` copies the four files into one frozen directory outside the worktree and outside `dist-electron`, so live patches cannot change a running supervisor.
- Real ports: `node:fs/promises`; `execFile` from `node:child_process` for git; `process.kill(pid, 0)` for `pidAlive`; `terminate` sends SIGTERM, then SIGKILL after 5 s, and resolves when `process.kill(pid, 0)` throws ESRCH (polled, bounded). `relaunch` is a detached spawn of the `--relaunch` command, with cwd set to the worktree, env `productionEnv(process.env)` and stdio to `preview-watchdog.log`.
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
