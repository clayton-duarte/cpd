// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the View menu switches the one Electron window between TUI, WebUI and WebUI Preview in
// place (guards the merged View menu: air-d40 always launch WebUI, air-d41 WebUI Preview in the
// View menu, air-d49 serve-only preview, air-d50 host tabs only in TUI).
//
// The three clients share one main process and one window: selecting the View menu writes the
// run-only marker `$CPD_HOME/run/ui-view.json` (`{view,pid}`) and sends the renderer `ui:mode`, so
// the window swaps `#terminal` and `#webui-root` without a relaunch (pid unchanged, never a second
// Electron). WebUI Preview runs the serve-only `scripts/preview-web.sh serve`, shows the built-in
// loading page while the client health is `loading`, and loads the dev renderer once it is
// `healthy`. A start or load failure shows a native error dialog and keeps the current view — no
// silent fallback.
//
// The day's bug class is a blank/stale window after a relaunch (air-n31) and a frozen view
// (air-n13/n15): this spec pins that switching is in place, not a second process.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { _electron as electron } from "playwright";
import { test as base, expect } from "./e2e-fixtures";

const webRoot = fileURLToPath(new URL("../..", import.meta.url));
const repoRoot = webRoot;
/** The pinned Electron binary the app runs on (the `electron` devDependency). */
const electronExecutable = createRequire(import.meta.url)("electron") as string;
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

const e2eFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

let staging = "";
/** The staged checkout root the app's in-place preview runs `scripts/preview-web.sh` from. */
let previewRoot = "";
/** Touch this to make the fake `preview-web.sh serve` fail loudly (the failure scenario). */
let failMarker = "";
/** A private `userData` dir, so the app writes nothing to the User's profile. */
let userDataDir = "";

/**
 * A deterministic stand-in for `scripts/preview-web.sh`. It replaces the real script (which drives
 * tmux and the real cpd) for the in-place preview: `serve` succeeds unless the fail marker exists,
 * `unserve` always succeeds. The e2e suite never runs tmux or a real preview server.
 */
const FAKE_PREVIEW_SCRIPT = `#!/bin/sh
dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
case "$1" in
  serve)
    if [ -f "$dir/fail" ]; then
      echo "fake preview: serve failed" >&2
      exit 1
    fi
    exit 0
    ;;
  *)
    exit 0
    ;;
esac
`;

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-view-"));
  const fakeBin = join(staging, "bin");
  const fakeShell = join(staging, "shell");
  previewRoot = join(staging, "preview");
  failMarker = join(previewRoot, "scripts", "fail");
  userDataDir = join(staging, "user-data");
  mkdirSync(fakeBin, { recursive: true });
  mkdirSync(join(previewRoot, "scripts"), { recursive: true });
  mkdirSync(userDataDir, { recursive: true });
  // A fake login shell reporting the app's PATH unchanged, so the pty's `cpd` is the fake, not a
  // real cpd (`resolveLoginPath` would otherwise drop the fake bin dir).
  writeFileSync(
    fakeShell,
    `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`,
  );
  chmodSync(fakeShell, 0o755);
  writeFileSync(join(previewRoot, "scripts", "preview-web.sh"), FAKE_PREVIEW_SCRIPT);
  chmodSync(join(previewRoot, "scripts", "preview-web.sh"), 0o755);
  symlinkSync(e2eFakeCpd, join(fakeBin, "cpd"));
  process.env["CPD_BIN"] = e2eFakeCpd;
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
  if (!existsSync(join(repoRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: repoRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

/**
 * The shared launcher, extended with the two seams the View scenario needs: the staged checkout
 * root the in-place preview runs its serve-only script from (`CPD_CHECKOUT`), and a hidden window
 * with a private `userData` (air-d68: no window on the User's screen, no writes to their profile).
 */
const test = base.extend<{ electronApp: unknown }>({
  electronApp: async ({ cpdHome, rendererServer }, use) => {
    const app = await electron.launch({
      executablePath: electronExecutable,
      args: [".", `--user-data-dir=${userDataDir}`],
      cwd: webRoot,
      env: {
        ...process.env,
        CPD_HOME: cpdHome,
        CPD_MODE: "preview",
        CPD_CHECKOUT: previewRoot,
        CPD_ELECTRON_DEV_URL: rendererServer.url,
      },
    });
    await app.firstWindow();
    await app.evaluate(({ BrowserWindow }) => {
      for (const window of BrowserWindow.getAllWindows()) window.hide();
    });
    await use(app);
    await app.close();
  },

  page: async ({ electronApp }, use) => {
    const page = await electronApp.firstWindow();
    await page.waitForLoadState("domcontentloaded");
    await use(page);
  },
});

/** The run-only view marker, written on launch and on every View menu switch (air-n19). */
function readMarker(cpdHome: string): { view?: string; pid?: number } {
  try {
    return JSON.parse(readFileSync(join(cpdHome, "run", "ui-view.json"), "utf8"));
  } catch {
    return {};
  }
}

/** Select a View menu item by its exact label, the way a User clicks it. */
async function selectView(app: unknown, label: string): Promise<void> {
  await app.evaluate(({ Menu }, wanted) => {
    const menu = Menu.getApplicationMenu();
    const view = menu?.items.find((item) => item.label === "View");
    const target = view?.submenu?.items.find((item) => item.label === wanted);
    if (!target) throw new Error(`View menu has no ${wanted} item`);
    target.click();
  }, label);
}

/** The number of BrowserWindows the one main process owns; a second Electron would add one. */
function windowCount(app: unknown): Promise<number> {
  return app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length);
}

/** Tell the app the preview client health turned healthy, so it loads the dev renderer. */
function markPreviewHealthy(cpdHome: string): void {
  const file = join(cpdHome, "run", "preview-health.json");
  const loading = JSON.parse(readFileSync(file, "utf8"));
  const tmp = `${file}.e2e.tmp`;
  writeFileSync(
    tmp,
    JSON.stringify({
      status: "healthy",
      reloadEpoch: loading.reloadEpoch,
      pid: loading.pid,
      startedAt: loading.startedAt,
      healthyAt: new Date().toISOString(),
    }),
  );
  renameSync(tmp, file);
}

test("the View menu switches TUI, WebUI and WebUI Preview in one window (air-d40, air-d41, air-d49)", async ({ page, cpdHome, electronApp }) => {
  const mainPid = await electronApp.evaluate(() => process.pid);
  const markerPid = () => readMarker(cpdHome).pid;

  // The app always launches in WebUI (air-d40): the WebUI is shown, the terminal hidden.
  await expect(page.locator("#webui-root")).toBeVisible();
  await expect(page.locator("#terminal")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null).toBe("webui");
  expect(markerPid()).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);

  // TUI: the same main process swaps the window to the terminal in place.
  await selectView(electronApp, "TUI");
  await expect(page.locator("#terminal")).toBeVisible();
  await expect(page.locator("#webui-root")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null).toBe("tui");
  expect(markerPid()).toBe(mainPid);

  // WebUI again: back in place, still one process and one window.
  await selectView(electronApp, "WebUI");
  await expect(page.locator("#webui-root")).toBeVisible();
  await expect(page.locator("#terminal")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null).toBe("webui");
  expect(markerPid()).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);

  // WebUI Preview: the serve-only preview succeeds, the built-in loading page shows while the
  // client health is loading, then the selected (WebUI) view renders once the health is healthy.
  await selectView(electronApp, "WebUI Preview");
  await expect(page.getByText("Starting WebUI Preview…")).toBeVisible();
  await expect.poll(() => readMarker(cpdHome).view ?? null).toBe("webui-preview");
  markPreviewHealthy(cpdHome);
  await expect(page.locator("#webui-root")).toBeVisible();
  await expect(page.locator("#terminal")).toBeHidden();

  // No relaunch and no second Electron anywhere in the run: the main pid and the single window
  // never changed, and the marker still carries that same pid.
  expect(await electronApp.evaluate(() => process.pid)).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);
  expect(markerPid()).toBe(mainPid);
});

test("a preview start failure keeps the current view and surfaces loudly, with no silent fallback", async ({ page, cpdHome, electronApp }) => {
  const mainPid = await electronApp.evaluate(() => process.pid);
  await expect(page.locator("#webui-root")).toBeVisible();

  // Observe the native dialog without opening a modal window that would block the main process.
  await electronApp.evaluate(({ dialog }) => {
    const seen: { title: string; content: string }[] = [];
    (globalThis as Record<string, unknown>)["__cpdViewDialogs"] = seen;
    dialog.showErrorBox = (title: string, content: string) => {
      seen.push({ title, content });
    };
  });

  // The fake preview server fails on start.
  writeFileSync(failMarker, "fail");

  await selectView(electronApp, "WebUI Preview");

  // Loud, not silent: the native dialog carries the failure, and the current view (WebUI) stays.
  await expect
    .poll(async () => (await electronApp.evaluate(() => (globalThis as Record<string, unknown>)["__cpdViewDialogs"] as { title: string }[])).map((entry) => entry.title))
    .toEqual(["WebUI Preview"]);
  await expect.poll(() => readMarker(cpdHome).view ?? null).toBe("webui");
  await expect(page.locator("#webui-root")).toBeVisible();
  await expect(page.locator("#terminal")).toBeHidden();

  // The failure never relaunched the app: same pid, one window.
  expect(await electronApp.evaluate(() => process.pid)).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);
});
