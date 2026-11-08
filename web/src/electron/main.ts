import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import { createRequire } from "node:module";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, dialog as electronDialog, ipcMain, Menu, nativeImage, nativeTheme, powerMonitor, shell as electronShell, Tray } from "electron";
import { boot } from "./boot.ts";
import { createPreviewClient, type PreviewClient } from "./preview-client.ts";
import { createPreviewWatchdog } from "./watchdog.ts";
import { parseHealth, type HealthRecord } from "./health.ts";
import { DEFAULT_DEV_URL } from "./preview-mode.ts";
import * as nodePty from "node-pty";
import { createServices, realBoundaries } from "../server/plugin.ts";
import { watchAttention } from "./attention.ts";
import { CHANNELS, UPDATE_BOOTSTRAP_CHANNEL, UPDATE_STATE_CHANNEL, type UiMode } from "./channels.ts";
import { createListenerFactory, createWsServer, WS_PATH } from "../server/ws.ts";
import { resolveLoginPath, stripSessionEnv } from "./env.ts";
import { createHealthRecorder } from "./health.ts";
import { attachHealth } from "./health-main.ts";
import { registerIpc } from "./ipc.ts";
import { createModeController } from "./mode.ts";
import { createPtyTabs, tabCommand } from "./pty-tabs.ts";
import { resolveSshHosts } from "./ssh-config.ts";
import type { SshHost } from "./ssh-config.ts";
import { createExternalLinkHandler, resolveAssetPaths, setupNativeShell } from "./native.ts";
import type { MenuTemplate } from "./menu.ts";
import { nodeExec, nodeFs, nodeSpawnFollow, nodeTimers } from "./update/node.ts";
import { startAutoUpdate } from "./update/start.ts";
import type { BootstrapStatus, UpdateState } from "./update/types.ts";
import { startPty } from "./pty.ts";
import type { PtyBackend } from "./pty.ts";

const here = path.dirname(fileURLToPath(import.meta.url));

const run = (file: string, args: readonly string[]): Promise<string> =>
  new Promise((resolve, reject) => {
    execFile(file, [...args], { timeout: 10_000 }, (error, stdout) => {
      if (error) reject(error);
      else resolve(stdout);
    });
  });

// `preview-web.sh serve|unserve` is long-running (it builds the worktree on first serve): 15 min.
const previewRun = (file: string, args: readonly string[]): Promise<{ code: number; stderr: string }> =>
  new Promise((resolve) => {
    execFile(file, [...args], { timeout: 15 * 60_000 }, (error, _stdout, stderr) => {
      const raw = (error as { code?: unknown } | null)?.code;
      const code = error ? (typeof raw === "number" ? raw : 1) : 0;
      resolve({ code, stderr: typeof stderr === "string" ? stderr : "" });
    });
  });

/** The built-in loading page shown while a preview server starts and its health turns healthy. */
const LOADING_PAGE = `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>CPD</title>` +
  `<style>html,body{margin:0;height:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;` +
  `gap:16px;background:#1c1a20;color:#d8d2cc;font:14px/1.4 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}` +
  `.spinner{width:28px;height:28px;border:3px solid #4a4450;border-top-color:#ecaec7;border-radius:50%;animation:spin 1s linear infinite}` +
  `@keyframes spin{to{transform:rotate(360deg)}}</style></head><body><div class="spinner"></div><p>Starting WebUI Preview…</p></body></html>`;

const loadingPageUrl = (): string => `data:text/html;charset=utf-8,${encodeURIComponent(LOADING_PAGE)}`;

/** The checkout root, resolved the way `cpd desktop --preview` does: the app-root install state, or `CPD_CHECKOUT`. */
async function readCheckoutRoot(env: NodeJS.ProcessEnv): Promise<string | null> {
  const explicit = env["CPD_CHECKOUT"];
  if (explicit) return explicit;
  try {
    const root = (await fs.readFile(path.join(os.homedir(), ".local", "share", "cpd", "app-root"), "utf8")).trim();
    return root || null;
  } catch {
    return null;
  }
}

const backend: PtyBackend = {
  spawn: (file, args, options) => nodePty.spawn(file, args, options),
};

/** The part of a BrowserWindow that `sendToWindow` touches: its liveness and its webContents. */
export interface SendTarget {
  isDestroyed(): boolean;
  webContents: { isDestroyed(): boolean; send(channel: string, payload: unknown): void };
}

/**
 * Send `payload` on `channel` to `win`'s renderer. Exported so the pty-teardown regression
 * (web/src/electron/main.test.ts, air-n62) can drive this path with a window double. A destroyed
 * window must be rejected before `webContents` is read: reading it on a closed BrowserWindow
 * throws `Object has been destroyed`. The `webContents.isDestroyed()` check stays as the last
 * guard for a window whose native handle survives while its renderer is gone.
 */
export function sendToWindow(win: SendTarget | undefined, channel: string, payload: unknown): void {
  if (!win || win.isDestroyed()) return;
  if (!win.webContents.isDestroyed()) win.webContents.send(channel, payload);
}

async function main(): Promise<void> {
  // The cpd Session that launched the app is not the app: strip its per-session
  // identity from `process.env` once, before any cpd child is spawned. Every
  // cpd call of the updater inherits `{ ...process.env }` (update/node.ts), so
  // this covers them all; CPD_HOME, PATH and every other variable stay
  // (contract §6.6; web/ELECTRON.md "Boot order").
  process.env = stripSessionEnv(process.env);
  let win!: BrowserWindow;
  const send = (channel: string, payload: unknown): void => {
    sendToWindow(win, channel, payload);
  };
  let env: NodeJS.ProcessEnv = process.env;
  let cpdHome = "";
  let sshHosts: SshHost[] = [];
  let assets!: ReturnType<typeof resolveAssetPaths>;
  let nativeShell!: ReturnType<typeof setupNativeShell>;
  let controller!: ReturnType<typeof createModeController>;
  // Assigned by initMode; the native shell is set up before the window.
  let controllerRef: ReturnType<typeof createModeController> | undefined;
  // The in-place WebUI Preview client and its tick loop, created on the first switch to the preview.
  let previewClient: PreviewClient | undefined;
  let previewTimer: ReturnType<typeof setInterval> | undefined;
  let previewPrevious: UiMode = "webui";
  const disposers: (() => void)[] = [];
  let services!: ReturnType<typeof createServices>;
  let started!: Promise<void>;
  let markStarted: () => void = () => {};
  let failStarted: (error: unknown) => void = () => {};
  let webSocketUrl = "";

  // Everything that needs the app to be ready but not the window: runs inside boot's whenReady.
  const prepare = async (): Promise<void> => {
  const loginPath = await resolveLoginPath({
    platform: process.platform,
    shell: process.env["SHELL"],
    currentPath: process.env["PATH"],
    run,
  });
  env = { ...process.env, PATH: loginPath };
  cpdHome = process.env["CPD_HOME"] ?? path.join(os.homedir(), ".cpd");
  sshHosts = await resolveSshHosts(path.join(os.homedir(), ".ssh", "config"));

  assets = resolveAssetPaths({
    packaged: app.isPackaged,
    resourcesPath: process.resourcesPath,
    appPath: app.getAppPath(),
  });
  nativeShell = setupNativeShell({
    platform: process.platform,
    app,
    nativeTheme,
    nativeImage,
    createTray: (image) => {
      const tray = new Tray(image as Electron.NativeImage);
      return {
        setImage: (next) => tray.setImage(next as Electron.NativeImage),
        setToolTip: (text) => tray.setToolTip(text),
        setContextMenu: (menu) => tray.setContextMenu(menu as Electron.Menu),
        destroy: () => tray.destroy(),
      };
    },
    buildMenu: (template) => Menu.buildFromTemplate(template),
    isWindowFocused: () => win?.isFocused() ?? false,
    assets,
    actions: {
      openCockpit: () => {
        if (win.isMinimized()) win.restore();
        win.show();
        win.focus();
        controllerRef?.setMode("webui").catch((error: unknown) => console.error(error));
      },
      setMode: (mode) => {
        controllerRef?.setMode(mode).catch((error: unknown) => console.error(error));
      },
      quit: () => app.quit(),
    },
  });

  // The one WebSocket endpoint (web/CONTRACT.md §15.1): bound here, before the window, so the
  // renderer receives ws://127.0.0.1:<port>/cpd-ws as a plain preload value, not over an IPC
  // data channel. `services` and `started` stay module-scoped so reloadServer can swap them.
  const boundaries = realBoundaries(env);
  services = createServices(boundaries);
  started = new Promise<void>((resolve, reject) => {
    markStarted = resolve;
    failStarted = reject;
  });
  started.catch(() => undefined);
  let signalListening: (port: number) => void = () => {};
  const listening = new Promise<number>((resolve) => {
    signalListening = resolve;
  });
  const endpoint = createWsServer({
    newConnection: (send) => services.newConnection(send),
    server: createListenerFactory(),
    scheduler: boundaries.scheduler,
    started,
    onError: (error) => console.error(error),
    host: "127.0.0.1",
    port: 0,
    path: WS_PATH,
    onListening: (port) => signalListening(port),
  });
  disposers.push(() => endpoint.dispose());
  disposers.push(() => {
    void Promise.resolve(services.cockpit.stop()).catch((error: unknown) => console.error(error));
  });
  webSocketUrl = `ws://127.0.0.1:${await listening}${WS_PATH}`;

  };

  // Built once, on the first switch to `webui-preview`; the server stays up across later switches.
  const ensurePreviewClient = async (): Promise<PreviewClient> => {
    if (previewClient) return previewClient;
    const root = await readCheckoutRoot(env);
    if (!root) throw new Error("no checkout root (install cpd desktop or set CPD_CHECKOUT)");
    const worktree = env["CPD_WEB_PREVIEW_WORKTREE"] || path.resolve(root, "..", "cpd.preview.web");
    const devUrl = env["CPD_ELECTRON_DEV_URL"] || DEFAULT_DEV_URL;
    const markerFile = path.join(cpdHome, "run", "ui-view.json");
    const healthFile = path.join(cpdHome, "run", "preview-health.json");
    const readHealth = async (): Promise<HealthRecord | null> => {
      try {
        return parseHealth(await fs.readFile(healthFile, "utf8"));
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
        throw error;
      }
    };
    const writeHealth = async (record: HealthRecord): Promise<void> => {
      await fs.mkdir(path.dirname(healthFile), { recursive: true });
      const tmp = `${healthFile}.${process.pid}.tmp`;
      await fs.writeFile(tmp, JSON.stringify(record));
      await fs.rename(tmp, healthFile);
    };
    const showErrorBox = (title: string, content: string): void => electronDialog.showErrorBox(title, content);
    const setMode = (mode: UiMode): Promise<void> => controller.setMode(mode);
    const watchdog = createPreviewWatchdog({
      now: () => Date.now(),
      readHealth,
      pidAlive: (pid) => {
        try {
          process.kill(pid, 0);
          return true;
        } catch (error) {
          return (error as NodeJS.ErrnoException).code === "EPERM";
        }
      },
      setMode,
      showErrorBox,
      log: async (entry) => console.log(`preview-watchdog ${JSON.stringify(entry)}`),
    });
    const client = createPreviewClient({
      root,
      worktree,
      devUrl,
      pid: process.pid,
      now: () => Date.now(),
      env,
      markerFile,
      run: previewRun,
      readHealth,
      writeHealth,
      showLoading: () => {
        void win.loadURL(loadingPageUrl());
      },
      loadDevUrl: (url) => win.loadURL(url),
      showErrorBox,
      setMode,
      previousMode: () => previewPrevious,
      removeMarker: async () => {
        await fs.rm(markerFile, { force: true });
      },
      watchdog,
    });
    previewClient = client;
    previewTimer = setInterval(() => {
      void client.tick().catch((error: unknown) => console.error(error));
    }, 500);
    return client;
  };

  const initMode = async (_window: unknown): Promise<void> => {
  controller = createModeController({
    fs: {
      writeFile: (file, data) => fs.writeFile(file, data),
      remove: (file) => fs.rm(file),
      mkdir: async (dir) => {
        await fs.mkdir(dir, { recursive: true });
      },
    },
    legacyFile: path.join(cpdHome, "run", "ui-mode.json"),
    viewFile: path.join(cpdHome, "run", "ui-view.json"),
    pid: process.pid,
    log: (line) => console.log(line),
    menu: {
      setApplicationMenu: (template: MenuTemplate) =>
        Menu.setApplicationMenu(Menu.buildFromTemplate(template)),
    },
    window: {
      setIgnoreMenuShortcuts: (ignore) => {
        if (!win.webContents.isDestroyed()) win.webContents.setIgnoreMenuShortcuts(ignore);
      },
      sendMode: (mode) => send(CHANNELS.uiMode, mode),
    },
    platform: process.platform,
    onError: (error) => console.error(error),
  });
  controllerRef = controller;
  await controller.init();
  // The View menu calls controller.setMode; wrap it so switching to `webui-preview` drives the client
  // (and switching away only leaves the server up). mode.ts owns the menu; this is the wiring seam.
  const realSetMode = controller.setMode.bind(controller);
  controller.setMode = async (mode: UiMode): Promise<void> => {
    const before = controller.getMode();
    await realSetMode(mode);
    if (before === mode) return;
    if (mode === "webui-preview") {
      previewPrevious = before;
      try {
        const client = await ensurePreviewClient();
        await client.open();
      } catch (error) {
        electronDialog.showErrorBox("WebUI Preview", (error as Error).message);
        await controller.setMode(before);
      }
    } else if (before === "webui-preview") {
      await previewClient?.leave();
    }
  };
  };


  class CpdWindow extends BrowserWindow {
    constructor(options: Electron.BrowserWindowConstructorOptions & { title: string }) {
      super({
        ...options,
        icon: assets.icon,
        width: 1200,
        height: 800,
        backgroundColor: "#1c1a20",
        webPreferences: {
          preload: path.join(here, "preload.cjs"),
          contextIsolation: true,
          sandbox: true,
          nodeIntegration: false,
          additionalArguments: [`--cpd-ws-url=${webSocketUrl}`],
        },
      });
      win = this;
    }
  }

  const registerIpcNow = (): void => {
    const sinks = {
      data: (chunk: string) => send(CHANNELS.ptyData, chunk),
      exit: (exitCode: number) => send(CHANNELS.ptyExit, exitCode),
    };
    const tabs = createPtyTabs({
      spawn: (file, args, options) => backend.spawn(file, args, options),
      commandFor: (tabId) => tabCommand(tabId, sshHosts),
      cwd: os.homedir(),
      env,
      data: (tabId, chunk) => send(CHANNELS.ptyData, { tabId, data: chunk }),
      exit: (tabId, code) => send(CHANNELS.ptyExit, { tabId, code }),
      status: (tabId, tabStatus, code) =>
        send(CHANNELS.ptyStatus, code === undefined ? { tabId, status: tabStatus } : { tabId, status: tabStatus, code }),
    });
    ipcMain.handle(CHANNELS.ptyHosts, () => sshHosts);
    const wasmPath = createRequire(import.meta.url).resolve("@aurigax-ai/ghostty-web/ghostty-vt.wasm");
    const ipc = registerIpc({
      ipc: ipcMain,
      startPty: (size, ptySinks) =>
        startPty({ backend, size, cwd: os.homedir(), env, sinks: ptySinks }),
      sinks,
      readWasm: async () => new Uint8Array(await fs.readFile(wasmPath)),
      getMode: controller.getMode,
      timers: nodeTimers,
      tabs,
    });
    disposers.push(() => ipc.dispose());
    disposers.push(() => ipcMain.removeHandler(CHANNELS.ptyHosts));
    // The window closed (air-n62): stop every pty behind these sinks before anything can emit.
    // ipc.dispose() kills the singleton session and disposes the tab registry, dropping every
    // node-pty data listener, so no stream keeps calling send for a dead window.
    win.on("closed", () => ipc.dispose());
  };

  // The endpoint and services are bound in `prepare` (before the window, for the preload URL);
  // this stays the boot hook boot.ts calls.
  const registerWebuiIpcNow = (): void => {};

  const startServices = (): void => {
    services.cockpit.start().then(markStarted, (error: unknown) => {
      console.error(error);
      failStarted(error);
    });
    const attention = started.then(() => watchAttention(services.cockpit, nativeShell.setAttention));
    attention.catch(() => undefined);
    disposers.push(() => {
      void attention.then((watch) => watch.dispose()).catch(() => undefined);
    });
  };

  // Hot reload: re-import server.js with a fresh query, swap in new services, stop the old ones.
  const reloadServer = async (): Promise<void> => {
    const fresh = (await import(/* @vite-ignore */ `${new URL("server.js", import.meta.url).href}?t=${Date.now()}`)) as {
      createServices: typeof createServices;
      realBoundaries: typeof realBoundaries;
    };
    const old = services;
    services = fresh.createServices(fresh.realBoundaries(env));
    await Promise.resolve(old.cockpit.stop());
    startServices();
  };

  let stopUpdate: () => void = () => {};
  let quitting = false;
  app.on("before-quit", (event) => {
    stopUpdate();
    controllerRef?.dispose().catch((error: unknown) => console.error(error));
    for (const dispose of disposers) dispose();
    nativeShell.dispose();
    if (previewTimer) {
      clearInterval(previewTimer);
      previewTimer = undefined;
    }
    if (previewClient && !quitting) {
      // Graceful quit: `unserve` once and drop the run marker, then let the quit proceed.
      quitting = true;
      event.preventDefault();
      const client = previewClient;
      previewClient = undefined;
      const done = Promise.race([
        client.close().catch((error: unknown) => console.error(error)),
        new Promise<void>((resolve) => setTimeout(resolve, 10_000)),
      ]);
      void done.finally(() => app.quit());
    }
  });
  app.on("window-all-closed", () => app.quit());

  const updateLog = (): string => path.join(cpdHome, "run", "electron-update.log");
  const log = (line: string): void => {
    console.log(line);
    fs.mkdir(path.dirname(updateLog()), { recursive: true })
      .then(() => fs.appendFile(updateLog(), `${new Date().toISOString()} ${line}\n`))
      .catch((error: unknown) => console.error(error));
  };

  const startUpdate = async (window: { webContents: unknown }) => {
    const w = window as BrowserWindow;
    const recorder = createHealthRecorder({
      file: path.join(cpdHome, "run", "preview-health.json"),
      crashLog: path.join(cpdHome, "run", "preview-crash.log"),
      pid: process.pid,
      now: () => new Date(),
      fs: {
        readFile: (f) => fs.readFile(f, "utf8"),
        writeFile: (f, data) => fs.writeFile(f, data),
        rename: (a, b) => fs.rename(a, b),
        appendFile: (f, data) => fs.appendFile(f, data),
        mkdir: async (dir) => {
          await fs.mkdir(dir, { recursive: true });
        },
      },
    });
    await recorder.init();
    attachHealth({ recorder, ipcMain, process, app, webContents: w.webContents });
    const openExternal = (url: string): unknown => electronShell.openExternal(url);
    w.webContents.setWindowOpenHandler(createExternalLinkHandler(openExternal));
    w.webContents.on("will-navigate", (event, url) => {
      if (url === w.webContents.getURL()) return;
      event.preventDefault();
      void openExternal(url);
    });
    let latestBootstrap: BootstrapStatus | undefined;
    let latestState: UpdateState | undefined;
    w.webContents.on("did-finish-load", () => {
      if (latestBootstrap) send(UPDATE_BOOTSTRAP_CHANNEL, latestBootstrap);
      if (latestState) send(UPDATE_STATE_CHANNEL, latestState);
    });
    const update = await startAutoUpdate({
      env: process.env,
      bundleDir: here,
      home: os.homedir(),
      platform: process.platform,
      exec: nodeExec,
      fs: nodeFs,
      timers: nodeTimers,
      spawnFollow: nodeSpawnFollow,
      isFocused: () => w.isFocused(),
      systemIdleSeconds: () => powerMonitor.getSystemIdleTime(),
      reloadRenderer: () => w.webContents.reloadIgnoringCache(),
      onState: (state) => {
        latestState = state;
        send(UPDATE_STATE_CHANNEL, state);
        nativeShell.setUpdateState(state, () => {
          app.relaunch();
          app.exit(0);
        });
      },
      onBootstrap: (status) => {
        latestBootstrap = status;
        send(UPDATE_BOOTSTRAP_CHANNEL, status);
      },
      log,
    });
    stopUpdate = update.stop;
    return update;
  };

  await boot({
    app: { setName: (name) => app.setName(name), whenReady: () => app.whenReady().then(prepare) },
    initMode,
    BrowserWindow: CpdWindow,
    powerMonitor,
    process,
    startAutoUpdate: startUpdate,
    startServices,
    reloadServer,
    registerIpc: registerIpcNow,
    registerWebuiIpc: registerWebuiIpcNow,
    loadDevUrl: (w, url) => (w as BrowserWindow).loadURL(url),
    loadFile: (w) => (w as BrowserWindow).loadFile(path.join(here, "renderer", "electron.html")),
    env: process.env,
  });
}

main().catch((error: unknown) => {
  console.error(error);
  app.exit(1);
});
