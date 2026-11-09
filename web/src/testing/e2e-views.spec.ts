// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the app renders the prod view built from the checkout and the preview view built from
// the preview branch (guards air-d89 points 3-4, air-d96).
//
// air-d89 points 3-4: the WebUI prod view is the default build of the latest commit on main, built
// from the checkout. air-d96: the preview view is that same prod build of the preview branch, kept
// in its own build dir (`dist-electron.preview`) so both can be on disk at once. This spec boots the
// real Electron app against a staged checkout whose fake build command (the house pattern of
// e2e-update-nowait.spec.ts) writes a distinctive renderer per view; the app's own auto-update
// rebuilds both views from their checkouts (prod from the checkout, preview from the preview
// branch), and the app renders the built prod renderer for prod and the built preview renderer for
// preview.
//
// Everything is hermetic: a fake `cpd` (the shared `e2e-fake-cpd`), a fake `git`, `pnpm`, `cargo.sh`
// and install script in a staged bin dir and checkout — no real git, no real build, no real cpd, no
// tmux, no network. The app is launched hidden and isolated (air-d68: a per-run `userData` dir and
// `CPD_HOME`); no window shows and every process the app spawns dies with it.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, firstWindow, launchApp, stageFakeBin, startRendererServer, test, webRoot } from "./e2e-fixtures";

const sharedFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

/** The fake `git` reports this for HEAD and origin/main; `syncCheckout` then sees a current checkout. */
const HEAD_SHA = "e2e-views-head-sha";
/** The sha the CLI is recorded as installed from; differing from HEAD_SHA makes the app install and rebuild. */
const OLD_CLI_SHA = "e2e-views-old-cli-sha";
/** The marker the fake build writes into the prod renderer (built from the checkout). */
const PROD_MARKER = "prod-view-from-checkout";
/** The marker the fake build writes into the preview renderer (built from the preview branch). */
const PREVIEW_MARKER = "preview-view-from-branch";
/** The page served before the app's own build runs, so the boot load never 404s. */
const PLACEHOLDER = '<!doctype html><html><head><meta charset="utf-8"><title>CPD</title></head><body><div id="placeholder">booting</div></body></html>';

let staging = "";
/** The staged checkout the app's auto-update builds the prod view from. */
let checkout = "";
/** The staged preview worktree the app builds the preview view from (on branch `preview`). */
let previewCheckout = "";
/** A staged `HOME`, so `ensureCli` finds an installed CLI and the run never reads the User's home. */
let stagedHome = "";
/** The scenario's fake `cpd`: the shared fake (empty panes, no running Attempts). */
let fakeCpd = "";

/**
 * The fake `git`: `--abbrev-ref` answers `preview` (so the preview worktree is on the preview
 * branch) and every other query answers HEAD_SHA. `fetch origin` succeeds. No real repo is touched.
 */
const FAKE_GIT = `#!/bin/sh
case "$*" in
  *--abbrev-ref*) printf '%s\\n' 'preview'; exit 0 ;;
esac
printf '%s\\n' '${HEAD_SHA}'
exit 0
`;

/**
 * The fake build command: `pnpm --filter cpd-web-client run build:electron` writes a distinctive
 * renderer into `web/<CPD_ELECTRON_OUT>/renderer/electron.html` — the prod marker for
 * `dist-electron`, the preview marker for `dist-electron.preview`. This is the one build command
 * prod and preview share (air-d89 point 3), named to its output dir through `CPD_ELECTRON_OUT`.
 */
const FAKE_PNPM = `#!/bin/sh
out="$CPD_ELECTRON_OUT"
case "$out" in
  dist-electron.preview) marker="${PREVIEW_MARKER}" ;;
  *) marker="${PROD_MARKER}" ;;
esac
for arg in "$@"; do
  if [ "$arg" = "build:electron" ]; then
    mkdir -p "web/$out/renderer"
    printf '<!doctype html><html><head><meta charset="utf-8"><title>CPD</title></head><body><div id="built-view">%s</div></body></html>' "$marker" > "web/$out/renderer/electron.html"
    exit 0
  fi
done
exit 0
`;

/** The fake `cargo.sh` and `install-cpd.sh`: the CLI install succeeds without building or installing. */
const FAKE_OK = `#!/bin/sh
exit 0
`;

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-views-"));
  checkout = join(staging, "checkout");
  previewCheckout = join(staging, "cpd.preview.web");
  stagedHome = join(staging, "home");
  fakeCpd = join(staging, "cpd");
  mkdirSync(join(stagedHome, ".local", "bin"), { recursive: true });
  mkdirSync(join(checkout, "scripts"), { recursive: true });
  mkdirSync(join(checkout, ".git"), { recursive: true });
  mkdirSync(join(checkout, "target"), { recursive: true });
  mkdirSync(join(checkout, "web", "dist-electron"), { recursive: true });
  mkdirSync(join(previewCheckout, "web", "dist-electron.preview"), { recursive: true });

  // An already-installed CLI, so `ensureCli` is `ready` and never touches the User's real home.
  writeFileSync(join(stagedHome, ".local", "bin", "cpd"), FAKE_OK);
  chmodSync(join(stagedHome, ".local", "bin", "cpd"), 0o755);

  // The prod checkout the updater resolves: `scripts/install-cpd.sh` and `.git` must exist.
  writeFileSync(join(checkout, "scripts", "install-cpd.sh"), FAKE_OK);
  chmodSync(join(checkout, "scripts", "install-cpd.sh"), 0o755);
  writeFileSync(join(checkout, "scripts", "cargo.sh"), FAKE_OK);
  chmodSync(join(checkout, "scripts", "cargo.sh"), 0o755);

  mkdirSync(join(checkout, "web", "dist-electron", "renderer"), { recursive: true });
  mkdirSync(join(previewCheckout, "web", "dist-electron.preview", "renderer"), { recursive: true });
  resetStaging();

  // The house fake-cpd-on-PATH setup, then the fake `git` and `pnpm` the update shells out to.
  const { bin } = stageFakeBin(staging, fakeCpd);
  writeFileSync(fakeCpd, `#!/bin/sh\nexec "${sharedFakeCpd}" "$@"\n`);
  chmodSync(fakeCpd, 0o755);
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

/**
 * Reset the staged checkout to its pre-build state, before every test. The app's own update mutates
 * the shared staging (it replaces both renderers with the built marker and records the CLI sha it
 * installed from), so without this the first test would leave the second nothing to build. The prod
 * build already carries HEAD's sha (the bundle flow is a no-op), the CLI's recorded sha differs (so
 * the CLI install runs and rebuilds both views, air-d89 point 6), and both renderers hold the
 * served placeholder so the boot load never 404s.
 */
function resetStaging(): void {
  writeFileSync(join(checkout, "web", "dist-electron", ".built-sha"), HEAD_SHA);
  writeFileSync(join(checkout, "target", ".cli-installed-sha"), OLD_CLI_SHA);
  writeFileSync(join(checkout, "web", "dist-electron", "renderer", "electron.html"), PLACEHOLDER);
  writeFileSync(join(previewCheckout, "web", "dist-electron.preview", "renderer", "electron.html"), PLACEHOLDER);
}

test.beforeEach(() => {
  resetStaging();
});

/** The env every launch needs: the staged checkout, the preview worktree and an isolated HOME. */
function viewEnv(): Record<string, string> {
  return { CPD_CHECKOUT: checkout, CPD_WEB_PREVIEW_WORKTREE: previewCheckout, HOME: stagedHome };
}

/** The built renderer file's contents, or `null` while the app's build is still writing it. */
function readRenderer(dir: string): string | null {
  try {
    return readFileSync(join(dir, "electron.html"), "utf8");
  } catch {
    return null;
  }
}

/**
 * Boot the app against `serveRoot` (served as the app's dev URL) and return it with its first
 * window. The launch is hidden and isolated (air-d68); the caller closes the app and the server.
 */
async function boot(serveRoot: string, cpdHome: string, userDataDir: string) {
  const server = await startRendererServer(serveRoot);
  const app = await launchApp({ cpdHome, userDataDir, rendererUrl: server.url, env: viewEnv() });
  const page = await firstWindow(app);
  await page.waitForLoadState("domcontentloaded");
  return { app, page, server };
}

test("the app renders the prod view built from the checkout (air-d89 points 3-4)", async ({ cpdHome, userDataDir }) => {
  const prodRenderer = join(checkout, "web", "dist-electron", "renderer");
  const { app, page, server } = await boot(prodRenderer, cpdHome, userDataDir);
  try {
    // Hidden and isolated (air-d68): no window on the User's screen, everything under the temp dirs.
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().every((w) => !w.isVisible()))).toBe(true);
    expect(await app.evaluate(({ app: electronApp }) => electronApp.getPath("userData"))).toContain("cpd-e2e-userdata-");

    // The updater may finish before the first page load, so the placeholder is not guaranteed.
    // The app's own auto-update rebuilt the prod view from the checkout with the shared build
    // command, writing the deterministic prod marker into the served build dir.
    await expect
      .poll(() => readRenderer(prodRenderer)?.includes(PROD_MARKER) ?? false, { timeout: 60_000 })
      .toBe(true);

    // The app renders the build made from its checkout.
    await page.reload();
    await expect(page.locator("#built-view")).toHaveText(PROD_MARKER, { timeout: 30_000 });
    expect(await page.locator("#placeholder").count()).toBe(0);

    // One window, and the reload never relaunched the app.
    expect(app.windows().length).toBe(1);
  } finally {
    await app.close();
    await server.close();
  }
});

test("the app renders the preview view built from the preview branch (air-d96)", async ({ cpdHome, userDataDir }) => {
  const previewRenderer = join(previewCheckout, "web", "dist-electron.preview", "renderer");
  const { app, page, server } = await boot(previewRenderer, cpdHome, userDataDir);
  try {
    // Hidden and isolated (air-d68).
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().every((w) => !w.isVisible()))).toBe(true);

    // The updater may finish before the first page load, so the placeholder is not guaranteed.
    // The app's own auto-update built the preview view from the preview branch (on branch `preview`,
    // into its own `dist-electron.preview` dir) with the same shared build command.
    await expect
      .poll(() => readRenderer(previewRenderer)?.includes(PREVIEW_MARKER) ?? false, { timeout: 60_000 })
      .toBe(true);

    // The app renders the build made from the preview branch.
    await page.reload();
    await expect(page.locator("#built-view")).toHaveText(PREVIEW_MARKER, { timeout: 30_000 });
    expect(await page.locator("#placeholder").count()).toBe(0);

    // The prod view was built too, from the checkout, at the same time (air-d89 point 6), and kept
    // apart from the preview view (air-d96): both are on disk at once.
    await expect
      .poll(() => readRenderer(join(checkout, "web", "dist-electron", "renderer"))?.includes(PROD_MARKER) ?? false, { timeout: 60_000 })
      .toBe(true);

    expect(app.windows().length).toBe(1);
  } finally {
    await app.close();
    await server.close();
  }
});
