import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import { createRequire } from "node:module";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, ipcMain, Menu, nativeImage, nativeTheme, powerMonitor, shell as electronShell, Tray } from "electron";
import { boot } from "./boot.ts";
import * as nodePty from "node-pty";
import { createServices, realBoundaries } from "../server/plugin.ts";
import { watchAttention } from "./attention.ts";
import { CHANNELS, UPDATE_BOOTSTRAP_CHANNEL, UPDATE_STATE_CHANNEL } from "./channels.ts";
import { registerWebuiIpc } from "./cockpit-ipc.ts";
import { resolveLoginPath } from "./env.ts";
import { createHealthRecorder } from "./health.ts";
import { attachHealth } from "./health-main.ts";
import { registerIpc } from "./ipc.ts";
import { createModeController, modeFilePath } from "./mode.ts";
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

const backend: PtyBackend = {
  spawn: (file, args, options) => nodePty.spawn(file, args, options),
};

async function main(): Promise<void> {
  let win!: BrowserWindow;
  const send = (channel: string, payload: unknown): void => {
    if (win && !win.webContents.isDestroyed()) win.webContents.send(channel, payload);
  };
  let env: NodeJS.ProcessEnv = process.env;
  let cpdHome = "";
  let assets!: ReturnType<typeof resolveAssetPaths>;
  let nativeShell!: ReturnType<typeof setupNativeShell>;
  let controller!: ReturnType<typeof createModeController>;
  // Assigned by initMode; the native shell is set up before the window.
  let controllerRef: ReturnType<typeof createModeController> | undefined;
  const disposers: (() => void)[] = [];
  let services!: ReturnType<typeof createServices>;
  let started!: Promise<void>;
  let markStarted: () => void = () => {};
  let failStarted: (error: unknown) => void = () => {};

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

  };

  const initMode = async (_window: unknown): Promise<void> => {
  controller = createModeController({
    fs: {
      readFile: (file) => fs.readFile(file, "utf8"),
      writeFile: (file, data) => fs.writeFile(file, data),
      mkdir: async (dir) => {
        await fs.mkdir(dir, { recursive: true });
      },
    },
    file: modeFilePath(cpdHome),
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
    const wasmPath = createRequire(import.meta.url).resolve("@aurigax-ai/ghostty-web/ghostty-vt.wasm");
    const ipc = registerIpc({
      ipc: ipcMain,
      startPty: (size, ptySinks) =>
        startPty({ backend, size, cwd: os.homedir(), env, sinks: ptySinks }),
      sinks,
      readWasm: async () => new Uint8Array(await fs.readFile(wasmPath)),
      getMode: controller.getMode,
      timers: nodeTimers,
    });
    disposers.push(() => ipc.dispose());
  };

  const registerWebuiIpcNow = (): void => {
    services = createServices(realBoundaries(env));
    started = new Promise<void>((resolve, reject) => {
      markStarted = resolve;
      failStarted = reject;
    });
    started.catch(() => undefined);
    const webuiIpc = registerWebuiIpc({
      ipc: ipcMain,
      newConnection: (...args) => services.newConnection(...args),
      started,
      onError: (error) => console.error(error),
    });
    disposers.push(() => {
      void webuiIpc.dispose();
      void Promise.resolve(services.cockpit.stop()).catch((error: unknown) => console.error(error));
    });
  };

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
  app.on("before-quit", () => {
    stopUpdate();
    for (const dispose of disposers) dispose();
    nativeShell.dispose();
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
