// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the health watchdog walks the revert ladder preview > prod > HEAD-1 by the view that is
// actually loaded (guards air-d88 the watchdog detects breaks in every view mode and walks the
// ladder, air-d95 the rollback relaunch carries production mode, air-d81 every detection and
// recovery is a structured Event).
//
// The app boots in preview mode against a suite-owned renderer URL (air-d68: a per-run `userData`
// dir and `CPD_HOME`, a hidden window, no real cpd, no tmux, no network). Each rung of the ladder is
// a build on disk the watchdog can swap in: `web/dist-electron` (prod), `web/dist-electron.head-1`
// and `web/dist-electron.head-2`; the preview view is the dev URL the suite serves. Every rung's
// renderer page names its own build, visibly in `#built-view`, in the window title, and in the
// `<title>` it sets as soon as its own script runs.
//
// Asserting "the view that is loaded" after a relaunch needs a handle Playwright does not have: the
// rollback relaunches the app, so the first `ElectronApplication` is gone. The spec therefore reads
// Chromium's own target list over the DevTools HTTP endpoint `http://127.0.0.1:<port>/json` — the
// app is launched (and so relaunched) with `--remote-debugging-port=<reserved port>`. That list is
// the truth about what Electron loaded: the page's `url` (the file URL of the build the app loaded,
// or the suite's dev URL) and its `title` (set by the page's own script), so a build is identified
// by the view that ran, never by an Event alone.
//
// The break each rung gets is the renderer's own cockpit/transport report
// (`$CPD_HOME/run/cockpit-health.json`, web/CONTRACT.md §15.2), the signal the watchdog reads in
// every view mode (web/src/electron/main.ts `startHealthWatchdog`):
//   - preview: `empty-data` — the view painted but never received a cockpit snapshot, so it never
//     becomes healthy;
//   - prod: `websocket-stalled` — the loaded build's transport never delivers a frame;
//   - HEAD-1: present and healthy — a real snapshot and a live heartbeat, so the walk stops.
// The report is kept live the way the renderer keeps it live (its heartbeat is the transport's
// pulse), and the spec turns it healthy the moment the watchdog's detection Event appears, so the
// relaunched process — a fresh watchdog with no memory of the epoch — finds the rung above it
// healthy instead of reverting on the break of the rung it just replaced.
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createServer, request as httpRequest } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, firstWindow, launchApp, stageFakeBin, test, webRoot } from "./e2e-fixtures";

const sharedFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

/** The fake `git` reports this for HEAD and origin/main, so the app's update pass is a no-op. */
const HEAD_SHA = "e2e-watchdog-head-sha";
/** The build marker each rung's page carries, and the title it sets when its script runs. */
const RUNG = {
  preview: "preview-build",
  prod: "prod-build",
  "head-1": "head-1-build",
  "head-2": "head-2-build",
} as const;

/** The title prefix a rung's page writes, so the target list names the build the view belongs to. */
const TITLE_PREFIX = "cpd-e2e-view:";

/** The fake CLI/scripts: nothing to build, nothing to install. */
const FAKE_OK = `#!/bin/sh
exit 0
`;

/** A fake `git`: every query answers HEAD_SHA, so `syncCheckout` sees a current checkout. */
const FAKE_GIT = `#!/bin/sh
printf '%s\\n' '${HEAD_SHA}'
exit 0
`;

/** The fake `pnpm`: the suite stages every build, so no build command may run. */
const FAKE_PNPM = `#!/bin/sh
exit 0
`;

/**
 * The entry Electron starts: the real main process, with the native error dialog neutralised. The
 * watchdog announces every detection and recovery with `dialog.showErrorBox` (main.ts
 * `startHealthWatchdog`), a modal that blocks the main process until it is dismissed and, on macOS,
 * a window on the User's screen (air-d68). Playwright cannot reach a relaunched main process to
 * patch it (e2e-view.spec.ts patches the process Playwright owns), so the staged app entry patches
 * it in every process of the walk, before the real main is imported. Only the dialog is stubbed;
 * the watchdog, the ladder and the Events are the product's own.
 */
const SPEC_BOOT = `"use strict";
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { dialog } = require("electron");
dialog.showErrorBox = () => {};
import(pathToFileURL(path.join(__dirname, "dist-electron", "main.js")).href).catch((error) => {
  console.error(error);
  require("electron").app.exit(1);
});
`;

let staging = "";
/** The staged checkout the app resolves (`CPD_CHECKOUT`); the ladder lives under its `web/`. */
let checkout = "";
/** The staged app dir the app is launched from: `<checkout>/web` (its `dist-electron` is prod). */
let appDir = "";
/** A staged `HOME`, so the CLI bootstrap finds an installed CLI and never reads the User's home. */
let stagedHome = "";
/** The port the app's DevTools HTTP endpoint answers on, in the first process and every relaunch. */
let debugPort = 0;

/** A rung's renderer page: it names its build visibly and in the title, once its script runs. */
function viewPage(build: string): string {
  return (
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>CPD</title></head><body>` +
    `<div id="webui-root"><div id="built-view">${build}</div></div>` +
    `<script>document.title = ${JSON.stringify(TITLE_PREFIX + build)};</script>` +
    `</body></html>`
  );
}

/** A port no one is using, reserved for the app's DevTools endpoint (freed for Electron to bind). */
async function reservePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((listening, failed) => {
    server.once("error", failed);
    server.listen(0, "127.0.0.1", listening);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no port to reserve");
  await new Promise<void>((closed) => server.close(() => closed()));
  return address.port;
}

/** The renderer URL the app loads in preview mode, served by the suite on an ephemeral port. */
async function startPreviewServer(): Promise<{ url: string; close(): Promise<void> }> {
  const page = viewPage(RUNG.preview);
  const server = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" }).end(page);
  });
  await new Promise<void>((listening, failed) => {
    server.once("error", failed);
    server.listen(0, "127.0.0.1", listening);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("preview server has no port");
  return {
    url: `http://127.0.0.1:${address.port}/electron.html`,
    close: () => new Promise<void>((closed) => server.close(() => closed())),
  };
}

/** One renderer target Chromium reports: the page's URL and the title its own script set. */
type ViewTarget = { url: string; title: string };

/** Chromium's target list, or `null` while no process owns the port (between every relaunch). */
async function readTargets(port: number): Promise<{ type?: string; url?: string; title?: string }[] | null> {
  return new Promise((resolve) => {
    const request = httpRequest({ host: "127.0.0.1", port, path: "/json", method: "GET", timeout: 2_000 }, (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk: string) => {
        body += chunk;
      });
      response.on("end", () => {
        try {
          resolve(JSON.parse(body));
        } catch {
          resolve(null);
        }
      });
    });
    request.on("timeout", () => request.destroy());
    request.on("error", () => resolve(null));
    request.end();
  });
}

/** The page target the app's window is showing, or `null` while it has none. */
async function currentView(): Promise<ViewTarget | null> {
  const targets = await readTargets(debugPort);
  const page = targets?.find((target) => target.type === "page" && typeof target.url === "string");
  if (page === undefined) return null;
  return { url: page.url ?? "", title: page.title ?? "" };
}

/** Stage one rung: the real main process, its server, and a renderer page that names its build. */
function stageRung(dir: string, build: string): void {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "renderer"), { recursive: true });
  for (const file of ["main.js", "preload.cjs", "server.js"]) {
    cpSync(join(webRoot, "dist-electron", file), join(dir, file));
  }
  writeFileSync(join(dir, "renderer", "electron.html"), viewPage(build));
}

/** Reset every rung to its pre-test state: the ladder reverts rewrite these dirs. */
function resetRungs(): void {
  stageRung(join(appDir, "dist-electron"), RUNG.prod);
  stageRung(join(appDir, "dist-electron.head-1"), RUNG["head-1"]);
  stageRung(join(appDir, "dist-electron.head-2"), RUNG["head-2"]);
  // The prod build carries HEAD's sha, so the app's own update pass builds and installs nothing.
  writeFileSync(join(appDir, "dist-electron", ".built-sha"), HEAD_SHA);
  for (const leftover of ["dist-electron.revert", "dist-electron.displaced", "dist-electron.preview"]) {
    rmSync(join(appDir, leftover), { recursive: true, force: true });
  }
}

test.beforeAll(async () => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-watchdog-"));
  checkout = join(staging, "checkout");
  appDir = join(checkout, "web");
  stagedHome = join(staging, "home");
  debugPort = await reservePort();

  mkdirSync(join(checkout, ".git"), { recursive: true });
  mkdirSync(join(checkout, "target"), { recursive: true });
  mkdirSync(join(checkout, "scripts"), { recursive: true });
  mkdirSync(join(appDir, "dist-electron"), { recursive: true });
  mkdirSync(join(stagedHome, ".local", "bin"), { recursive: true });

  // The app resolves its checkout by these two paths (update/checkout.ts `resolveCheckout`).
  writeFileSync(join(checkout, "scripts", "install-cpd.sh"), FAKE_OK);
  chmodSync(join(checkout, "scripts", "install-cpd.sh"), 0o755);
  writeFileSync(join(checkout, "scripts", "cargo.sh"), FAKE_OK);
  chmodSync(join(checkout, "scripts", "cargo.sh"), 0o755);
  // An already-installed CLI, so `ensureCli` is `ready` and the update pass runs as a no-op.
  writeFileSync(join(stagedHome, ".local", "bin", "cpd"), FAKE_OK);
  chmodSync(join(stagedHome, ".local", "bin", "cpd"), 0o755);
  writeFileSync(join(checkout, "target", ".cli-installed-sha"), HEAD_SHA);

  // The staged app dir: an Electron app whose `main` is the built main process, launched as the
  // app path so `app.getAppPath()` (and therefore every `here`-relative load) is this dir's `web/`.
  // `spec-boot.cjs` stays beside `dist-electron`, so a rollback that swaps the build dir never
  // removes it, and every rung loads through it.
  writeFileSync(
    join(appDir, "package.json"),
    JSON.stringify({ name: "cpd-watchdog-e2e", version: "0.0.0", private: true, type: "module", main: "spec-boot.cjs" }),
  );
  writeFileSync(join(appDir, "spec-boot.cjs"), SPEC_BOOT);
  symlinkSync(join(webRoot, "node_modules"), join(appDir, "node_modules"), "dir");
  symlinkSync(join(webRoot, "build"), join(appDir, "build"), "dir");

  // The house fake-cpd-on-PATH setup, then the fake `git` and `pnpm` the update pass shells out to.
  const { bin } = stageFakeBin(staging, sharedFakeCpd);
  writeFileSync(join(bin, "git"), FAKE_GIT);
  chmodSync(join(bin, "git"), 0o755);
  writeFileSync(join(bin, "pnpm"), FAKE_PNPM);
  chmodSync(join(bin, "pnpm"), 0o755);

  if (!existsSync(join(webRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

/** The renderer's cockpit/transport report, the file the watchdog reads in every view mode. */
function healthFile(cpdHome: string): string {
  return join(cpdHome, "run", "cockpit-health.json");
}

/** The app's own structured Event sink (air-d81): one JSON line per detection and per recovery. */
function eventsFile(cpdHome: string): string {
  return join(cpdHome, "run", "watchdog-events.jsonl");
}

/** The run-only view marker, written on every boot (web/src/electron/mode.ts). */
function markerFile(cpdHome: string): string {
  return join(cpdHome, "run", "ui-view.json");
}

/** Write a report the way the renderer does: a temp file renamed over the real one (no torn read). */
function writeHealth(cpdHome: string, report: Record<string, unknown>): void {
  const file = healthFile(cpdHome);
  mkdirSync(join(cpdHome, "run"), { recursive: true });
  const tmp = `${file}.e2e.tmp`;
  writeFileSync(tmp, JSON.stringify(report));
  renameSync(tmp, file);
}

/** The rung's report as its renderer would write it: `null` is a healthy rung. */
type ReportState = { epoch: number; breakKind: "empty-data" | "websocket-stalled" | null };

/**
 * A report in one of the shapes `detectBreak` reads (web/src/electron/health.ts). Healthy: a cockpit
 * snapshot arrived and the server heartbeat is live. `empty-data`: the view painted but never
 * received a cockpit snapshot 20 s after boot. `websocket-stalled`: the heartbeat stopped 20 s ago,
 * so no frame can arrive.
 */
function renderReport(state: ReportState): Record<string, unknown> {
  const now = Date.now();
  if (state.breakKind === null) {
    return { epoch: state.epoch, startedAt: now, tree: { contexts: [] }, snapshotAt: now, heartbeatAt: now };
  }
  return state.breakKind === "empty-data"
    ? { epoch: state.epoch, startedAt: now - 20_000, tree: null }
    : { epoch: state.epoch, startedAt: now, tree: null, heartbeatAt: now - 20_000 };
}

/**
 * The renderer's report, played by the spec. A report is not a fact written once: the renderer keeps
 * it live (the heartbeat is the transport's pulse, and a heartbeat that stops arriving is itself
 * `websocket-stalled`), so the report is rewritten on the watchdog's own cadence until the walk
 * moves to the next rung's epoch.
 */
let reportState: ReportState | null = null;
let reportHome = "";
let reportTimer: ReturnType<typeof setInterval> | undefined;

function startReports(cpdHome: string): void {
  reportHome = cpdHome;
  reportState = null;
  stopReports();
  reportTimer = setInterval(() => {
    if (reportState !== null) writeHealth(reportHome, renderReport(reportState));
  }, 500);
}

function stopReports(): void {
  if (reportTimer !== undefined) clearInterval(reportTimer);
  reportTimer = undefined;
  reportState = null;
}

/** Hand the app the rung's report now (and keep handing it on `startReports`' cadence). */
function setReport(epoch: number, breakKind: ReportState["breakKind"]): void {
  reportState = { epoch, breakKind };
  writeHealth(reportHome, renderReport(reportState));
}

/** The Events reported so far, as `kind:detection` for a break and `kind:build` for a recovery. */
function eventKeys(cpdHome: string): string[] {
  let text = "";
  try {
    text = readFileSync(eventsFile(cpdHome), "utf8");
  } catch {
    return [];
  }
  return text
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line))
    .map((event) =>
      event.kind === "watchdog.break"
        ? `watchdog.break:${event.data?.detection}`
        : `watchdog.recovery:${event.data?.build ?? event.data?.recovery}`,
    );
}

/** The main pid the app last wrote into its view marker, or `null` while it has written none. */
function markerPid(cpdHome: string): number | null {
  try {
    const marker = JSON.parse(readFileSync(markerFile(cpdHome), "utf8")) as { pid?: number };
    return typeof marker.pid === "number" && marker.pid > 0 ? marker.pid : null;
  } catch {
    return null;
  }
}

/** Whether a pid is still alive (EPERM counts as alive: it exists, we just may not signal it). */
function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

/** Every live process whose command line names `needle` (the app and the children it spawns). */
function processesMatching(needle: string): number[] {
  const out = execFileSync("ps", ["-axo", "pid=,command="], { encoding: "utf8", timeout: 20_000 });
  return out
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.includes(needle))
    .map((line) => Number(line.split(/\s+/)[0]))
    .filter((pid) => Number.isInteger(pid) && pid > 0 && pid !== process.pid);
}

/** Stop every process the test started: the app's main pids, then anything left under the staging. */
async function stopEverything(pids: number[]): Promise<void> {
  const targets = [...new Set([...pids, ...processesMatching(staging)])];
  for (const pid of targets) {
    if (pid === process.pid) continue;
    try {
      process.kill(pid, "SIGTERM");
    } catch {
      // already gone
    }
  }
  const stillAlive = () => [...new Set([...pids, ...processesMatching(staging)])].filter(alive);
  await expect
    .poll(stillAlive, { timeout: 20_000, intervals: [100] })
    .toEqual([])
    .catch(async () => {
      for (const pid of stillAlive()) {
        try {
          process.kill(pid, "SIGKILL");
        } catch {
          // already gone
        }
      }
    });
  // No orphan survives the test: nothing of the app, its pty or its cpd children is left running.
  await expect.poll(stillAlive, { timeout: 20_000, intervals: [100] }).toEqual([]);
}

/**
 * A bounded wait, polled rather than slept: the last rung must stay put over several watchdog ticks
 * (the watchdog ticks every 500 ms), so "no further revert" is a claim over time, not one instant.
 */
async function settle(ms: number): Promise<void> {
  const since = Date.now();
  await expect.poll(() => Date.now() - since, { timeout: ms + 10_000, intervals: [250] }).toBeGreaterThanOrEqual(ms);
}

/** The file URL a build's renderer loads from, through the macOS /var -> /private/var symlink. */
function rendererUrl(): string {
  return `file://${join(realpathSync(appDir), "dist-electron", "renderer", "electron.html")}`;
}

type WalkStep = {
  cpdHome: string;
  /** The main pid the walk is running in now. */
  fromPid: number;
  /** The break this rung really has, as the cockpit/transport report shows it. */
  breakKind: "empty-data" | "websocket-stalled";
  /** The renderer epoch of the break (one per rung; the healthy report takes the next one). */
  breakEpoch: number;
  /** The rung the ladder must step to: its build's page must be the view that loads. */
  nextRung: "prod" | "head-1" | "head-2";
};

/**
 * Break the rung the app is running now and let the watchdog walk one step down the ladder. The
 * detection Event proves the watchdog read the break; the report is turned healthy right after, so
 * the relaunched process finds the rung above it healthy instead of reverting again on it. Then the
 * walk is asserted twice over: the recovery Event names the rung taken, and the *view that is
 * loaded* is that rung's build — its page's own title and the file it was loaded from — so no rung
 * is proven by an Event alone.
 */
async function walkStep(step: WalkStep): Promise<number> {
  const { cpdHome, fromPid } = step;
  const before = eventKeys(cpdHome).length;
  setReport(step.breakEpoch, step.breakKind);

  // The detection Event: the watchdog read the break and is reverting now. The report is turned
  // healthy for the next rung the moment it appears, so the relaunched process — a fresh watchdog
  // with no memory of the epoch — finds the rung above it healthy instead of reverting again.
  await expect
    .poll(() => eventKeys(cpdHome).slice(before), { timeout: 60_000, intervals: [10, 20, 50] })
    .toEqual([`watchdog.break:${step.breakKind}`]);
  setReport(step.breakEpoch + 1, null);

  // The recovery Event names the rung the ladder stepped to (air-d81).
  await expect
    .poll(() => eventKeys(cpdHome).slice(before), { timeout: 60_000, intervals: [50] })
    .toEqual([`watchdog.break:${step.breakKind}`, `watchdog.recovery:${step.nextRung}`]);

  // A new main process came up (the rollback relaunch), and it is not the one we were watching.
  await expect
    .poll(() => {
      const pid = markerPid(cpdHome);
      return pid !== null && pid !== fromPid ? pid : null;
    }, { timeout: 60_000, intervals: [100] })
    .not.toBeNull();
  const pid = markerPid(cpdHome);
  if (pid === null) throw new Error("the relaunched app wrote no view marker");

  // The view that is loaded names the rung: the page Electron loaded sets its own title, and the
  // URL it was loaded from says the rollback relaunch carried production mode — the built renderer
  // from disk, never the preview dev URL (air-d95).
  await expect
    .poll(async () => (await currentView())?.title ?? null, { timeout: 60_000, intervals: [100] })
    .toBe(`${TITLE_PREFIX}${RUNG[step.nextRung]}`);
  const view = await currentView();
  expect(view?.url).toBe(rendererUrl());
  return pid;
}

/** Boot the app against the staged checkout and return it with the first window and the server. */
async function bootApp(cpdHome: string, userDataDir: string) {
  const preview = await startPreviewServer();
  resetRungs();
  const app = await launchApp({
    cpdHome,
    userDataDir,
    rendererUrl: preview.url,
    // The staged app dir is the app path, so the main process it boots resolves every build it
    // loads (`here`) and every rung of the ladder (`CPD_CHECKOUT/web`) under the staging.
    appPath: appDir,
    // The DevTools endpoint the spec reads the loaded view from; the relaunch repeats it.
    extraArgs: [`--remote-debugging-port=${debugPort}`],
    env: { CPD_CHECKOUT: checkout, HOME: stagedHome },
  });
  const page = await firstWindow(app);
  await page.waitForLoadState("domcontentloaded");
  startReports(cpdHome);
  return { app, page, preview };
}

test("the watchdog walks preview > prod > HEAD-1 by the loaded view (air-d88, air-d95, air-d81)", async ({ cpdHome, userDataDir }) => {
  const { app, page, preview } = await bootApp(cpdHome, userDataDir);
  const pids: number[] = [];
  try {
    // Hidden and isolated (air-d68): no window on the User's screen, every write under the per-run dirs.
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().every((w) => !w.isVisible()))).toBe(true);
    expect(await app.evaluate(({ app: electronApp }) => electronApp.getPath("userData"))).toContain("cpd-e2e-userdata-");

    // The rung the app boots on is preview: the view is the suite's dev URL, naming the preview build.
    await expect(page.locator("#built-view")).toHaveText(RUNG.preview, { timeout: 60_000 });
    await expect.poll(async () => (await currentView())?.title ?? null, { timeout: 30_000 }).toBe(`${TITLE_PREFIX}${RUNG.preview}`);
    expect((await currentView())?.url).toBe(preview.url);
    await expect.poll(() => markerPid(cpdHome), { timeout: 30_000 }).not.toBeNull();
    const launchPid = markerPid(cpdHome) as number;
    pids.push(launchPid);
    expect(app.windows().length).toBe(1);

    // Preview never becomes healthy (no cockpit snapshot): the ladder steps to prod, and the loaded
    // view is the built prod renderer from disk — not the preview dev URL (air-d95).
    const prodPid = await walkStep({ cpdHome, fromPid: launchPid, breakKind: "empty-data", breakEpoch: 1, nextRung: "prod" });
    pids.push(prodPid);

    // The prod build's transport never delivers a frame: the ladder steps to HEAD-1, whose build the
    // watchdog swapped in, and whose page is what Electron loaded.
    const head1Pid = await walkStep({ cpdHome, fromPid: prodPid, breakKind: "websocket-stalled", breakEpoch: 3, nextRung: "head-1" });
    pids.push(head1Pid);

    // HEAD-1 is present and healthy, so the walk stops there: over several watchdog ticks nothing is
    // detected, nothing is reverted, no HEAD-2 build is loaded and the process stays the same.
    await settle(3000);
    expect(eventKeys(cpdHome)).toEqual([
      "watchdog.break:empty-data",
      "watchdog.recovery:prod",
      "watchdog.break:websocket-stalled",
      "watchdog.recovery:head-1",
    ]);
    expect(markerPid(cpdHome)).toBe(head1Pid);
    expect((await currentView())?.title).toBe(`${TITLE_PREFIX}${RUNG["head-1"]}`);
  } finally {
    stopReports();
    await app.close().catch(() => undefined);
    await preview.close();
    await stopEverything(pids);
  }
});

test("a broken HEAD-1 walks on to HEAD-2 by the loaded view (air-d88, air-d95)", async ({ cpdHome, userDataDir }) => {
  const { app, page, preview } = await bootApp(cpdHome, userDataDir);
  const pids: number[] = [];
  try {
    await expect(page.locator("#built-view")).toHaveText(RUNG.preview, { timeout: 60_000 });
    await expect.poll(() => markerPid(cpdHome), { timeout: 30_000 }).not.toBeNull();
    const launchPid = markerPid(cpdHome) as number;
    pids.push(launchPid);

    const prodPid = await walkStep({ cpdHome, fromPid: launchPid, breakKind: "empty-data", breakEpoch: 1, nextRung: "prod" });
    pids.push(prodPid);
    const head1Pid = await walkStep({ cpdHome, fromPid: prodPid, breakKind: "websocket-stalled", breakEpoch: 3, nextRung: "head-1" });
    pids.push(head1Pid);

    // HEAD-1 turns out broken too: the ladder keeps walking down, to HEAD-2, and its page is the view
    // that loads — asserted by the build it names, not only by the Event that announced it.
    const head2Pid = await walkStep({ cpdHome, fromPid: head1Pid, breakKind: "empty-data", breakEpoch: 5, nextRung: "head-2" });
    pids.push(head2Pid);
    expect(eventKeys(cpdHome)).toEqual([
      "watchdog.break:empty-data",
      "watchdog.recovery:prod",
      "watchdog.break:websocket-stalled",
      "watchdog.recovery:head-1",
      "watchdog.break:empty-data",
      "watchdog.recovery:head-2",
    ]);
    expect(markerPid(cpdHome)).toBe(head2Pid);
  } finally {
    stopReports();
    await app.close().catch(() => undefined);
    await preview.close();
    await stopEverything(pids);
  }
});
