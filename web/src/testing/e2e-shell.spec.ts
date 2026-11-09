// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: cpd desktop launches the installed app (air-d89 point 1) and a closed window with a
// live pty never raises an uncaught exception (air-n62).
//
// air-d89 point 1: `cpd desktop` launches the packaged app installed in the app dir
// (`<home>/.local/share/cpd/app/CPD.app/Contents/MacOS/CPD` on macOS, core/src/app/mod.rs
// `launch`), not the checkout's `node_modules` Electron. The installed bundle is a shell that
// boots a hidden, isolated window.
//
// air-n62: `win.webContents` on a destroyed window throws `Object has been destroyed`. The send
// path (main.ts `sendToWindow`, fixed in main by #963) rejects a destroyed window before reading
// `webContents`; this spec pins that a pty that is still emitting while the window is closed never
// reaches an uncaught exception in the main process.
//
// The installed app dir is staged: a directory whose `package.json` points at the checkout's built
// main, so Electron's app path IS the staged installed dir (`app.getAppPath()`), and the app boots
// its real main. The fake `cpd` is the house pattern (stageFakeBin); its `tui` (the pty command)
// emits continuously, so the window is closed while chunks are in flight. Hidden and isolated
// (air-d68): a per-run `userData` dir and `CPD_HOME`, and the window is created with `show:false`.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchApp, firstWindow, stageFakeBin, test, expect } from "./e2e-fixtures";

const webRoot = fileURLToPath(new URL("../..", import.meta.url));
const sharedFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

let staging = "";
/** The staged installed app dir; the app is launched with this as its Electron app path. */
let installedAppDir = "";
/** The fake pty writes its pid here, so the spec can prove it was live at close and dead after. */
let ptyPidFile = "";

/**
 * The fake pty command (`cpd tui --attach`): record the pid, then emit a chunk every 2 ms forever.
 * It is the "live pty" the window is closed under, so a chunk is in flight at close time.
 */
const PTY_EMITTER = `#!/usr/bin/env node
"use strict";
const fs = require("node:fs");
const file = process.env.CPD_E2E_PTY_PID_FILE;
if (file) fs.writeFileSync(file, String(process.pid));
setInterval(() => { process.stdout.write("live pty tick\\r\\n"); }, 2);
`;

/** The fake `cpd`: the pty command is the emitter; every other command is the shared fake. */
const FAKE_CPD = `#!/bin/sh
if [ "$1" = "tui" ]; then
  exec "${process.execPath}" "$STAGING/pty-emitter.cjs"
fi
exec "${sharedFakeCpd}" "$@"
`;

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-shell-"));
  installedAppDir = join(staging, "installed-app");
  ptyPidFile = join(staging, "pty.pid");
  mkdirSync(installedAppDir, { recursive: true });

  // Build the bundle the installed app dir points at (the checkout's built main); the app path must
  // resolve to a real main file. Same self-heal as the other specs.
  const builtMain = join(webRoot, "dist-electron", "main.js");
  if (!existsSync(builtMain)) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }

  // The staged installed app dir: an Electron app whose `main` is the checkout's built main, so
  // Electron's app path is the staged dir. `build/` (icon, tray) is linked so the native shell
  // resolves its assets relative to the staged app path, exactly as unpackaged Electron does.
  writeFileSync(
    join(installedAppDir, "package.json"),
    JSON.stringify({ name: "cpd-installed-e2e", version: "0.0.0", main: builtMain }),
  );
  symlinkSync(join(webRoot, "build"), join(installedAppDir, "build"), "dir");

  // The house fake-cpd-on-PATH setup, with the pty-emitting fake cpd.
  const fakeCpd = join(staging, "cpd");
  writeFileSync(fakeCpd, FAKE_CPD.replaceAll("$STAGING", staging));
  chmodSync(fakeCpd, 0o755);
  writeFileSync(join(staging, "pty-emitter.cjs"), PTY_EMITTER);
  stageFakeBin(staging, fakeCpd);
  process.env["CPD_E2E_PTY_PID_FILE"] = ptyPidFile;
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

test("the installed app boots and a closed window with a live pty does not throw (air-d89 p1, air-n62)", async ({ cpdHome, userDataDir, rendererServer }) => {
  const app = await launchApp({
    cpdHome,
    userDataDir,
    rendererUrl: rendererServer.url,
    appPath: installedAppDir,
    env: { CPD_CHECKOUT: webRoot },
  });
  try {
    // The app launched from the staged installed dir (air-d89 point 1). `realpathSync` collapses
    // the macOS /var -> /private/var symlink on both sides.
    const appPath = await app.evaluate(({ app: electronApp }) => electronApp.getAppPath());
    expect(realpathSync(appPath)).toBe(realpathSync(installedAppDir));

    const page = await firstWindow(app);
    await page.waitForLoadState("domcontentloaded");

    // Hidden and isolated (air-d68): no window is visible, and the app's own userData is the
    // per-run temp dir, never the User's profile or lock.
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().every((w) => !w.isVisible()))).toBe(true);
    expect(await app.evaluate(({ app: electronApp }) => electronApp.getPath("userData"))).toContain("cpd-e2e-userdata-");

    // Record any main-process uncaught exception (the air-n62 throw would land here), and keep the
    // app alive after the window closes so the spec can read the record.
    await app.evaluate(({ app: electronApp }) => {
      const g = globalThis as Record<string, unknown>;
      g["__cpdUncaught"] = [] as string[];
      process.on("uncaughtException", (error: unknown) => {
        (g["__cpdUncaught"] as string[]).push(String((error as Error)?.stack ?? error));
      });
      electronApp.on("before-quit", (event: { preventDefault(): void }) => event.preventDefault());
    });

    // Start the local pty (the fake cpd's `tui`): it emits continuously. Count chunks in the
    // renderer to prove the pty is live before the window is closed.
    await page.evaluate(() => {
      const g = window as unknown as Record<string, unknown>;
      g["__cpdPtyChunks"] = 0;
      const bridge = (window as unknown as { cpdElectron: { onPtyData(cb: () => void): void; startPty(size: { cols: number; rows: number }): Promise<void> } }).cpdElectron;
      bridge.onPtyData(() => {
        g["__cpdPtyChunks"] = (g["__cpdPtyChunks"] as number) + 1;
      });
      return bridge.startPty({ cols: 80, rows: 24 });
    });
    await expect.poll(() => page.evaluate(() => (window as unknown as Record<string, number>)["__cpdPtyChunks"]), { timeout: 30_000 }).toBeGreaterThan(0);
    await expect.poll(() => existsSync(ptyPidFile), { timeout: 30_000 }).toBe(true);
    const ptyPid = Number(readFileSync(ptyPidFile, "utf8"));

    // Close the window while the pty is still emitting. The closed handler disposes the pty; any
    // chunk already in flight hits the send path for a destroyed window.
    await page.close();

    // The pty is killed with the window (no orphan), giving any in-flight chunk time to arrive.
    await expect.poll(
      () => {
        try {
          process.kill(ptyPid, 0);
          return false;
        } catch {
          return true;
        }
      },
      { timeout: 30_000 },
    ).toBe(true);

    // No uncaught exception reached the main process (air-n62), and no window is left.
    expect(await app.evaluate(() => (globalThis as Record<string, unknown>)["__cpdUncaught"])).toEqual([]);
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(0);
  } finally {
    // The before-quit preventer kept the app alive; exit it so the fixture teardown finds it gone.
    await app.evaluate(({ app: electronApp }) => electronApp.exit(0)).catch(() => undefined);
    await app.close().catch(() => undefined);
  }
});
