// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the View menu switches the one Electron window between TUI, WebUI and WebUI Preview in
// place (guards the merged View menu: air-d40 always launch WebUI, air-d41 WebUI Preview in the
// View menu, air-d49 serve-only preview, air-d50 host tabs only in TUI).
//
// The three clients share one main process and one window: selecting the View menu writes the
// run-only marker `$CPD_HOME/run/ui-view.json` (`{view,pid}`) and sends the renderer `ui:mode`, so
// the window swaps `#terminal` and `#webui-root` without a relaunch (pid unchanged, never a second
// Electron). WebUI Preview runs the serve-only `scripts/preview-web.sh serve` and loads the dev
// renderer once that server is ready; a preview that never becomes ready rolls back to the current
// view within a bounded wait, and leaving the preview restores the production renderer, so a dead
// preview page never blocks a later View change (§19.3). (The failure toast is currently wiped by
// that restore: air-d94, owned by Plan desktop-shell; this spec pins the rollback it can observe.)
//
// The day's bug class is a blank/stale window after a relaunch (air-n31) and a frozen view
// (air-n13/n15): this spec pins that switching is in place, not a second process.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchApp, test as base, expect } from "./e2e-fixtures";

const webRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

const e2eFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

let staging = "";
/** The staged checkout root the app's in-place preview runs `scripts/preview-web.sh` from. */
let previewRoot = "";
/** A private `userData` dir, so the app writes nothing to the User's profile. */
let userDataDir = "";

/**
 * A deterministic stand-in for `scripts/preview-web.sh`. It replaces the real script (which drives
 * tmux and the real cpd) for the in-place preview: `serve` and `unserve` both succeed. The e2e
 * suite never runs tmux or a real preview server.
 */
const FAKE_PREVIEW_SCRIPT = `#!/bin/sh
exit 0
`;

/**
 * The shared `launchApp` helper, extended with the one seam this scenario needs: the staged
 * checkout root the in-place preview runs its serve-only script from (`CPD_CHECKOUT`, so no real
 * tmux/preview server runs). The helper itself pins the hidden window, the private `userData` dir
 * and the Linux flags (air-d68), and this fixture proves the window never showed before hiding it.
 */
const test = base.extend<{ electronApp: unknown }>({
  electronApp: async ({ cpdHome, rendererServer }, use) => {
    const app = await launchApp({
      cpdHome,
      userDataDir,
      rendererUrl: rendererServer.url,
      // The staged checkout root the in-place preview runs its serve-only script from (so no real
      // tmux/preview server runs).
      env: { CPD_CHECKOUT: previewRoot },
    });
    const first = await app.firstWindow();
    // The window is created hidden (air-d68): prove it never showed, before any hide() call.
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().every((w) => !w.isVisible()))).toBe(true);
    const window = await app.browserWindow(first);
    await window.evaluate((w) => w.hide());
    await use(app);
    await app.close();
  },

  page: async ({ electronApp }, use) => {
    const page = await electronApp.firstWindow();
    await page.waitForLoadState("domcontentloaded");
    await use(page);
  },
});

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-view-"));
  const fakeBin = join(staging, "bin");
  const fakeShell = join(staging, "shell");
  previewRoot = join(staging, "preview");
  userDataDir = join(staging, "user-data");
  mkdirSync(fakeBin, { recursive: true });
  mkdirSync(join(previewRoot, "scripts"), { recursive: true });
  mkdirSync(userDataDir, { recursive: true });
  // The staged checkout is a real git repo with an `origin/main` ref, so the WebUI's status call
  // (`git status` + `git log origin/main..HEAD`, run against `CPD_CHECKOUT`) succeeds instead of
  // surfacing an unrelated "not a git repository" error toast that would mask the preview failure.
  execFileSync("git", ["init", "-q"], { cwd: previewRoot });
  execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "commit", "--allow-empty", "-q", "-m", "e2e preview root"], { cwd: previewRoot });
  execFileSync("git", ["update-ref", "refs/remotes/origin/main", "HEAD"], { cwd: previewRoot });
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
  if (!existsSync(join(webRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
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

/** The number of windows the one Electron app owns; a second Electron would add one. */
function windowCount(app: { windows(): unknown[] }): number {
  return app.windows().length;
}

/** The app's health record, or `{}` when it is missing or malformed. */
function readHealth(cpdHome: string): { status?: string; reloadEpoch?: number; pid?: number; startedAt?: string } {
  try {
    return JSON.parse(readFileSync(join(cpdHome, "run", "preview-health.json"), "utf8"));
  } catch {
    return {};
  }
}

test("the View menu switches TUI, WebUI and WebUI Preview in one window (air-d40, air-d41, air-d49)", async ({ page, cpdHome, electronApp }) => {
  const mainPid = await electronApp.evaluate(() => process.pid);
  const markerPid = () => readMarker(cpdHome).pid;

  // Launched hidden and isolated: no CPD window on the User's screen, and the app writes to the
  // per-run `userData`, never the User's profile (air-d68).
  const window = await electronApp.browserWindow(page);
  expect(await window.evaluate((w) => w.isVisible())).toBe(false);
  expect(await electronApp.evaluate(({ app }) => app.getPath("userData"))).toContain(staging);

  // The app always launches in WebUI (air-d40): the WebUI is shown, the terminal hidden. Its own
  // health recorder reports the boot healthy (the renderer's post-mount `health:ok`).
  await expect(page.locator("#webui-root")).toBeVisible();
  await expect.poll(() => readHealth(cpdHome).status ?? null, { timeout: 30_000 }).toBe("healthy");
  await expect(page.locator("#terminal")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null, { timeout: 30_000 }).toBe("webui");
  expect(markerPid()).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);

  // TUI: the same main process swaps the window to the terminal in place.
  await selectView(electronApp, "TUI");
  await expect(page.locator("#terminal")).toBeVisible();
  await expect(page.locator("#webui-root")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null, { timeout: 30_000 }).toBe("tui");
  expect(markerPid()).toBe(mainPid);

  // WebUI again: back in place, still one process and one window.
  await selectView(electronApp, "WebUI");
  await expect(page.locator("#webui-root")).toBeVisible();
  await expect(page.locator("#terminal")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null, { timeout: 30_000 }).toBe("webui");
  expect(markerPid()).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);

  // WebUI Preview: the serve-only preview succeeds, and the dev URL (the suite's renderer server)
  // is ready at once, so the app loads the selected (WebUI) renderer in place; the marker records
  // the view. There is no circular wait on the app's own health (§19.3).
  await selectView(electronApp, "WebUI Preview");
  await expect.poll(() => readMarker(cpdHome).view ?? null, { timeout: 30_000 }).toBe("webui-preview");
  await expect(page.locator("#webui-root")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("#terminal")).toBeHidden();

  // No relaunch and no second Electron anywhere in the run: the main pid and the single window
  // never changed, and the marker still carries that same pid.
  expect(await electronApp.evaluate(() => process.pid)).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);
  expect(markerPid()).toBe(mainPid);
});

test("a preview that never becomes ready rolls back without hanging and never blocks the View change", async ({ page, cpdHome, electronApp, rendererServer }) => {
  const mainPid = await electronApp.evaluate(() => process.pid);
  await expect(page.locator("#webui-root")).toBeVisible();

  // Take the preview's dev server down: the preview starts but can never become ready. The
  // readiness wait is bounded (§19.3), so the switch fails after that bound instead of hanging, and
  // rolls back to the WebUI. No sleep: every poll is bounded by an explicit timeout, so a stuck
  // switch fails the test here rather than blocking it.
  await rendererServer.close();

  await selectView(electronApp, "WebUI Preview");

  // The serve-only preview still starts and shows the built-in loading page while it waits.
  await expect(page.getByText("Starting WebUI Preview…")).toBeVisible({ timeout: 30_000 });

  // The failure kept the WebUI (no silent fallback) and never relaunched: same pid, one window. The
  // visible failure toast is a known Preview defect (wiped by the renderer restore, air-d94, owned
  // by Plan desktop-shell), so this scenario pins the observable rollback only.
  await expect.poll(() => readMarker(cpdHome).view ?? null, { timeout: 30_000 }).toBe("webui");
  await expect(page.locator("#webui-root")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("#terminal")).toBeHidden();
  expect(await electronApp.evaluate(() => process.pid)).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);

  // The failed preview never blocks a later View change: TUI still lands in place, one window, one pid.
  await selectView(electronApp, "TUI");
  await expect(page.locator("#terminal")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator("#webui-root")).toBeHidden();
  await expect.poll(() => readMarker(cpdHome).view ?? null, { timeout: 30_000 }).toBe("tui");
  expect(await electronApp.evaluate(() => process.pid)).toBe(mainPid);
  expect(await windowCount(electronApp)).toBe(1);
});
