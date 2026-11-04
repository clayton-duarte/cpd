# Electron app

The Electron app is a standalone Electron desktop app in `web/` with two views of the cockpit. TUI runs `cpd tui --attach` in a full-window terminal. The terminal is `@aurigax-ai/ghostty-web` 0.3.9 (libghostty-vt WASM, WebGL 2 renderer with a silent Canvas 2D fallback), backed by `node-pty` in the Electron main process. It draws the Dango v3 16-colour palette in Maple Mono 13px with ligatures off (ghostty-web has no ligature option), WebUI is the Dango v3 cockpit (the same `App` the browser build mounts), fed by the main process over IPC. A native View menu switches between TUI (the default) and WebUI (`CmdOrCtrl+Shift+M`). On macOS the app also owns the Dock icon, the Dock badge, the bounce and the menu bar (Tray) icon. Decisions: pro-d21 (D5, D6, D7, dual-mode), pro-d22 (TUI), pro-d23 (native shell and live WebUI over IPC) and pro-d24 (packaging).

## Run

Run the app from the repository root:

```sh
pnpm --filter cpd-web-client run electron
```

The script runs `build:electron`, then makes node-pty's `spawn-helper` executable. node-pty 1.1.0 ships it with mode 0644, and without the execute bit the pty fails with `posix_spawnp failed`. It then launches Electron. Electron downloads its binary lazily on the first launch, so the first run needs network access.

Prerequisites:

- `pnpm install` has been run in the repository root.
- `cpd` and `tmux` are installed and on the PATH. The pty runs `cpd tui --attach` in your home directory with your login-shell PATH.
- Maple Mono is installed on the system. It is not bundled; without it the terminal falls back to the generic monospace font.

Why `allowBuilds` in `pnpm-workspace.yaml` sets `electron` and `node-pty` to `false`: frozen installs, typecheck and vitest in worktrees and CI need no compiler and no Electron binary. node-pty is imported only in the Electron main process and is mocked in tests.

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
export type UiMode = "tui" | "webui";
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
- The View submenu is exactly two items in this order: `{ label: "TUI", type: "radio", checked: mode === "tui", click }` and `{ label: "WebUI", type: "radio", checked: mode === "webui", accelerator: "CmdOrCtrl+Shift+M", click }`. TUI has no accelerator. The `click` of each calls `onSelect("tui")` / `onSelect("webui")` (the click arguments are ignored).

### mode.ts (UI mode persistence and controller)

```ts
export const DEFAULT_MODE: UiMode;   // "tui"
export function parseUiMode(raw: unknown): UiMode;
export function modeFilePath(cpdHome: string): string;
export interface ModeFs {
  readFile(path: string): Promise<string>;      // rejects with an error whose `code` is "ENOENT" when the file is missing
  writeFile(path: string, data: string): Promise<void>;
  mkdir(path: string): Promise<void>;           // recursive; the real adapter passes { recursive: true }
}
export function readMode(fs: ModeFs, file: string): Promise<UiMode>;
export function writeMode(fs: ModeFs, file: string, mode: UiMode): Promise<void>;
export interface ModeWindow { setIgnoreMenuShortcuts(ignore: boolean): void; sendMode(mode: UiMode): void }
export interface MenuHost { setApplicationMenu(template: MenuTemplate): void }
export interface ModeControllerDeps {
  fs: ModeFs; file: string; menu: MenuHost; window: ModeWindow;
  platform: NodeJS.Platform; onError(error: unknown): void;
}
export interface ModeController { init(): Promise<void>; getMode(): UiMode; setMode(mode: UiMode): Promise<void> }
export function createModeController(deps: ModeControllerDeps): ModeController;
```

- `parseUiMode(raw)`: returns `raw` when it is `"tui"` or `"webui"`; otherwise throws `Error("invalid ui mode: " + JSON.stringify(raw))`.
- `modeFilePath(cpdHome)` = `path.join(cpdHome, "run", "ui-mode.json")`.
- `readMode`: a rejection with `code === "ENOENT"` returns `"tui"`; any other fs error propagates unchanged. Otherwise the content must be JSON of the shape `{"mode": "tui" | "webui"}`; invalid JSON, a non-object or an invalid mode throws an `Error` whose message contains `file` (no fallback to `"tui"`).
- `writeMode`: `fs.mkdir(path.dirname(file))` first, then `fs.writeFile(file, JSON.stringify({ mode }) + "\n")`.
- Controller: `getMode()` is `"tui"` until `init()` resolves. `init()` reads the persisted mode with `readMode` and applies it (below) unconditionally. `setMode(mode)`: when `mode` equals the current mode it does nothing (no write, no menu, no window call); otherwise it calls `writeMode` first (a failure rejects `setMode` and leaves the current mode and the menu unchanged), then updates the current mode and applies it. Applying = `menu.setApplicationMenu(buildMenuTemplate({ mode, platform, onSelect }))`, then `window.setIgnoreMenuShortcuts(mode === "tui")`, then `window.sendMode(mode)`, in that order. The template's `onSelect(m)` is `controller.setMode(m).catch(deps.onError)`.

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

- `DANGO_THEME` values (Dango v3, from dotfiles themes/Dango): foreground `#d8d2cc`, background `#1c1a20`, cursor `#ecaec7`, cursorAccent `#1c1a20`, selectionBackground `#5a504a`, selectionForeground `#d8d2cc`; black `#5a504a`, red `#e17f6e`, green `#8da96e`, yellow `#c29653`, blue `#7aa2ce`, magenta `#ca88a3`, cyan `#67ada7`, white `#aba3af`; brightBlack `#87808c`, brightRed `#ffa796`, brightGreen `#b3cd96`, brightYellow `#e4bb7f`, brightBlue `#a1c6ef`, brightMagenta `#ecaec7`, brightCyan `#92d1ca`, brightWhite `#f3ead9`. `TERMINAL_FONT` = `{ fontFamily: '"Maple Mono", monospace', fontSize: 13 }` (ligatures: ghostty-web has no ligature option and draws per cell).
- `startTerminalHost`, in this order: `engine.open()`; `engine.fit()`; register `bridge.onPtyData(engine.write)`; register `bridge.onPtyExit(code => engine.write("\r\n[cpd tui exited with code " + code + "]\r\n"))`; `engine.onData(bridge.writePty)`; `resize.observe(() => engine.fit())`; `engine.setClipboardWriteHandler(text => { deps.clipboard.writeText(text).catch(deps.onError); return true; })`; `const mode = await bridge.getMode()` and apply it; `bridge.onMode(apply)`; `await bridge.startPty(engine.size())`; and only then `engine.onResize(size => bridge.resizePty(size))`. Every listener is registered before `startPty` is called, so no pty data is lost, and no resize reaches main before the pty exists.
- Apply mode `m`: `engine.setPaused(m !== "tui")`; `view.showTerminal(m === "tui")`; `engine.focus()` only when `m === "tui"`.
- The returned `dispose()` calls every unsubscribe function it got (bridge listeners, the resize unobserve) and `engine.dispose()`.

### Composition roots (not unit-tested, thin)

- `preload.ts`: `contextBridge.exposeInMainWorld("cpdElectron", createBridge(ipcRenderer))`.
- `renderer.ts`: reads `window.cpdElectron`, loads `Ghostty.fromBytes(await bridge.readWasm())`, builds a `TerminalEngine` over `@aurigax-ai/ghostty-web` (`new Terminal({ ghostty, renderer: "webgl", theme: DANGO_THEME, ...TERMINAL_FONT })` plus its `FitAddon`; `setPaused`, `clipboardWriteHandler` as in the research file), a `ResizeSource` over `ResizeObserver`, and calls `startTerminalHost`.
- `main.ts`: the Electron main process (below).

### Terminal fixes (pro-d25)

- Hover: ghostty-web 0.3.9 reports no-button motion as SGR code 32 (a left drag), so hover dragged tmux panes. `fixSgrMouseHover` rewrites `\x1b[<32;c;rM` to code 35 while no button is pressed; the pointer buttons are tracked from capture-phase window mouse listeners in renderer.ts.
- Resize: `FitAddon.fit()` ignores calls within 50 ms of one, so the final size was dropped. `fitToContainer` uses `proposeDimensions()` and calls `term.resize` once.
- Fonts: `TERMINAL_FONT` falls back to Symbols Nerd Font Mono and MesloLGS, and renderer.ts awaits `document.fonts.ready` before opening the terminal.
- Pointer: electron.html forces `cursor: default` on `#terminal` and its canvas.

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
- Tray: a menu bar item with the tooltip `cpd` and the template image. Its menu, in order: `Open cockpit`, `Switch to TUI`, `Switch to WebUI`, a separator, `Quit cpd`. `Open cockpit` shows and focuses the window and switches to WebUI (A6). The two switch items do what the View menu does.
- Attention image: the Tray shows `trayAttentionTemplate.png` while there is something to attend to, and `trayTemplate.png` otherwise. Both images are created once at setup.
- Dock badge: blocking + decisions. The badge is empty when the sum is zero.
- Counting (A2): the main process counts the rows of the Inbox from the same cockpit service the WebUI reads. A row with urgency `red` is blocking, and a row with urgency `magenta` is a decision. Other urgencies are not counted. The count is recomputed on every cockpit update.
- Bounce: the Dock bounces once (`informational`) when blocking rises above the previous count and the window is not focused. The first count after startup is the baseline and never bounces. A rise in decisions alone does not bounce, and neither does a fall in blocking, or any rise while the window has focus.
- Dispose: the shell's `dispose` destroys the Tray. It is idempotent.

## IPC WebUI transport

The WebUI reaches the cockpit over one IPC pair per window (A4): `cpd:c2s` (renderer to main) and `cpd:s2c` (main to renderer). The frames are the existing wire frames of `web/src/protocol/wire.ts`, unchanged. They pass through `createElectronTransport` in the renderer and `registerWebuiIpc` in the main process.

- Main process: the first frame from a window creates one hub connection for that window, through `services.newConnection(send)`. `send` writes to `cpd:s2c` unless the window is destroyed. When the window is destroyed its hub is closed and forgotten. Each frame waits for the cockpit to start, then goes to its hub. A rejection goes to the error handler.
- Renderer: `createElectronTransport` has the same semantics as the browser transport (`createHotTransport`): it opens with status `open`, `send` throws before open or after close, and `close` is final.
- Both views stay mounted, so a switch is instant and keeps the session state. The WebUI keeps its connection and selection, and the TUI keeps its pty.
- TUI mode: the terminal is shown, resumed, refitted to its container and focused. Menu shortcuts are ignored, so the keys reach cpd.
- WebUI mode: the terminal is paused and hidden, and the WebUI is shown. Menu shortcuts work, including `CmdOrCtrl+Shift+M`.
- The chosen mode persists in `$CPD_HOME/run/ui-mode.json` (see Module contract, mode.ts).

## Verification checklist

Run each check against the built app (`pnpm --filter cpd-web-client run electron`). A check passes only when its observable result is seen.

1. The window opens full-window with background `#1c1a20`, and the `cpd tui --attach` cockpit renders. Result: the terminal fills the window, no margin shows another colour, and the cockpit is drawn.
2. Keys reach cpd tui / pi. Type text, then press Ctrl-W, Ctrl-T, Ctrl-N and Esc. Result: each key acts inside the cockpit (in TUI mode the menu shortcuts are ignored, so the keys are not taken by the menu).
3. Colours match the Dango palette. Run `for i in $(seq 0 15); do printf '\033[48;5;%dm  ' $i; done; printf '\033[0m\n'`. Result: 16 colour blocks in the Dango v3 colours (black `#5a504a`, red `#e17f6e`, green `#8da96e`, yellow `#c29653`, blue `#7aa2ce`, magenta `#ca88a3`, cyan `#67ada7`, white `#aba3af`, then the bright set `#87808c`, `#ffa796`, `#b3cd96`, `#e4bb7f`, `#a1c6ef`, `#ecaec7`, `#92d1ca`, `#f3ead9`).
4. Maple Mono is used. Result: glyphs match Maple Mono (for example `0`, `O`, `l`, `1`, `|` are distinct). Uninstalling Maple Mono changes the glyphs, so the check is meaningful only with it installed.
5. Resizing the window refits the cockpit. Drag the window edges. Result: the cockpit redraws at the new size with no wrapped or stray lines from the old width.
6. OSC 52 copy from inside pi lands in the macOS clipboard. Copy a message from pi, then run `pbpaste` in a shell. Result: `pbpaste` prints the copied text.
7. View > WebUI hides the terminal, shows the WebUI cockpit and pauses drawing. Result: the terminal disappears, the cockpit is shown with its panes, and output stops being drawn.
8. View > TUI restores the terminal with focus and the pty session intact. Result: the terminal reappears with the cursor focused, and the cockpit state is unchanged (no restart; the `cpd tui` process id is the same).
9. The choice persists across restarts in `$CPD_HOME/run/ui-mode.json`. Set View > WebUI, quit, relaunch. Result: the app opens in WebUI and the file holds `{"mode":"webui"}`. Set View > TUI and the file holds `{"mode":"tui"}`.
10. Closing the window quits the app and leaves no orphan process. Close the window, then run `pgrep -fl 'cpd tui'`. Result: no line is printed, so no `node-pty` or `cpd tui` process remains.
11. The renderer is WebGL. In devtools, `renderer` shows `webgl` (`term.rendererType`, when available). Result: `webgl`. If `term.rendererType` is not exposed by the build, this check is unverified, not passed.
12. The accelerator caveat. In TUI mode, CmdOrCtrl+Shift+M does not fire, because menu shortcuts are ignored so terminal keys pass through. Result: nothing changes. Switch to WebUI with View > WebUI. Switch back to TUI with CmdOrCtrl+Shift+M or with View > TUI. Result: in WebUI the accelerator switches to TUI.

## Automated checks

Run these from the repository root:

```sh
pnpm --filter cpd-web-client run typecheck
pnpm --filter cpd-web-client run test
pnpm --filter cpd-web-client run build:electron
pnpm run verify:steps
```

`pnpm run verify:steps` is the full Gate chain. The GitHub Actions check runs `pnpm run verify` on the self-hosted runner, which runs this chain for every pull request.
