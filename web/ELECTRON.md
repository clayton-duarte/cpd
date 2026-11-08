# Electron app

The Electron app is a standalone Electron desktop app in `web/` with three in-place clients in one process and one window: TUI, WebUI and WebUI Preview. TUI runs `cpd tui --attach` in a full-window terminal. The terminal is `@aurigax-ai/ghostty-web` 0.3.9 (libghostty-vt WASM, WebGL 2 renderer with a silent Canvas 2D fallback), backed by `node-pty` in the Electron main process. It draws the Dango v3 16-colour palette in Maple Mono 13px, weight 300, with ligatures off (ghostty-web has no ligature option), WebUI is the Dango v3 cockpit (the same `App` the browser build mounts), fed by the main process over the one WebSocket transport (ADR 0015 option B, air-d46; formerly over IPC, pro-d23 — see §WebSocket transport). WebUI Preview is the same cockpit served by the Vite dev server of a preview worktree (see [Previewing a UI change](../docs/ui-preview.md)). The app always launches in WebUI (the built production renderer); a native View menu switches the client for this run only — TUI, WebUI or WebUI Preview — and no choice is ever read back as a launch default. On macOS the app also owns the Dock icon, the Dock badge, the bounce and the menu bar (Tray) icon. Decisions: pro-d21 (D5, D6, D7, dual-mode), pro-d22 (TUI), pro-d23 (native shell; its live WebUI over IPC transport is superseded by ADR 0015 option B, air-d46) and pro-d24 (packaging), with air-d40 (always launch WebUI; the View menu switches for the run only; `ui-mode.json` dropped), air-d41 (WebUI Preview in the View menu), air-d49 (serve-only preview) and air-d50 (the host tabs show only in TUI).

## Run

Launch the app with `cpd desktop`:

```sh
cpd desktop
```

`cpd desktop` runs the checkout that `cpd install --root <checkout>` built: it reads `~/.local/share/cpd/app-root`, which install writes, and without that file it refuses with `desktop_not_installed`. It builds `web/dist-electron` when that is missing, makes node-pty's `spawn-helper` executable and, on macOS, names the app CPD. Then it sends SIGUSR1 to the running app, or, when none runs, launches Electron without waiting, with its output in `$CPD_HOME/run/app.log` (commands.md §14.4a). The launch removes the five per-session cpd variables — `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT`, `CPD_QUIT_MARKER`, `CPD_JOB_KIND` (contract §6.6) — from the child's environment, and the app strips the same five from its own environment at boot (Boot order, below), so no agent Session's identity reaches the app or the `cpd` children it spawns; `CPD_HOME`, `PI_SKIP_VERSION_CHECK` and the rest of the environment stay.

These steps are needed because node-pty 1.1.0 ships `spawn-helper` with mode 0644, and without the execute bit the pty fails with `posix_spawnp failed`. The `allowBuilds: electron: false` setting in `pnpm-workspace.yaml` stops the postinstall download of the Electron binary, so worktrees and CI have no binary. When the binary is missing, `cpd install` and `cpd desktop` run `node web/node_modules/electron/install.js` to download it; if that fails, the install or app step fails before plutil runs. When the binary is present, no extra command runs. `pnpm --filter cpd-web-client run electron` does the same by hand; it is not the way to launch.

Prerequisites:

- `cpd install --root <checkout>` has run for the checkout you launch (it runs `pnpm install --frozen-lockfile` there).
- `cpd` and `tmux` are installed and on the PATH. The pty runs `cpd tui --attach` in your home directory with your login-shell PATH.
- Maple Mono is installed on the system. It is not bundled; without it the terminal falls back to the generic monospace font.

Why `allowBuilds` in `pnpm-workspace.yaml` sets `electron` and `node-pty` to `false`: frozen installs, typecheck and vitest in worktrees and CI need no compiler and no Electron binary. node-pty is imported only in the Electron main process and is mocked in tests.

## Boot order

The Electron boot sequence ensures the mode is applied only after the window exists. `prepare()` (called by `app.whenReady()`) runs paths, env and native shell setup. When ready, the window is created. Then the mode controller's `init()` applies WebUI, the only launch view, with menu and window calls, and deletes the dropped `$CPD_HOME/run/ui-mode.json` once, with a log line. Finally the renderer loads with the result. The view is applied after the window exists, so no view apply touches the window before it is created.

The boot strips the five per-session cpd variables — `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT`, `CPD_QUIT_MARKER` and `CPD_JOB_KIND`, the same five the `cpd desktop` launch removes (commands.md §14.4a; contract §6.6) — from `process.env`, before any cpd child is spawned (every `cpd` call of the updater inherits the app's environment); `CPD_HOME`, `PI_SKIP_VERSION_CHECK` and every other variable stay, so the app still finds the home.

## Module contract

### Module contract (web/src/electron/)

All modules are ES modules in `web/src/electron/`; relative imports carry the `.ts` extension (`import { CHANNELS } from "./channels.ts"`); type-only imports use `import type` (the repo sets `verbatimModuleSyntax`). Nothing in this table imports `electron`, `node-pty` or `@aurigax-ai/ghostty-web` at runtime: those three are reached only through the boundary interfaces below, so unit tests use hand-written fakes. The only runtime importers of them are the thin composition roots `main.ts`, `preload.ts` and `renderer.ts`, which have no unit tests (they are verified by the manual checklist). Never declare an interface named `CpdResult`, `CpdExec`, `Cpd`, `ResolvedSession`, `SessionEmit`, `GitBoundary`, `SessionHandle`, `SessionBackend` or `PendingSubmission` (web/src/types-unique.test.ts forbids duplicates of those names).

### channels.ts (types and constants only, no logic)

```ts
export const CHANNELS = {
  ptyStart: "pty:start",     // renderer -> main, invoke, payload PtySize, resolves undefined
  ptyWrite: "pty:write",     // renderer -> main, send, payload string
  ptyResize: "pty:resize",   // renderer -> main, send, payload PtySize
  ptyData: "pty:data",       // main -> renderer, payload string
  ptyExit: "pty:exit",       // main -> renderer, payload number (exit code)
  uiGetMode: "ui:get-mode",  // renderer -> main, invoke, resolves UiMode
  uiMode: "ui:mode",         // main -> renderer, payload UiMode
  wasm: "ghostty:wasm",      // renderer -> main, invoke, resolves Uint8Array (bytes of ghostty-vt.wasm)
} as const;
export type UiMode = "tui" | "webui" | "webui-preview";
export interface PtySize { cols: number; rows: number }
export interface CpdElectronBridge {          // window.cpdElectron
  readWasm(): Promise<Uint8Array>;
  getMode(): Promise<UiMode>;
  startPty(size: PtySize): Promise<void>;
  writePty(data: string): void;
  resizePty(size: PtySize): void;
  onPtyData(cb: (data: string) => void): () => void;   // returns the unsubscribe function
  onPtyExit(cb: (exitCode: number) => void): () => void;
  onMode(cb: (mode: UiMode) => void): () => void;
}
```

### env.ts (macOS login-shell PATH)

```ts
export const PATH_BEGIN = "__CPD_PATH_BEGIN__";
export const PATH_END = "__CPD_PATH_END__";
export type ShellRunner = (file: string, args: readonly string[]) => Promise<string>; // resolves stdout; rejects on non-zero exit or timeout
export interface LoginPathOptions { platform: NodeJS.Platform; shell: string | undefined; currentPath: string | undefined; run: ShellRunner }
export function parseMarkedPath(stdout: string): string;
export function resolveLoginPath(options: LoginPathOptions): Promise<string>;
```

- `parseMarkedPath` returns the text between the FIRST `PATH_BEGIN` and the first `PATH_END` after it, ignoring any other output (shell rc noise) before, between and after; it throws `Error("login shell printed no PATH marker")` when either marker is missing.
- `resolveLoginPath` when `platform !== "darwin"`: returns `currentPath ?? ""` and never calls `run`. On darwin: calls `run(shell ?? "/bin/zsh", ["-ilc", script])` exactly once, where `script` prints `PATH_BEGIN`, then `$PATH`, then `PATH_END` (the script text contains both marker constants and `$PATH`), and returns `parseMarkedPath(stdout)`. A rejection of `run` and a `parseMarkedPath` error propagate unchanged: no fallback, no default PATH.

### pty.ts (the node-pty boundary)

```ts
export interface PtyProcess {
  onData(cb: (data: string) => void): void;
  onExit(cb: (event: { exitCode: number }) => void): void;
  write(data: string): void;
  resize(cols: number, rows: number): void;
  kill(): void;
}
export interface PtySpawnOptions { name: string; cols: number; rows: number; cwd: string; env: Record<string, string> }
export interface PtyBackend { spawn(file: string, args: string[], options: PtySpawnOptions): PtyProcess }
export interface PtySinks { data(chunk: string): void; exit(exitCode: number): void }
export interface PtySession { write(data: string): void; resize(size: PtySize): void; kill(): void }
export const CPD_TUI_COMMAND: { readonly file: "cpd"; readonly args: readonly ["tui", "--attach"] };
export interface StartPtyOptions {
  backend: PtyBackend; size: PtySize; cwd: string;
  env: Record<string, string | undefined>; sinks: PtySinks;
  command?: { file: string; args: readonly string[] };   // default CPD_TUI_COMMAND
}
export function startPty(options: StartPtyOptions): PtySession;
```

- `startPty` calls `backend.spawn(command.file, [...command.args], { name: "xterm-256color", cols, rows, cwd, env })` once. `env` is `options.env` without its `undefined` values, with `TERM` set to `"xterm-256color"` and `COLORTERM` set to `"truecolor"` (overriding any inherited value).
- Process data goes to `sinks.data(chunk)` unchanged; process exit goes to `sinks.exit(exitCode)` exactly once.
- `write(data)` and `resize({cols, rows})` delegate to the process (`resize(cols, rows)`). `kill()` calls the process `kill()` once; a second `kill()` is a no-op. After the process exited, `write`, `resize` and `kill` are no-ops and do not throw.
- When `spawn` throws, `startPty` does not throw: it calls `sinks.data("failed to start cpd tui --attach: " + message + "\r\n")` (message = the error's `message`, or `String(error)` for a non-Error) and then `sinks.exit(1)`, and returns an inert session (all three methods no-ops).

### ipc.ts (main-process IPC wiring)

```ts
export type IpcListener = (event: unknown, ...args: unknown[]) => unknown;
export interface IpcMainLike {
  handle(channel: string, listener: IpcListener): void;
  on(channel: string, listener: IpcListener): void;
  removeHandler(channel: string): void;
  removeListener(channel: string, listener: IpcListener): void;
}
export interface IpcDeps {
  ipc: IpcMainLike;
  startPty(size: PtySize, sinks: PtySinks): PtySession;
  sinks: PtySinks;                    // where pty data and exit go (the window)
  readWasm(): Promise<Uint8Array>;
  getMode(): UiMode;
}
export function parsePtySize(raw: unknown): PtySize;
export function parsePtyWrite(raw: unknown): string;
export function registerIpc(deps: IpcDeps): { dispose(): void };
```

- `parsePtySize(raw)`: `raw` must be an object whose `cols` and `rows` are integers from 1 to 1000; returns `{ cols, rows }` (a fresh object with only those two keys); otherwise throws `Error("invalid pty size")`. `parsePtyWrite(raw)`: returns `raw` when it is a string, otherwise throws `Error("invalid pty write")`.
- `registerIpc` registers `ipc.handle(CHANNELS.ptyStart)`, `ipc.handle(CHANNELS.uiGetMode)`, `ipc.handle(CHANNELS.wasm)`, `ipc.on(CHANNELS.ptyWrite)` and `ipc.on(CHANNELS.ptyResize)`; each listener receives `(event, payload)`.
- `pty:start`: parses the payload with `parsePtySize`; when no session exists it calls `deps.startPty(size, wrappedSinks)` and keeps the session; when a session exists it calls `session.resize(size)` instead (no second spawn). It returns `undefined`. `wrappedSinks.data` forwards to `deps.sinks.data`; `wrappedSinks.exit(code)` clears the kept session FIRST, then forwards to `deps.sinks.exit(code)`, so the next `pty:start` spawns again.
- `pty:write`: `session.write(parsePtyWrite(payload))`; `pty:resize`: `session.resize(parsePtySize(payload))`. Both throw `Error("pty not started")` when there is no session. Invalid payloads throw the parse errors (the session is untouched).
- `ui:get-mode` returns `deps.getMode()`; `ghostty:wasm` returns `deps.readWasm()`.
- `dispose()` removes every handler and listener it registered (`removeHandler` / `removeListener` with the same listener objects) and kills the session if one exists.

### menu.ts (native menu template)

```ts
import type { MenuItemConstructorOptions } from "electron";
export type MenuTemplate = MenuItemConstructorOptions[];
export interface MenuTemplateOptions { mode: UiMode; platform: NodeJS.Platform; onSelect(mode: UiMode): void }
export function buildMenuTemplate(options: MenuTemplateOptions): MenuTemplate;
```

- Template order of `buildMenuTemplate`, on every platform: `{ role: "appMenu" }` only when `platform === "darwin"`; then `{ role: "editMenu" }`, on every platform (it is there so Cmd+C, Cmd+V, Cmd+X, Cmd+A and Cmd+Z work in WebUI; TUI ignores menu shortcuts, so the terminal's own key handler does its paste, see terminal.ts); then `{ label: "View", submenu: [...] }`; then `{ role: "windowMenu" }`. The template is therefore `[appMenu (darwin only), editMenu, View, windowMenu]`.
- The View submenu is exactly three items in this order: `{ label: "TUI", type: "radio", checked: mode === "tui", click }`, `{ label: "WebUI", type: "radio", checked: mode === "webui", accelerator: "CmdOrCtrl+Shift+M", click }` and `{ label: "WebUI Preview", type: "radio", checked: mode === "webui-preview", accelerator: "CmdOrCtrl+Shift+M", click }`. TUI has no accelerator. WebUI and WebUI Preview keep `CmdOrCtrl+Shift+M`. The `click` of each calls `onSelect("tui")` / `onSelect("webui")` / `onSelect("webui-preview")` (the click arguments are ignored). TUI ignores the menu shortcuts, so in TUI none of these accelerators fire; in WebUI and WebUI Preview they do.

### mode.ts (UI view controller)

The three UI modes are `tui`, `webui` and `webui-preview` (`UiMode` in channels.ts). `cpd desktop` always launches in WebUI, the built production renderer. No mode is ever persisted or read back as a launch default: the choice lasts for the run only. The dropped `$CPD_HOME/run/ui-mode.json` is deleted once on the first launch, with a log line; a missing file deletes nothing.

```ts
export function parseUiMode(raw: unknown): UiMode;
export interface ModeWindow { setIgnoreMenuShortcuts(ignore: boolean): void; sendMode(mode: UiMode): void }
export interface MenuHost { setApplicationMenu(template: MenuTemplate): void }
export interface ModeControllerDeps {
  fs: { remove(path: string): Promise<void> };   // removes the dropped ui-mode.json
  legacyFile: string;                            // $CPD_HOME/run/ui-mode.json
  menu: MenuHost; window: ModeWindow;
  platform: NodeJS.Platform; onError(error: unknown): void;
}
export interface ModeController { init(): Promise<void>; getMode(): UiMode; setMode(mode: UiMode): Promise<void> }
export function createModeController(deps: ModeControllerDeps): ModeController;
```

- `parseUiMode(raw)`: returns `raw` when it is `"tui"`, `"webui"` or `"webui-preview"`; otherwise throws `Error("invalid ui mode: " + JSON.stringify(raw))`.
- The persistence helpers are gone: no `DEFAULT_MODE`, no `readMode`, no `writeMode`, no `modeFilePath` for a mode file. No mode file is ever read as a launch default.
- Delete-once: `init()` removes `legacyFile` (`$CPD_HOME/run/ui-mode.json`, the format of earlier builds) exactly once, and logs one line when it removes a file. A missing file is not an error and removes nothing. `init()` then applies WebUI (below) unconditionally: the app always launches in WebUI.
- Controller: `getMode()` is `"webui"` until `init()` resolves. `setMode(mode)`: when `mode` equals the current view it does nothing (no menu, no window call); otherwise it updates the current view and applies it — there is no write, the choice lasts for the run only. Applying = `menu.setApplicationMenu(buildMenuTemplate({ mode, platform, onSelect }))`, then `window.setIgnoreMenuShortcuts(mode === "tui")`, then `window.sendMode(mode)`, in that order. The template's `onSelect(m)` is `controller.setMode(m).catch(deps.onError)`.

Run-only view marker (air-n19): the controller writes `$CPD_HOME/run/ui-view.json` on launch and on every switch, as one JSON object `{"view":"tui" | "webui" | "webui-preview","pid":<electron main pid>}` where `pid` is the pid of the Electron main process. The file is removed on graceful quit. It is a marker only: it is never read as a launch default. A reader treats a missing file, a malformed file or a dead pid as view unknown.

### bridge.ts (preload bridge, bundled into the sandboxed preload)

```ts
export interface IpcRendererLike {
  invoke(channel: string, ...args: unknown[]): Promise<unknown>;
  send(channel: string, ...args: unknown[]): void;
  on(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
  removeListener(channel: string, listener: (event: unknown, ...args: unknown[]) => void): void;
}
export function createBridge(ipc: IpcRendererLike): CpdElectronBridge;
```

- `readWasm()` = `ipc.invoke(CHANNELS.wasm)`; resolves the payload when it is a `Uint8Array`, otherwise rejects `Error("invalid wasm payload")`. `getMode()` resolves `ipc.invoke(CHANNELS.uiGetMode)` (main already validated; cast to `UiMode`). `startPty(size)` = `ipc.invoke(CHANNELS.ptyStart, { cols, rows })` resolving `undefined`.
- `writePty(data)` = `ipc.send(CHANNELS.ptyWrite, data)`; `resizePty(size)` = `ipc.send(CHANNELS.ptyResize, { cols, rows })`.
- `onPtyData`, `onPtyExit`, `onMode` register one listener with `ipc.on` on `ptyData`, `ptyExit`, `uiMode`; the listener calls `cb` with the first payload argument (after the event); the returned function calls `ipc.removeListener` with that same listener.
- This file may import only `./channels.ts` (it is bundled into a sandboxed preload that can `require` nothing but `electron`).

### terminal.ts (terminal host, no ghostty-web import)

```ts
export const DANGO_THEME: {
  foreground: string; background: string; cursor: string; cursorAccent: string;
  selectionBackground: string; selectionForeground: string;
  black: string; red: string; green: string; yellow: string; blue: string; magenta: string; cyan: string; white: string;
  brightBlack: string; brightRed: string; brightGreen: string; brightYellow: string; brightBlue: string; brightMagenta: string; brightCyan: string; brightWhite: string;
};
export const TERMINAL_FONT: { fontFamily: string; fontSize: number; fontWeight: number };

// Pinned pure helpers (pro-d30, pro-d32). Exact names and signatures; tests call them directly.
export const FONT_SIZE_MIN = 9;
export const FONT_SIZE_MAX = 28;
export const FONT_SIZE_DEFAULT = 13;
export function shiftEnterSequence(event: { key: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean }): string | null;
export function zoomAction(event: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean }): "in" | "out" | "reset" | null;
export function nextFontSize(current: number, action: "in" | "out" | "reset"): number;
export function createWheelAccumulator(): { push(deltaY: number, deltaMode: number, cellHeight: number): number };
export function isCmdV(event: { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }): boolean;

// Event shapes handed to the handlers below. A KeyboardEvent or WheelEvent satisfies them structurally.
export interface KeyEventLike { key: string; code: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean; isComposing: boolean }
export interface WheelEventLike { deltaY: number; deltaMode: number; clientX: number; clientY: number }

export interface TerminalEngine {
  open(): void;                       // mounts into its container
  fit(): void;                        // recomputes cols and rows from the container size
  size(): PtySize;
  write(data: string): void;
  focus(): void;
  setPaused(paused: boolean): void;
  onData(cb: (data: string) => void): void;       // keyboard input from the user
  onResize(cb: (size: PtySize) => void): void;    // fires when cols or rows change
  setClipboardWriteHandler(handler: (text: string) => boolean): void;   // OSC 52 writes
  fontSize(): number;                                                   // term.options.fontSize
  setFontSize(px: number): void;                                        // term.options.fontSize = px; does not resize the pty, the caller calls fit()
  cellHeight(): number;                                                 // renderer cell height in px
  setKeyHandler(handler: (event: KeyEventLike) => boolean): void;       // term.attachCustomKeyEventHandler
  setWheelHandler(handler: (event: WheelEventLike) => boolean): void;   // term.attachCustomWheelEventHandler
  appOwnsWheel(): boolean;                                              // true when mouse tracking is on or the alternate screen is active
  wheelStep(direction: "up" | "down", clientX: number, clientY: number): string;
    // alternate screen: "\x1b[A" / "\x1b[B"; mouse tracking: SGR "\x1b[<64;col;rowM" / "\x1b[<65;col;rowM" for the cell under (clientX, clientY)
  input(data: string): void;                                            // term.input(data, true): fires onData, so the bytes reach bridge.writePty
  paste(text: string): void;                                            // term.paste(text): bracketed when the app asked
  dispose(): void;
}
export interface ResizeSource { observe(cb: () => void): () => void }   // returns the unobserve function
export interface TerminalHostDeps {
  engine: TerminalEngine; bridge: CpdElectronBridge; resize: ResizeSource;
  clipboard: { writeText(text: string): Promise<void>; readText(): Promise<string> };
  view: { showTerminal(visible: boolean): void };
  onError(error: unknown): void;
}
export function startTerminalHost(deps: TerminalHostDeps): Promise<{ dispose(): void }>;
```

- `DANGO_THEME` values (Dango v3, from dotfiles themes/Dango): foreground `#d8d2cc`, background `#1c1a20`, cursor `#ecaec7`, cursorAccent `#1c1a20`, selectionBackground `#5a504a`, selectionForeground `#d8d2cc`; black `#5a504a`, red `#e17f6e`, green `#8da96e`, yellow `#c29653`, blue `#7aa2ce`, magenta `#ca88a3`, cyan `#67ada7`, white `#aba3af`; brightBlack `#87808c`, brightRed `#ffa796`, brightGreen `#b3cd96`, brightYellow `#e4bb7f`, brightBlue `#a1c6ef`, brightMagenta `#ecaec7`, brightCyan `#92d1ca`, brightWhite `#f3ead9`. `TERMINAL_FONT` = `{ fontFamily: '"Maple Mono", "Symbols Nerd Font Mono", "MesloLGS Nerd Font Mono", "MesloLGS NF", monospace', fontSize: 13, fontWeight: 300 }` (ligatures: ghostty-web has no ligature option and draws per cell). `fontSize` stays 13; zoom changes it at runtime only (see below) and nothing persists it.
- `startTerminalHost`, in this order: `engine.open()`; `engine.fit()`; register `bridge.onPtyData(engine.write)`; register `bridge.onPtyExit(code => engine.write("\r\n[cpd tui exited with code " + code + "]\r\n"))`; `engine.onData(bridge.writePty)`; `resize.observe(() => engine.fit())`; `engine.setClipboardWriteHandler(text => { deps.clipboard.writeText(text).catch(deps.onError); return true; })`; `engine.setKeyHandler(handleKey)` and `engine.setWheelHandler(handleWheel)` (below); `const mode = await bridge.getMode()` and apply it; `bridge.onMode(apply)`; `await bridge.startPty(engine.size())`; and only then `engine.onResize(size => bridge.resizePty(size))`. Every listener is registered before `startPty` is called, so no pty data is lost, and no resize reaches main before the pty exists.
- Apply mode `m`: `engine.setPaused(m !== "tui")`; `view.showTerminal(m === "tui")`; `engine.focus()` only when `m === "tui"`.
- The returned `dispose()` calls every unsubscribe function it got (bridge listeners, the resize unobserve) and `engine.dispose()`.
- Key handler `handleKey(e: KeyEventLike): boolean`, first match wins, otherwise `false` so ghostty-web handles the key (Cmd+C, copy of its selection, is unchanged):
  1. `shiftEnterSequence(e)` is not `null`: `engine.input(seq)` writes the CSI-u sequence `\x1b[13;2u` to the pty (through `onData`); return `true`. Plain Enter is untouched.
  2. `isCmdV(e)`: `deps.clipboard.readText()`, then `engine.paste(text)` when the text is not empty (an empty clipboard pastes nothing); return `true` at once, so ghostty calls `preventDefault` and the browser's native paste does not run a second time. A read rejection goes to `deps.onError`; the key stays consumed.
  3. `zoomAction(e)` is not `null`: `engine.setFontSize(nextFontSize(engine.fontSize(), action))`, then `engine.fit()` so `term.resize` fires `onResize` and the pty resizes; return `true`. Zoom is not persisted. It acts only while the terminal handles keys (TUI, focused), because the terminal is paused and hidden in WebUI.
- Wheel handler `handleWheel(e: WheelEventLike): boolean`:
  - `engine.appOwnsWheel()` is `false` (plain scrollback): return `false`, so ghostty-web scrolls its scrollback as today.
  - Otherwise (mouse tracking or alternate screen): `steps = wheel.push(e.deltaY, e.deltaMode, engine.cellHeight())` with `const wheel = createWheelAccumulator()` made once per host. When `steps !== 0`, `engine.input(engine.wheelStep(steps < 0 ? "up" : "down", e.clientX, e.clientY).repeat(Math.abs(steps)))`. Return `true` even when `steps` is `0`, so ghostty does not also scroll for a sub-threshold delta.
- Pinned semantics of the helpers:
  - `shiftEnterSequence`: returns `"\x1b[13;2u"` for `key === "Enter"` with `shiftKey` only (no ctrl, alt or meta) and `isComposing` false; otherwise `null`.
  - `zoomAction`: needs `metaKey` and no `ctrlKey` or `altKey`. `key` `"="` or `"+"` is `"in"` (Cmd+Shift+= types `+`, so shift is ignored); `"-"` or `"_"` is `"out"`; `"0"` is `"reset"`; anything else `null`.
  - `nextFontSize`: `"in"` adds 1, `"out"` subtracts 1, both clamped to `FONT_SIZE_MIN` (9) through `FONT_SIZE_MAX` (28); `"reset"` returns `FONT_SIZE_DEFAULT` (13).
  - `createWheelAccumulator().push`: converts `deltaY` to px (`deltaMode` 0 = px; 1 = lines, `deltaY * cellHeight`; 2 = pages, treated as 10 lines, `10 * deltaY * cellHeight`). Threshold is `max(24, 1.5 * cellHeight)` px. It returns the signed number of whole steps (negative = up, same sign as `deltaY`) and keeps the sub-threshold remainder. A delta of the opposite sign first zeroes the remainder.
  - `isCmdV`: `code === "KeyV"` with `metaKey` and no `ctrlKey`, `altKey` or `shiftKey`.
- Menu accelerators do not fire in TUI (menu.ts section), so the key handler, not the editMenu, does Cmd+V in TUI. The editMenu roles serve WebUI.

### Composition roots (not unit-tested, thin)

- `preload.ts`: `contextBridge.exposeInMainWorld("cpdElectron", createBridge(ipcRenderer))`.
- `renderer.ts`: reads `window.cpdElectron`, loads `Ghostty.fromBytes(await bridge.readWasm())`, builds a `TerminalEngine` over `@aurigax-ai/ghostty-web` (`new Terminal({ ghostty, renderer: "webgl", theme: DANGO_THEME, ...TERMINAL_FONT })` plus its `FitAddon`; `setPaused`, `clipboardWriteHandler` as in the research file), a `ResizeSource` over `ResizeObserver`, and calls `startTerminalHost`.
- `main.ts`: the Electron main process (below).

### Terminal fixes (pro-d25)

- Hover: ghostty-web 0.3.9 reports no-button motion as SGR code 32 (a left drag), so hover dragged tmux panes. `fixSgrMouseHover` rewrites `\x1b[<32;c;rM` to code 35 while no button is pressed; the pointer buttons are tracked from capture-phase window mouse listeners in renderer.ts.
- Resize: `FitAddon.fit()` ignores calls within 50 ms of one, so the final size was dropped. `fitToContainer` uses `proposeDimensions()` and calls `term.resize` once.
- Fonts: `TERMINAL_FONT` falls back to Symbols Nerd Font Mono and MesloLGS, and renderer.ts awaits `document.fonts.ready` before opening the terminal.
- Pointer: electron.html forces `cursor: default` on `#terminal` and its canvas.

#### Terminal polish (pro-d30, pro-d32)

User bug reports pro-d30 and pro-d32, and decision pro-d33. Each item is pinned above; this list is the index.

- Shift+Enter sends the CSI-u sequence `\x1b[13;2u` to the pty (`handleKey`), not a plain Enter.
- Zoom: Cmd+= / Cmd++ / Cmd+- / Cmd+0 step the font 1px within 9..28, reset to 13, and refit the pty. Not persisted.
- Wheel: when the app owns the wheel (mouse tracking or alternate screen), one step per `max(24, 1.5 * cellHeight)` px of accumulated delta, sent to the pty; plain scrollback stays with ghostty-web.
- Cmd+V in TUI reads the clipboard and pastes (bracketed when the app asked). Cmd+C is unchanged (ghostty-web).
- Menu: `editMenu` is on every platform, so Cmd+C, Cmd+V, Cmd+X, Cmd+A and Cmd+Z work in WebUI.
- `TERMINAL_FONT` is `fontWeight: 300`, `fontSize: 13`.

## Assets (web/build/)

The macOS asset bundle's `build/` directory is copied unchanged to `web/build/`. Every `build/...` path in `electron-builder.yml`, `afterPack.cjs` and the script resolves from `web/`, because electron-builder and the script both resolve from the app root. The Finder `.DS_Store` files are not copied.

- `icon.icns`: the app icon for macOS 15 and earlier, and the fallback (`mac.icon`, `dmg.icon`).
- `icon.png`: the 1024 px icon the Dock shows at runtime (`app.dock.setIcon`, darwin only).
- `icon/`: the cpd icon as SVG and PNG sizes (16 to 1024 px). Nothing reads it at runtime.
- `icon-composer/`: `phi.png` and `phi.svg`, the Icon Composer layers. The script turns them into `cpd.icon/`.
- `tray/trayTemplate.png` and `tray/trayTemplate@2x.png`: the menu bar icon, a template image (macOS tints it for light and dark menu bars).
- `tray/trayAttentionTemplate.png` and `tray/trayAttentionTemplate@2x.png`: the menu bar attention variant.
- `dmg/background.png` and `dmg/background@2x.png`: the DMG window background. The script builds `dmg/background.tiff` from them.
- `entitlements.mac.plist` and `entitlements.mac.inherit.plist`: hardened-runtime exceptions for the app and its helper processes (JIT, unsigned executable memory, library validation off, because `node-pty` is a dependency).
- `afterPack.cjs`: the electron-builder hook. It copies `Assets.car` (the macOS 26 Liquid Glass icon) into `Contents/Resources/` when present, otherwise it warns and ships `icon.icns`.

Generated on a Mac by `scripts/prepare-macos-assets.sh` and absent until it runs: `build/cpd.icon/`, `build/Assets.car`, `build/dmg/background.tiff`.

At runtime `native.ts` reads only the two tray images. In dev they come from `app.getAppPath()/build/`; packaged, from `process.resourcesPath/`, which holds `tray/` (the `extraResources` rule) and `icon.png`.

## Packaging

- `web/electron-builder.yml` is the electron-builder config. `appId` is `com.cpd.desktop`; `productName` is `cpd`.
- `electron-builder` is a pinned devDependency of `web/` (pro-d24).
- `files` ships the main process output (`dist-electron/**/*`) and `package.json`. `extraResources` copies `build/tray` into `Contents/Resources/tray`.
- The macOS target is arm64: a DMG and a zip. The build uses the hardened runtime and notarizes, and reads the Apple credentials from the environment.
- `dist:mac` (`pnpm --filter cpd-web-client run dist:mac`) builds the Electron bundle and runs electron-builder for macOS. The output goes to `web/dist/`.
- Run `scripts/prepare-macos-assets.sh` once on a Mac before `dist:mac`. The DMG background is `build/dmg/background.tiff`, and the script creates it.

## Native shell

The native shell is `web/src/electron/native.ts` and `web/src/electron/attention.ts`. The main process calls `setupNativeShell` once the app is ready. The Dock and Tray parts run on macOS only. Linux and Windows have no Dock and no Tray, so nothing in this section runs there.

On every platform:

- Dark theme: `nativeTheme.themeSource` is `dark`. The cockpit is dark only.
- About panel: `cpd`, the app version and the credits `Continuous Parallel Delivery`.
- External links open in the system browser. A link that asks for a new window (a PR, a deploy run) opens in the browser through `shell.openExternal`, and the app never opens a window for it.

On macOS only:

- Dock icon: `app.dock.setIcon` shows `build/icon.png` in dev, and `icon.png` in `process.resourcesPath` when packaged.
- Tray: a menu bar item with the tooltip `cpd` and the template image. Its menu, in order: `Open cockpit`, `Switch to TUI`, `Switch to WebUI`, `Switch to WebUI Preview`, a separator, `Quit cpd`. `Open cockpit` shows and focuses the window and switches to WebUI (A6). The three switch items do what the View menu does.
- Attention image: the Tray shows `trayAttentionTemplate.png` while there is something to attend to, and `trayTemplate.png` otherwise. Both images are created once at setup.
- Dock badge: blocking + decisions. The badge is empty when the sum is zero.
- Counting (A2): the main process counts the rows of the Inbox from the same cockpit service the WebUI reads. A row with urgency `red` is blocking, and a row with urgency `magenta` is a decision. Other urgencies are not counted. The count is recomputed on every cockpit update.
- Bounce: the Dock bounces once (`informational`) when blocking rises above the previous count and the window is not focused. The first count after startup is the baseline and never bounces. A rise in decisions alone does not bounce, and neither does a fall in blocking, or any rise while the window has focus.
- Dispose: the shell's `dispose` destroys the Tray. It is idempotent.

## WebSocket transport

The WebUI reaches the cockpit over one WebSocket (ADR 0015, option B, air-d46), shared by the desktop and the browser build. The frames are the existing wire frames of `web/src/protocol/wire.ts`, unchanged. They pass through `createWebSocketTransport` in the renderer and the WS server of `web/src/server/ws.ts` in the main process; the `cpd:c2s`/`cpd:s2c` IPC pair, `createElectronTransport` and `registerWebuiIpc` are gone. The full transport contract, the heartbeat and the watchdog are in docs/contract.md §5.5 and web/CONTRACT.md §15.

- Main process: the server binds `ws://127.0.0.1:<port>/cpd-ws` — loopback only, on a port the OS chooses (bind port 0). It creates one hub connection per accepted socket, through `services.newConnection(send)`; `send` writes one JSON text frame to the socket. A closed socket closes its hub. Each frame waits for the cockpit to start, then goes to its hub. A rejection goes to the error handler.
- Renderer learns the URL through the preload `contextBridge`, not over a data channel: the main process exposes the resulting `ws://127.0.0.1:<port>/cpd-ws` to the renderer as one read-only member of `window.cpdElectron` (`webSocketUrl`), and `createWebSocketTransport` connects to it. No `cpd:c2s`/`cpd:s2c` channel exists; the bridge carries only this URL and the non-data members it already had (pty, mode, wasm, health).
- Authorisation: no token. The check is the Origin alone, at connect, before any hub is created: an absent or `null` Origin (the desktop renderer runs from `file://` in production) and a loopback Origin (`localhost`, `127.0.0.1`, `[::1]`) are accepted; every other Origin is rejected. A remote thin client reaches the same endpoint through `ssh -L` (ADR 0015 §Comparison, row R3).
- Heartbeat: the server protocol-pings every socket every `WS_PING_MS` and terminates a socket that does not pong within `WS_PONG_TIMEOUT_MS`, so a stalled transport is seen loud, never silent. The client sends its own wire `ping` every `WS_CLIENT_PING_MS` and closes the socket when the server's `pong` misses `WS_CLIENT_PONG_TIMEOUT_MS`, so a hung server is seen too (the browser `WebSocket` cannot send protocol ping frames, so the client's ping rides the wire).
- Renderer: `createWebSocketTransport` has the semantics of the `Transport` interface unchanged (web/src/protocol/runtime.ts:10-15): it opens with status `open`, `send` throws before open or after close, and `close` is final. A closed socket — a terminate, an error or a heartbeat failure — is a real `closed` status, so the renderer backs off, reconnects and re-subscribes for a fresh full snapshot (web/src/state/connection.ts:205-224).
- Both views stay mounted, so a switch is instant and keeps the session state. The WebUI keeps its connection and selection, and the TUI keeps its pty.
- TUI mode: the terminal is shown, resumed, refitted to its container and focused. Menu shortcuts are ignored, so the keys reach cpd.
- WebUI mode: the terminal is paused and hidden, and the WebUI is shown. Menu shortcuts work, including `CmdOrCtrl+Shift+M`.
- WebUI Preview mode: the window loads the dev renderer of a preview worktree (see [Previewing a UI change](../docs/ui-preview.md)); the terminal is paused and hidden. Menu shortcuts work, including `CmdOrCtrl+Shift+M`.
- The chosen mode lasts for the run only: no mode is persisted, `$CPD_HOME/run/ui-mode.json` is deleted once on the first launch, and the run-only marker `$CPD_HOME/run/ui-view.json` is written instead (see Module contract, mode.ts).

## Remote TUI tabs over ssh

cpd desktop shows one terminal per tab: the local tab, and one remote tab per ssh host (air-d33). This section is the contract for that tab list, the per-tab pty and its IPC; the WebUI data plane for a remote cockpit is future work and is not documented as shipped here.

### Tabs and their hosts

- The host source is `~/.ssh/config`: the tab list is the `Host` blocks that declare an explicit `HostName`, with `Include` directives resolved transitively and wildcard patterns (for example `Host *`) skipped. There is no new cpd setting for this (air-d34).
- Tab order: `local` first, then the ssh aliases in file order of the resolved config.
- Tab ids: the local tab is `local`; a remote tab is `ssh:<alias>` (for example `ssh:homeserver`). The tab label is the alias.

### Per-tab pty

- Each tab has exactly one pty, created when the tab is first selected and kept for the life of the app. Switching tabs never restarts a live pty; a hidden tab keeps running.
- The local tab's pty runs the local default command (`CPD_TUI_COMMAND`: `cpd tui --attach`). A remote tab's pty runs `ssh -t <alias> cpd tui --attach`, so the remote cockpit attaches over ssh.
- Status values per tab: `connecting` (the pty is spawning or the ssh connection is not up yet), `live`, `exited` (the process ended normally), `failed`. A `failed` tab carries a reason: the ssh exit code, or `cpd not found on <host>` when the remote command is missing (exit code 127).
- Nothing reconnects on its own. Clicking a `failed` or `exited` tab restarts that tab's pty and nothing else; the other tabs are untouched.

### Visibility and the view

- The host tabs (`local` plus every remote tab) show only in the TUI view: they are hidden in WebUI and WebUI Preview (air-d50).
- Clicking a remote tab switches the view to TUI for this run (air-d40) and connects that tab's pty if it has none. The `local` tab follows the view the View menu selects; how the menu and the mode switch work is owned by mode.ts and menu.ts (see Module contract and the View menu items) and is not restated here.

### IPC channels

The pty channels carry a tab id on every payload. Existing channel names (`pty:start`, `pty:write`, `pty:resize`, `pty:data`, `pty:exit`, see Module contract, channels.ts) keep their names and gain the id; `pty:status` and `pty:hosts` are new.

- Main to renderer:
  - `pty:data` with `{tabId, data}`: a raw terminal chunk for that tab.
  - `pty:exit` with `{tabId, code}`: the process exit code for that tab.
  - `pty:status` with `{tabId, status, code?}`: the tab's status (`connecting` | `live` | `exited` | `failed`), with `code` present when the status is `exited` or `failed`.
- Renderer to main:
  - `pty:start` invoke with `{tabId, cols, rows}`: starts (or, after `exited`/`failed`, restarts) that tab's pty.
  - `pty:write` with `{tabId, data}`: keystrokes and pastes for that tab.
  - `pty:resize` with `{tabId, cols, rows}`: refit that tab's pty.
  - `pty:hosts` invoke with no payload: returns the resolved hosts from `~/.ssh/config`, in tab order; the renderer builds the tab list from it.

Keys, resize, OSC 52 copy, paste and zoom behave per tab exactly as the terminal.ts contract (Module contract) requires; the Verification checklist below covers the remote tabs as well as the local one.

## Auto-update and CLI bootstrap

### Overview

The Electron app and the `cpd` CLI are two binaries, both built from the cpd checkout. A push to `main` is a release: each `pr-merged` Event, and a catch-up check at launch, syncs the checkout to `origin/main` (fast-forward; in preview mode a rebase of local commits, see `checkout.ts`), builds the app bundle and installs the CLI. Nothing stops for it: the renderer reloads only while the window is unfocused or the system has been idle for 300 s, and never takes focus. An update that changes any non-renderer build artefact (`main.js`, `preload.cjs`, `server.js` or another main-side artefact) relaunches the Electron main process with `app.relaunch()` + `app.exit()`. A renderer-only change keeps the in-place renderer reload.

Scope (A1): the app run from the cpd checkout. The packaged `cpd.app` does not replace itself; with no checkout found, auto-update logs one line and stays off.

### Contract

All modules live in `web/src/electron/update/`. Each takes every external effect (process, file system, timer, focus, idle time, log) as an injected dependency. Tests use injected fakes only: no test starts a real process, timer or socket. The 11 interface names of `web/src/types-unique.test.ts` are not redeclared.

#### types.ts

```ts
export interface UpdateExecResult { code: number; stdout: string; stderr: string }
export type UpdateExec = (cmd: string, args: string[], opts: { cwd: string; env?: Record<string, string> }) => Promise<UpdateExecResult>;
export interface UpdateFs { exists(p: string): boolean; readText(p: string): string | null; writeText(p: string, text: string): void; rename(from: string, to: string): void; remove(p: string): void }
export interface UpdateTimers { setTimeout(fn: () => void, ms: number): unknown; clearTimeout(handle: unknown): void }
export type BootstrapStatus = { state: 'ready' } | { state: 'installing' } | { state: 'failed'; error: string; logPath: string };
export type UpdateState = { phase: 'idle' } | { phase: 'syncing' } | { phase: 'building' } | { phase: 'waiting' } | { phase: 'restart-needed' } | { phase: 'error'; message: string };
```

#### outdir.ts

```ts
export function electronOutDir(env: Record<string, string | undefined>): string
```

Returns `env.CPD_ELECTRON_OUT`, or `dist-electron` when it is unset. Throws an `Error` when the value is not a single relative segment of `[A-Za-z0-9._-]`: absolute paths, `/`, `.` and `..` are refused. `vite.electron.config.ts` builds the renderer, `main.js`, `server.js` and `preload.cjs` under this directory; the default is unchanged.

#### checkout.ts

```ts
export function resolveCheckout(deps: { env: Record<string, string | undefined>; bundleDir: string; fs: Pick<UpdateFs, 'exists'> }): string | null
export type SyncResult = { kind: 'current'; sha: string } | { kind: 'updated'; from: string; to: string; lockfileChanged: boolean } | { kind: 'skipped'; reason: 'fetch-failed' | 'diverged' | 'rebase-conflict' };
export function syncCheckout(exec: UpdateExec, root: string, opts: { readonly rebase: boolean }): Promise<SyncResult>
```

The root is `env.CPD_CHECKOUT`, else `path.resolve(bundleDir, '..', '..')` (A3). `resolveCheckout` returns the root only when `fs.exists(<root>/scripts/install-cpd.sh)` and `fs.exists(<root>/.git)` are both true, else `null`.

`syncCheckout` syncs a checkout to `origin/main` on any branch (A7). The steps run in this order; each returns once it has decided:

1. `git fetch origin`; it exits non-zero: skipped, `fetch-failed`.
2. HEAD and `origin/main` are equal: `current` with that `sha`.
3. `git diff --name-only HEAD origin/main` lists the changed paths; `git status --porcelain --untracked-files=all` lists the dirty paths. Of the changed paths that are dirty and tracked, `git checkout HEAD -- <those paths>` runs (only when there are any). Of the changed paths that are untracked (`??`), `git clean -f -- <those paths>` runs (only when there are any). Other dirty paths are left alone.
4. `git merge-base --is-ancestor HEAD origin/main` exits 0: `git merge --ff-only origin/main`; non-zero: skipped, `diverged`.
5. Otherwise, with `opts.rebase`: `git rebase --autostash origin/main`; non-zero runs `git rebase --abort` and returns skipped, `rebase-conflict`. Without `opts.rebase`: skipped, `diverged`.
6. `updated` is `{ kind: 'updated', from, to, lockfileChanged }`, with `lockfileChanged` true when the changed paths contain `pnpm-lock.yaml`.

`start.ts` passes `{ rebase: readPreviewMode(deps.env).mode === 'preview' }`: production never rebases, preview rebases a checkout with local commits (contract: [docs/ui-preview.md](../docs/ui-preview.md#production-and-preview-modes), item 7 and A7).

#### bundle.ts

Paths are relative to the `root` argument.

```ts
export const BUILT_SHA_FILE = '.built-sha';
export function buildBundle(deps: { exec: UpdateExec; fs: UpdateFs }, root: string, opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }>
export function readBuiltSha(fs: UpdateFs, root: string): string | null
export function swapBundle(fs: UpdateFs, root: string): { mainChanged: boolean }
```

`buildBundle` removes `<root>/web/dist-electron.next` first. When `lockfileChanged` is true it runs `exec('pnpm', ['install', '--frozen-lockfile'], { cwd: root })` (A9). Then it runs `exec('pnpm', ['--filter', 'cpd-web-client', 'run', 'build:electron'], { cwd: root, env: { CPD_ELECTRON_OUT: 'dist-electron.next' } })`. A non-zero exit returns `{ ok: false, log }`, where `log` is that command's stderr and stdout, and stops. On success it writes `opts.sha` to `<root>/web/dist-electron.next/.built-sha`.

`readBuiltSha` reads `<root>/web/dist-electron/.built-sha`, trimmed, and returns `null` when it is missing.

`swapBundle` sets `mainChanged` to true when any non-renderer build artefact differs between `web/dist-electron` and `web/dist-electron.next`: the artefacts of the main and preload builds — `main.js`, `preload.cjs`, `server.js` — plus any other main-side artefact the build emits. The renderer output is excluded. A missing current file counts as different. It then renames `web/dist-electron` to `web/dist-electron.prev`, `web/dist-electron.next` to `web/dist-electron`, and removes `web/dist-electron.prev`. If the second rename throws, it renames `.prev` back and rethrows.

#### cli.ts

```ts
export function countRunningAttempts(exec: UpdateExec, root: string): Promise<number>
export function installCli(deps: { exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; log: (line: string) => void; retryMs?: number }, root: string, sha: string): Promise<{ ok: true } | { ok: false; log: string }>
export function readInstalledSha(fs: UpdateFs, root: string): string | null
```

`countRunningAttempts` runs `exec('cpd', ['plan','list'], { cwd: root })`, which prints JSON `{ plans: [{ id }] }`. For each plan it runs `exec('cpd', ['attempt','list','--active','--plan', id], { cwd: root })`, which prints JSON `{ attempts: [{ status }] }`. It counts the attempts whose status is `running`. A non-zero exit or invalid JSON throws an `Error` whose message is `<name> failed (exit <code>): <stderr>`, `<name>` being `cpd <args>`. When stderr is empty, the message parses the captured stdout as cpd's one JSON error envelope (commands.md §1.2; cpd prints every error, a refusal included, on stdout and leaves stderr empty) and the message names the envelope's `error.code` and `error.message`: a refused call (exit 3) is logged with its code and message, not as an empty tail.

`installCli` runs in four steps:

1. `exec('scripts/cargo.sh', ['build','--release','--locked','-p','cpd'], { cwd: root })`. Non-zero: returns `{ ok: false, log }`.
2. The install, at once and without waiting for running attempts: `exec('scripts/install-cpd.sh', ['--no-follow-up'], { cwd: root })` (A7). `--no-follow-up` (commands.md §14.4) skips the install's disruptive follow-ups; the binary write inside the install stays atomic (copied to `.cpd.tmp.<pid>`, then `rename`d over `~/.local/bin/cpd`), so a running `cpd` keeps its old inode and installing while attempts run is safe. Non-zero: returns `{ ok: false, log }`.
3. The follow-ups, deferred: loop `countRunningAttempts`. When the count is above 0, it logs a line through `deps.log` and waits `retryMs` (default 60000) with `timers.setTimeout`, then counts again. A throw from counting returns `{ ok: false, log: <its message> }`.
4. When the count is 0: `exec('scripts/install-cpd.sh', [], { cwd: root })`, which runs the follow-ups (the view respawn and the engine restart) on the already-swapped binary. Non-zero: returns `{ ok: false, log }`. Success writes `sha` to `<root>/target/.cli-installed-sha` and returns `{ ok: true }`.

`readInstalledSha` reads `<root>/target/.cli-installed-sha`, trimmed, and returns `null` when it is missing.

#### bootstrap.ts

```ts
export function ensureCli(deps: { platform: string; home: string; root: string | null; exec: UpdateExec; fs: UpdateFs; onStatus: (s: BootstrapStatus) => void }): Promise<BootstrapStatus>
```

The first case that applies is taken:

- `platform` is not `'darwin'`, or `fs.exists(<home>/.local/bin/cpd)`: `onStatus({ state: 'ready' })`, return it, run nothing.
- `root` is `null`: `failed` with error `cpd checkout not found: set CPD_CHECKOUT` and `logPath` `<home>/.cpd/run/cli-bootstrap.log`.
- Otherwise: `onStatus({ state: 'installing' })`, then `exec('scripts/install-cpd.sh', [], { cwd: root })`. A non-zero exit writes stdout and stderr to `logPath` with `fs.writeText` and returns `failed`, with `error` set to the last non-empty line of stderr. An exit 0 with `<home>/.local/bin/cpd` still missing returns `failed`. Otherwise it returns `ready`.

`onStatus` receives every status returned, in order (A8).

#### follow.ts

```ts
export function parseEventId(line: string): number
export interface FollowProcess { onLine(cb: (line: string) => void): void; onExit(cb: (code: number | null) => void): void; kill(): void }
export type SpawnFollow = (args: string[]) => FollowProcess;
export function followMerges(deps: { spawn: SpawnFollow; timers: UpdateTimers; onMerged: () => void; onError: (e: Error) => void; restartMs?: number }): { stop(): void }
```

`parseEventId` accepts a JSON line with kind `pr-merged` and a numeric `id`, and returns the id. Any other line throws an `Error`.

`followMerges` spawns `['events','follow','--kind','pr-merged']` at once and calls `onMerged()` once, as the launch catch-up. For each line it calls `parseEventId`; a throw goes to `onError` and following continues. A parsed line records the last id and calls `onMerged()`. When the process exits and `stop()` was not called, it spawns again after `restartMs` (default 5000) through `timers.setTimeout`. The arguments are `['events','follow','--kind','pr-merged','--since',String(lastId)]` when an id was seen, else `['events','follow','--kind','pr-merged']`; it calls `onMerged()` as well. `stop()` kills the process, clears the pending restart and prevents any further spawn.

#### idle.ts

```ts
export function whenApplicable(deps: { isFocused: () => boolean; systemIdleSeconds: () => number; timers: UpdateTimers; pollMs?: number; idleSeconds?: number }): Promise<void>
```

Resolves when `!isFocused()` or `systemIdleSeconds() >= idleSeconds` (default 300). It checks at once, then every `pollMs` (default 30000) through `timers.setTimeout`.

#### controller.ts

```ts
export interface UpdaterDeps { sync(): Promise<SyncResult>; headSha(): Promise<string>; builtSha(): string | null; cliSha(): string | null; build(opts: { lockfileChanged: boolean; sha: string }): Promise<{ ok: true } | { ok: false; log: string }>; swap(): { mainChanged: boolean }; whenApplicable(): Promise<void>; reloadRenderer(): void; installCli(sha: string): Promise<{ ok: true } | { ok: false; log: string }>; onState(s: UpdateState): void; log(line: string): void }
export function createUpdater(deps: UpdaterDeps): { check(): Promise<void> }
```

`SyncResult` is imported from `checkout.ts`. One run of `check()`:

1. `onState({ phase: 'syncing' })`, then `sync()`. A `skipped` result logs the reason, calls `onState({ phase: 'idle' })` and ends: no build and no CLI install.
2. `sha = headSha()`.
3. Bundle flow, when `builtSha() !== sha`: `onState({ phase: 'building' })`, then `build({ lockfileChanged, sha })`, where `lockfileChanged` is the `lockfileChanged` of the sync when it was `updated`, else `true`. A failure logs the log and calls `onState({ phase: 'error', message })`, where `message` is the log's last line; the old bundle is kept. A success calls `onState({ phase: 'waiting' })`, awaits `whenApplicable()`, then `swap()`. When `mainChanged` is true, it calls `onState({ phase: 'restart-needed' })` and then relaunches the Electron main process with `app.relaunch()` followed by `app.exit(0)`, through an injected port like every other external effect; it does NOT call `reloadRenderer`. Otherwise it calls `reloadRenderer()` and then `onState({ phase: 'idle' })`. When `builtSha() === sha`, there is no build and the state is `idle`.
4. CLI flow, concurrent with the bundle flow, when `cliSha() !== sha`: `installCli(sha)`. A failure is only logged; it never sets the `error` state.

`check()` is single-flight. A call made during a run returns that run's promise, and queues exactly one trailing run after it.

#### menu.ts

```ts
export function updateMenuItem(state: UpdateState, onRestart: () => void): { label: string; enabled: boolean; click?: () => void }
```

`restart-needed` gives `{ label: 'Restart to Apply Update', enabled: true, click: onRestart }`. `syncing`, `building` and `waiting` give `{ label: 'Updating…', enabled: false }`. `error` gives `{ label: 'Update Failed (see log)', enabled: false }`. `idle` gives `{ label: 'Up to Date', enabled: false }`.

#### start.ts

```ts
export interface AutoUpdateDeps { env: Record<string, string | undefined>; bundleDir: string; home: string; platform: string; exec: UpdateExec; fs: UpdateFs; timers: UpdateTimers; spawnFollow: SpawnFollow; isFocused: () => boolean; systemIdleSeconds: () => number; reloadRenderer: () => void; onState: (s: UpdateState) => void; onBootstrap: (s: BootstrapStatus) => void; log: (line: string) => void }
export function startAutoUpdate(deps: AutoUpdateDeps): Promise<{ bootstrap: BootstrapStatus; stop(): void }>
```

`startAutoUpdate` runs `resolveCheckout`, then `ensureCli` (awaited; its statuses are forwarded to `onBootstrap`). When the bootstrap is not `ready`, it returns without following. When the root is `null` and the CLI is present, it logs one line and returns `ready` with a no-op `stop`. Otherwise it composes `createUpdater` from the real modules: `headSha` is `git rev-parse HEAD` through `exec`, `builtSha` is `readBuiltSha`, `cliSha` is `readInstalledSha`, `build` is `buildBundle`, `swap` is `swapBundle`, and `installCli` is `installCli`. It then starts `followMerges`, whose `onMerged` runs `updater.check()`. A rejection of `check()` is logged, never thrown.

#### Renderer and main-process behaviour

- `ipc.ts`: the first `pty:start` is unchanged. A second `pty:start` on a live session never spawns another pty. It calls `session.resize(cols - 1, rows)` at once and `session.resize(cols, rows)` after 50 ms, through the injected `UpdateTimers` in `registerIpc`'s deps. The one-column nudge makes tmux redraw a freshly loaded renderer, whose terminal starts blank.
- Selection survives a reload. The selected plan id and session key are saved in `sessionStorage` under `'cpd.selection'` as JSON `{planId, sessionKey}` (`web/src/ui/state/selection.ts`: `SelectionStorage`, `PersistedSelection`, `SELECTION_KEY`, `saveSelection(storage, sel)`, `loadSelection(storage): PersistedSelection | null`). `loadSelection` returns `null` for missing, malformed or wrong-shape data. The renderer reselects it after the first cockpit snapshot of a connection.
- Bootstrap overlay: `web/src/ui/state/bootstrap-view.ts` exports `bootstrapView(status: BootstrapStatus): { visible: boolean; title: string; detail: string }`. `ready` gives `visible` false. `installing` gives `visible` true, title `Installing the cpd command tool`, and detail `Runs once in the background; the cockpit starts when it finishes.`. `failed` gives `visible` true, title `cpd command tool install failed`, and detail `${error} (log: ${logPath})`. The overlay is fed by the IPC channel `update:bootstrap`. Main publishes `UpdateState` on `update:state`, and its menu item comes from `updateMenuItem`.

### Failure modes

| Failure | What the User sees | Where the log is |
|---|---|---|
| Sync skipped (`fetch-failed`, `diverged`, `rebase-conflict`) | Nothing in the cockpit. The menu item reads `Up to Date`. The app keeps the bundle it has. | The updater's `log` line with the reason. |
| Build failed (`pnpm install` or `build:electron` exits non-zero) | The menu item reads `Update Failed (see log)`, disabled. The old bundle stays and the renderer is not reloaded. | The updater's `log` line: the failing command's stdout and stderr. The last line is the menu's message. |
| CLI install failed (the `cargo build`, either `install-cpd.sh` run, or the attempt count fails) | Nothing. A CLI failure never sets the error state, and the bundle update is unaffected. `target/.cli-installed-sha` is not written, so the next run retries. | The updater's `log` line: the failing command's output, or the counting error's message, which names a refused cpd call's `error.code` and `error.message` when stderr was empty. |
| Bootstrap failed: no checkout, `install-cpd.sh` exits non-zero, or `~/.local/bin/cpd` is still missing after exit 0 | The overlay `cpd command tool install failed`, with the error and the log path. | `~/.cpd/run/cli-bootstrap.log` holds the stdout and stderr of `install-cpd.sh`. The file is written only when the script exits non-zero, so with no checkout the overlay text is the only report. |

The updater's `log` is injected by the composition root, and this contract does not fix a file for it. Where those lines land is unverified for the running app.

A changed non-renderer build artefact (`main.js`, `preload.cjs`, `server.js` or another main-side artefact) never reloads the renderer in place: the updater reports `restart-needed` and relaunches the Electron main process with `app.relaunch()` + `app.exit(0)`, so the new main-process, preload and server code load in a fresh process. Per plan view-menu (air-d40), the relaunched app keeps the WebUI view and restores no persisted TUI mode. A renderer-only change keeps the in-place renderer reload.

## Verification checklist

Run each check against the built app (`pnpm --filter cpd-web-client run electron`). A check passes only when its observable result is seen.

1. The window opens full-window with background `#1c1a20`, and the `cpd tui --attach` cockpit renders. Result: the terminal fills the window, no margin shows another colour, and the cockpit is drawn.
2. Keys reach cpd tui / pi, on every tab (local and remote). Type text, then press Ctrl-W, Ctrl-T, Ctrl-N and Esc. Result: each key acts inside that tab's cockpit (in TUI mode the menu shortcuts are ignored, so the keys are not taken by the menu).
3. Colours match the Dango palette. Run `for i in $(seq 0 15); do printf '\033[48;5;%dm  ' $i; done; printf '\033[0m\n'`. Result: 16 colour blocks in the Dango v3 colours (black `#5a504a`, red `#e17f6e`, green `#8da96e`, yellow `#c29653`, blue `#7aa2ce`, magenta `#ca88a3`, cyan `#67ada7`, white `#aba3af`, then the bright set `#87808c`, `#ffa796`, `#b3cd96`, `#e4bb7f`, `#a1c6ef`, `#ecaec7`, `#92d1ca`, `#f3ead9`).
4. Maple Mono is used. Result: glyphs match Maple Mono (for example `0`, `O`, `l`, `1`, `|` are distinct). Uninstalling Maple Mono changes the glyphs, so the check is meaningful only with it installed.
5. Resizing the window refits the cockpit, on every tab (local and remote): the tab being viewed refits, and each hidden tab refits on its next selection. Drag the window edges. Result: the cockpit redraws at the new size with no wrapped or stray lines from the old width.
6. OSC 52 copy from inside pi lands in the macOS clipboard, on every tab (local and remote). Copy a message from pi, then run `pbpaste` in a shell. Result: `pbpaste` prints the copied text.
7. Cmd+V pastes the clipboard into the focused tab (local or remote), bracketed when the app asked; an empty clipboard pastes nothing; Cmd+C is unchanged. Copy a message inside a remote tab's cockpit, focus it, press Cmd+V. Result: the text is pasted into that tab's cockpit.
8. Zoom (Cmd+= / Cmd++ / Cmd+- / Cmd+0) steps the font of the focused tab (local or remote) within 9..28, resets to 13, and refits that tab's pty; it is not persisted. Zoom a remote tab. Result: the font changes and the remote cockpit redraws at the new size; after a restart the font is back to the default.
9. View > WebUI hides the terminal, shows the WebUI cockpit and pauses drawing. Result: the terminal disappears, the cockpit is shown with its panes, and output stops being drawn.
10. View > TUI restores the terminal with focus and the pty session intact. Result: the terminal reappears with the cursor focused, and the cockpit state is unchanged (no restart; the `cpd tui` process id is the same).
11. The choice lasts for the run only. On the first launch with this build, `$CPD_HOME/run/ui-mode.json` is deleted once with a log line. While the app runs, `$CPD_HOME/run/ui-view.json` holds `{"view":"<mode>","pid":<pid>}` and changes on every View menu switch; a graceful quit removes it. Set View > WebUI Preview, quit, relaunch. Result: the app opens in WebUI again, `ui-mode.json` is absent, and `ui-view.json` is absent after the quit (and says `webui-preview` while the preview ran).
12. Closing the window quits the app and leaves no orphan process. Close the window, then run `pgrep -fl 'cpd tui'`. Result: no line is printed, so no `node-pty` or `cpd tui` process remains.
13. The renderer is WebGL. In devtools, `renderer` shows `webgl` (`term.rendererType`, when available). Result: `webgl`. If `term.rendererType` is not exposed by the build, this check is unverified, not passed.
14. The accelerator caveat. In TUI mode, CmdOrCtrl+Shift+M does not fire, because menu shortcuts are ignored so terminal keys pass through. Result: nothing changes. Switch to WebUI with View > WebUI. Switch back to TUI with CmdOrCtrl+Shift+M or with View > TUI. Result: in WebUI the accelerator switches to TUI.

## Automated checks

Part of the manual verification checklist above is automated by the Playwright Electron e2e suite under `web/src/testing/`: it drives the real Electron binary against the scripted fake cpd backend and guards the boot, tree and update scenarios the unit tests could not catch (air-n31, air-n37). It is the one named exception to the tests-mock-everything rule, never runs in the Gate, and runs in a separate, non-blocking CI job (docs/implementing.md, `The Electron e2e exception (air-d53)`; decisions air-d53 and air-d55).

Run the e2e suite locally from the repository root:

```sh
pnpm --filter cpd-web-client run e2e
```

On a headless Linux box (no display server):

```sh
xvfb-run -a pnpm --filter cpd-web-client run e2e
```

Run these from the repository root:

```sh
pnpm --filter cpd-web-client run typecheck
pnpm --filter cpd-web-client run test
pnpm --filter cpd-web-client run build:electron
pnpm run verify:steps
```

`pnpm run verify:steps` is the full Gate chain. The GitHub Actions check runs `pnpm run verify` on the self-hosted runner, which runs this chain for every pull request.
