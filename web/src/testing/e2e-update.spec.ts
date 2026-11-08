// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: a relaunch of the desktop main process comes back fresh and healthy, and promotes a
// ready bundle whose `.built-sha` matches HEAD (guards air-n31 + air-n67 / the desktop-update
// relaunch and launch-promote contract).
//
// Bug air-n31: after an auto-update the renderer was newer than the running main process and the
// window came back blank. The update path's own repair is a full main-process relaunch
// (`app.relaunch()` + `app.exit()`), which `web/src/electron/main.ts` triggers from the update
// state and whose decision logic is unit-covered in `update/controller.test.ts`. This spec drives
// that exact relaunch pair and pins the e2e contract: a new main pid comes up and the window
// reports healthy again (the renderer's post-mount `health:ok`). The Playwright handle is gone
// after `app.exit`, so the assertions read the app's own health file.
//
// air-n67 / §19.1: before the window opens, a launch swaps `web/dist-electron.next` into
// `web/dist-electron` when `.next/.built-sha` equals `git rev-parse HEAD`; otherwise it does
// nothing. The second scenario stages a checkout whose ready `.next` matches HEAD, relaunches, and
// asserts the promoted bundle (the `.next` is gone, `dist-electron` carries its contents).
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchApp, test as base, expect } from "./e2e-fixtures";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";
const fakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

let staging = "";
/** A staged checkout root (a real git repo) the app promotes from; `CPD_CHECKOUT` points at it. */
let stageRoot = "";
/** The staged checkout's `HEAD`, the sha a ready `.next` must carry to be promoted. */
let headSha = "";

/** The main pid of the process a test or its relaunch left behind, killed in teardown. */
let relaunchedPid: number | null = null;

function readHealth(cpdHome: string): { pid?: number; status?: string } | null {
  try {
    return JSON.parse(readFileSync(join(cpdHome, "run", "preview-health.json"), "utf8"));
  } catch {
    return null;
  }
}

/** A file's text, or `null` when it is missing. */
function readText(path: string): string | null {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return null;
  }
}

/**
 * The shared `launchApp` helper, pointed at the staged checkout through `CPD_CHECKOUT` so the
 * launch-promote path (`promoteOnLaunch`) resolves a root it can promote from. It pins the hidden
 * window, the private `userData` dir and the Linux flags (air-d68).
 */
const test = base.extend<{ electronApp: unknown }>({
  electronApp: async ({ cpdHome, userDataDir, rendererServer }, use) => {
    const app = await launchApp({
      cpdHome,
      userDataDir,
      rendererUrl: rendererServer.url,
      env: { CPD_CHECKOUT: stageRoot },
    });
    await use(app);
    await app.close();
  },
});

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-update-"));
  const fakeBin = join(staging, "bin");
  const fakeShell = join(staging, "shell");
  stageRoot = join(staging, "checkout");
  mkdirSync(fakeBin, { recursive: true });
  // The staged checkout is a real git repo with an `origin/main` ref: `promoteOnLaunch` reads HEAD
  // from it, and the WebUI's status call (`git status` + `git log origin/main..HEAD`) succeeds.
  mkdirSync(join(stageRoot, "web", "dist-electron"), { recursive: true });
  writeFileSync(join(stageRoot, "web", "dist-electron", ".built-sha"), "stale");
  execFileSync("git", ["init", "-q"], { cwd: stageRoot });
  execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "commit", "--allow-empty", "-q", "-m", "e2e checkout"], { cwd: stageRoot });
  headSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: stageRoot }).toString().trim();
  execFileSync("git", ["update-ref", "refs/remotes/origin/main", "HEAD"], { cwd: stageRoot });
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

test("a relaunch promotes a ready .next whose .built-sha matches HEAD (air-n67)", async ({ cpdHome, electronApp }) => {
  const dist = join(stageRoot, "web", "dist-electron");
  const next = join(stageRoot, "web", "dist-electron.next");

  // First launch: no ready `.next` exists, so nothing is promoted and the checkout's built bundle
  // is untouched.
  await expect.poll(() => readHealth(cpdHome)?.status ?? null, { timeout: 60_000 }).toBe("healthy");
  const firstPid = readHealth(cpdHome)?.pid ?? 0;
  expect(firstPid).toBeGreaterThan(0);
  expect(readText(join(dist, ".built-sha"))).toBe("stale");
  expect(existsSync(next)).toBe(false);

  // A ready bundle appears while the app runs: its `.built-sha` equals the checkout's HEAD, so the
  // next launch must swap it into `dist-electron` before the window opens (web/CONTRACT.md §19.1).
  mkdirSync(next, { recursive: true });
  writeFileSync(join(next, ".built-sha"), headSha);
  writeFileSync(join(next, "promoted-marker"), "new");

  // The exact relaunch pair the update path uses; the Playwright handle is gone right after.
  await electronApp.evaluate(({ app }) => {
    app.relaunch();
    app.exit(0);
  });

  // The relaunched main promotes the ready bundle: `dist-electron` now carries the `.next` contents
  // and the consumed `.next` is gone.
  await expect.poll(() => readText(join(dist, ".built-sha")), { timeout: 60_000 }).toBe(headSha);
  expect(readText(join(dist, "promoted-marker"))).toBe("new");
  expect(existsSync(next)).toBe(false);

  // The relaunched process is fresh (a new main pid), left for the afterEach to kill.
  await expect
    .poll(() => {
      const h = readHealth(cpdHome);
      return h?.pid !== undefined && h.pid !== firstPid ? h.pid : null;
    }, { timeout: 60_000 })
    .not.toBeNull();
  relaunchedPid = readHealth(cpdHome)?.pid ?? null;
});
