// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: a relaunch of the desktop main process comes back fresh and healthy (guards air-n31 /
// the Plan desktop-update relaunch contract).
//
// Bug air-n31: after an auto-update the renderer was newer than the running main process and the
// window came back blank. The update path's own repair is a full main-process relaunch
// (`app.relaunch()` + `app.exit()`), which `web/src/electron/main.ts` triggers from the update
// state and whose decision logic is unit-covered in `update/controller.test.ts`. This spec drives
// that exact relaunch pair and pins the e2e contract: a new main pid comes up and the window
// reports healthy again (the renderer's post-mount `health:ok`). The Playwright handle is gone
// after `app.exit`, so the assertions read the app's own health file.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "./e2e-fixtures";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";
const fakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

let staging = "";

/** The main pid of the process a test or its relaunch left behind, killed in teardown. */
let relaunchedPid: number | null = null;

function readHealth(cpdHome: string): { pid?: number; status?: string } | null {
  try {
    return JSON.parse(readFileSync(join(cpdHome, "run", "preview-health.json"), "utf8"));
  } catch {
    return null;
  }
}

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-update-"));
  const fakeBin = join(staging, "bin");
  const fakeShell = join(staging, "shell");
  mkdirSync(fakeBin, { recursive: true });
  // A fake login shell reporting the app's PATH unchanged, so the pty's `cpd` is the fake, not a
  // real cpd (`resolveLoginPath` would otherwise drop the fake bin dir).
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

test.afterEach(async () => {
  const pid = relaunchedPid;
  relaunchedPid = null;
  if (pid === null || pid <= 0 || pid === process.pid) return;
  try {
    process.kill(pid, "SIGTERM");
  } catch {
    return; // already gone
  }
  // Bounded wait for the process to actually die; SIGKILL it if it lingers.
  await expect
    .poll(() => {
      try {
        process.kill(pid, 0);
        return true;
      } catch {
        return false;
      }
    }, { timeout: 10_000 })
    .toBe(false)
    .catch(() => {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        /* already gone */
      }
    });
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

test("a relaunched main process comes back with a new pid and a healthy window (air-n31)", async ({ cpdHome, electronApp }) => {
  // The first main process is up and its window mounted (health:ok after the renderer's startup IPC).
  await expect.poll(() => readHealth(cpdHome)?.status ?? null, { timeout: 60_000 }).toBe("healthy");
  const firstPid = readHealth(cpdHome)?.pid ?? 0;
  expect(firstPid).toBeGreaterThan(0);

  // The exact relaunch pair the update path uses; the Playwright handle is gone right after.
  await electronApp.evaluate(({ app }) => {
    app.relaunch();
    app.exit(0);
  });

  // A new main pid appears, then its window reports healthy — the stale/blank-window failure air-n31
  // is exactly a main process that never came back with a fresh bundle.
  await expect
    .poll(() => {
      const h = readHealth(cpdHome);
      return h?.pid !== undefined && h.pid !== firstPid ? h.pid : null;
    }, { timeout: 60_000 })
    .not.toBeNull();
  relaunchedPid = readHealth(cpdHome)?.pid ?? null;
  await expect.poll(() => readHealth(cpdHome)?.status ?? null, { timeout: 60_000 }).toBe("healthy");
});
