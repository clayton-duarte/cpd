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
// renderer page names its own build twice — visibly in `#built-view` and to the suite's probe
// (`<probe>/view?build=<rung>&href=<location.href>`), a beacon the page fires only if it really
// ran. That is how the spec asserts the loaded view after a relaunch, when Playwright's handle to
// the first main process is gone: the renderer Electron loaded is the one that reports itself, and
// its `href` names the file it came from (`file://…/dist-electron/renderer/electron.html` for prod
// and HEAD-1, the suite's dev URL for preview).
//
// The break each rung gets is the renderer's own cockpit/transport report
// (`$CPD_HOME/run/cockpit-health.json`, web/CONTRACT.md §15.2), the signal the watchdog reads in
// every view mode (web/src/electron/main.ts `startHealthWatchdog`):
//   - preview: `empty-data` — the view painted but never received a cockpit snapshot, so it never
//     becomes healthy;
//   - prod: `websocket-stalled` — the loaded build's transport never delivers a frame;
//   - HEAD-1: present and healthy — a real snapshot and a live heartbeat, so the walk stops.
// The report is written the way a renderer writes it (an atomic rename), and the spec waits for the
// watchdog's own `watchdog.break` Event before it turns the report healthy again, so the relaunched
// process finds a healthy rung and does not revert on the break of the rung above it.
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, firstWindow, launchApp, stageFakeBin, test, webRoot } from "./e2e-fixtures";

const sharedFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

/** The fake `git` reports this for HEAD and origin/main, so the app's update pass is a no-op. */
const HEAD_SHA = "e2e-watchdog-head-sha";
/** The one build marker per rung, written into that rung's renderer page (`#built-view`). */
const RUNG = {
  preview: "preview-build",
  prod: "prod-build",
  "head-1": "head-1-build",
  "head-2": "head-2-build",
} as const;

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

let staging = "";
/** The staged checkout the app resolves (`CPD_CHECKOUT`); the ladder lives under its `web/`. */
let checkout = "";
/** The staged app dir the app is launched from: `<checkout>/web` (its `dist-electron` is prod). */
let appDir = "";
/** A staged `HOME`, so the CLI bootstrap finds an installed CLI and never reads the User's home. */
let stagedHome = "";

/** A rung the suite has seen a renderer report. */
type ViewHit = { build: string; href: string };

/**
 * The suite's renderer URL and view probe. `/electron.html` is the preview view (the dev URL the
 * app loads in preview mode); every rung's page beacons its own build and `location.href` to
 * `/view`, so the spec knows which build Electron actually loaded.
 */
type ViewProbe = { readonly url: string; readonly hits: ViewHit[]; close(): Promise<void> };

/** Each rung's page names its build visibly and beacons it to the probe (it runs only if loaded). */
function viewPage(build: string, probeBase: string): string {
  return (
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>CPD</title></head><body>` +
    `<div id="webui-root"><div id="built-view">${build}</div></div>` +
    `<img id="view-beacon" width="1" height="1" alt="" src="data:,">` +
    `<script>document.getElementById("view-beacon").src = ${JSON.stringify(`${probeBase}/view`)} +` +
    ` "?build=" + encodeURIComponent(${JSON.stringify(build)}) +` +
    ` "&href=" + encodeURIComponent(location.href) + "&t=" + Date.now();</script>` +
    `</body></html>`
  );
}

async function startViewProbe(): Promise<ViewProbe> {
  const hits: ViewHit[] = [];
  let previewPage = "";
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    if (url.pathname === "/electron.html") {
      response.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" }).end(previewPage);
      return;
    }
    if (url.pathname === "/view") {
      hits.push({ build: url.searchParams.get("build") ?? "", href: url.searchParams.get("href") ?? "" });
      response.writeHead(204, { "cache-control": "no-store" }).end();
      return;
    }
    response.writeHead(404).end("not found");
  });
  await new Promise<void>((listening, failed) => {
    server.once("error", failed);
    server.listen(0, "127.0.0.1", listening);
  });
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("view probe has no port");
  const url = `http://127.0.0.1:${address.port}`;
  previewPage = viewPage(RUNG.preview, url);
  return {
    url: `${url}/electron.html`,
    hits,
    close: () => new Promise<void>((closed) => server.close(() => closed())),
  };
}

/** Stage one rung: the real main process, its server, and a renderer page that names its build. */
function stageRung(dir: string, build: string, probeBase: string): void {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "renderer"), { recursive: true });
  for (const file of ["main.js", "preload.cjs", "server.js"]) {
    cpSync(join(webRoot, "dist-electron", file), join(dir, file));
  }
  writeFileSync(join(dir, "renderer", "electron.html"), viewPage(build, probeBase));
}

/** Reset every rung to its pre-test state: the ladder reverts rewrite these dirs. */
function resetRungs(probeBase: string): void {
  stageRung(join(appDir, "dist-electron"), RUNG.prod, probeBase);
  stageRung(join(appDir, "dist-electron.head-1"), RUNG["head-1"], probeBase);
  stageRung(join(appDir, "dist-electron.head-2"), RUNG["head-2"], probeBase);
  // The prod build carries HEAD's sha, so the app's own update pass builds and installs nothing.
  writeFileSync(join(appDir, "dist-electron", ".built-sha"), HEAD_SHA);
  for (const leftover of ["dist-electron.revert", "dist-electron.displaced", "dist-electron.preview"]) {
    rmSync(join(appDir, leftover), { recursive: true, force: true });
  }
}

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-watchdog-"));
  checkout = join(staging, "checkout");
  appDir = join(checkout, "web");
  stagedHome = join(staging, "home");

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
  writeFileSync(
    join(appDir, "package.json"),
    JSON.stringify({ name: "cpd-watchdog-e2e", version: "0.0.0", private: true, type: "module", main: "dist-electron/main.js" }),
  );
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
