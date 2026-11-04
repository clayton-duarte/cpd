import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import { createRequire } from "node:module";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, ipcMain, Menu } from "electron";
import * as nodePty from "node-pty";
import { CHANNELS } from "./channels.ts";
import { resolveLoginPath } from "./env.ts";
import { registerIpc } from "./ipc.ts";
import { createModeController, modeFilePath } from "./mode.ts";
import type { MenuTemplate } from "./menu.ts";
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
  await app.whenReady();
  const loginPath = await resolveLoginPath({
    platform: process.platform,
    shell: process.env["SHELL"],
    currentPath: process.env["PATH"],
    run,
  });
  const env = { ...process.env, PATH: loginPath };
  const cpdHome = process.env["CPD_HOME"] ?? path.join(os.homedir(), ".cpd");

  const win = new BrowserWindow({
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
  const send = (channel: string, payload: unknown): void => {
    if (!win.webContents.isDestroyed()) win.webContents.send(channel, payload);
  };

  const controller = createModeController({
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
  await controller.init();

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
  });
  app.on("before-quit", () => ipc.dispose());
  app.on("window-all-closed", () => app.quit());

  const devUrl = process.env["CPD_ELECTRON_DEV_URL"];
  if (devUrl) await win.loadURL(devUrl);
  else await win.loadFile(path.join(here, "renderer", "electron.html"));
}

main().catch((error: unknown) => {
  console.error(error);
  app.exit(1);
});
