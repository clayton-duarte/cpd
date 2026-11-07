// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: boot renders the desktop window (guards air-n31 + the Plan app-boot webContents crash).
//
// Bug air-n31: after an auto-update, the renderer was newer than the running main process; the
// renderer's startup IPC called `pty:hosts`, the old main.js had no handler, and the window came
// back blank. This spec boots the real Electron app against the scripted fake cpd and pins the
// contract the bug violated: the window loads, the renderer root renders visible content (not
// blank), and no page error about a missing IPC handler is raised.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "./e2e-fixtures";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const fakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

let staging = "";

test.beforeAll(() => {
  // A private bin dir with the fake cpd as `cpd`, and a fake login shell that reports the app's
  // PATH unchanged. `resolveLoginPath` runs `$SHELL -ilc`; the real one would drop the fake dir, so
  // the pty (`cpd tui --attach`) would spawn a real cpd. The fake shell keeps the fake first.
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-boot-"));
  const fakeBin = join(staging, "bin");
  const fakeShell = join(staging, "shell");
  mkdirSync(fakeBin, { recursive: true });
  writeFileSync(
    fakeShell,
    `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`,
  );
  chmodSync(fakeShell, 0o755);
  symlinkSync(fakeCpd, join(fakeBin, "cpd"));
  process.env["CPD_BIN"] = fakeCpd;
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
  if (!existsSync(join(repoRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: repoRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

test("boot renders the desktop window with no missing IPC handler error (air-n31)", async ({ electronApp, page }) => {
  // Hidden + isolated launch (air-d68): no window is visible on screen, and the app's own
  // userData lives in the per-run temp dir, never the User's real profile or lock.
  const launch = await electronApp.evaluate(({ app, BrowserWindow }) => ({
    visible: BrowserWindow.getAllWindows().map((w) => w.isVisible()),
    userData: app.getPath("userData"),
  }));
  expect(launch.visible).not.toContain(true);
  expect(launch.userData).toContain("cpd-e2e-userdata-");

  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(String(error)));

  // The renderer root the WebUI mounts into must be visible and hold rendered content (air-n31's
  // blank window had an empty root).
  const root = page.locator("#webui-root");
  await expect(root).toBeVisible();
  await expect(root.locator("[data-shell]")).toBeVisible();
  // And the cockpit data reached the renderer over the WebSocket transport: the tree pane left its
  // "Loading contexts…" state for the fake's empty snapshot.
  await expect(root.locator('[data-part="tree-section"] [data-part="empty"]')).toHaveText("No contexts");

  // The startup IPC resolved against the real main process: no page error about a missing handler.
  const missingHandler = /No handler registered|pty:hosts/i;
  expect(pageErrors.filter((e) => missingHandler.test(e))).toEqual([]);
});
