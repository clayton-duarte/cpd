// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the CLI update install runs with a running Attempt and the previous prod build remains
// (guards air-n81 / air-d95).
//
// air-n81: the desktop updater's full CLI install — the one that carries the follow-ups, the engine
// restart included — was gated on zero running Attempts, so under steady load it never ran and the
// CLI stayed stale. This spec boots the real Electron app against a staged checkout whose fake `cpd`
// reports one running Attempt and whose fake `install-cpd.sh` records that it ran; the app's own
// auto-update must run the full install anyway, at once, with no wait. air-d95: the update's bundle
// swap keeps the displaced prod build as HEAD-1 (demoting the former HEAD-1 to HEAD-2) instead of
// deleting it, so this spec also pins that the staged previous prod build is still on disk after the
// swap.
//
// Everything is hermetic: a fake `cpd` (the shared `e2e-fake-cpd`, wrapped to report the running
// Attempt), a fake `git`, `pnpm` and install/cargo scripts in a staged bin dir and checkout — no
// real git, no real build, no network, no real cpd. The app is launched hidden and isolated (air-d68)
// by the extended `electronApp` fixture; no window shows and every process the app spawns dies with it.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, launchApp, stageFakeBin, test as base, webRoot } from "./e2e-fixtures";

const sharedFakeCpd = fileURLToPath(new URL("./e2e-fake-cpd", import.meta.url));

/** The sha the fake `git` reports for both HEAD and origin/main, so `syncCheckout` is a no-op. */
const HEAD_SHA = "e2e-head-sha";
const CUR_MAIN = "cur-main";
const PREV_MAIN = "prev-main";

let staging = "";
/** The staged checkout the app's auto-update runs `git`/`pnpm`/`scripts/*.sh` against. */
let checkout = "";
/** A staged `HOME`, so `ensureCli` finds an installed CLI and the run never reads the User's home. */
let stagedHome = "";
/** The scenario's fake `cpd`: the shared fake, wrapped to report one running Attempt. */
let fakeCpd = "";
/** Written by the full `install-cpd.sh` (no `--no-follow-up`): the install air-n81 starved. */
let installMarker = "";
/** Written by `install-cpd.sh --no-follow-up`: the binary-only step that always ran. */
let binaryMarker = "";

/**
 * The fake `install-cpd.sh`: `--no-follow-up` records the binary step, and the full install (no
 * args) records the follow-up-carrying step air-n81 starved.
 */
const FAKE_INSTALL = `#!/bin/sh
if [ "$1" = "--no-follow-up" ]; then
  : > "$CPD_E2E_INSTALL_BINARY_MARKER"
  exit 0
fi
: > "$CPD_E2E_INSTALL_MARKER"
exit 0
`;

/**
 * The fake `git`: `fetch origin` succeeds and both `rev-parse HEAD` and `rev-parse origin/main`
 * answer HEAD_SHA, so `syncCheckout` sees HEAD === origin/main and skips without touching a repo.
 */
const FAKE_GIT = `#!/bin/sh
if [ "$1" = "rev-parse" ]; then printf '%s\\n' "${HEAD_SHA}"; fi
exit 0
`;

/** The fake `pnpm`: `pnpm install` is a no-op and `build:electron` emits a deterministic next build. */
const FAKE_PNPM = `#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "build:electron" ]; then
    out="web/\${CPD_ELECTRON_OUT:-dist-electron.next}"
    mkdir -p "$out"
    printf 'next-main' > "$out/main.js"
    printf 'next-preload' > "$out/preload.cjs"
    printf 'next-server' > "$out/server.js"
    exit 0
  fi
done
exit 0
`;

/**
 * The fake `cpd`: the shared `e2e-fake-cpd` for every command, except `plan list` and
 * `attempt list`, which report one Plan with one running Attempt — the state that used to gate the
 * install (air-n81).
 */
const FAKE_CPD_WRAPPER = `#!/bin/sh
if [ "$1" = "plan" ] && [ "$2" = "list" ]; then
  printf '%s\\n' '{"ok":true,"events_cursor":0,"plans":[{"id":"p-running","context":"cpd","title":"E2E running plan","phase":"dispatch","archived_at":null,"created_at":"2026-01-01T00:00:00.000Z","lead_session":null,"workflows":[],"counts":{"draft":0,"queued":1,"running":1,"merged":0,"failed":0,"cancelled":0}}]}'
  exit 0
fi
if [ "$1" = "attempt" ] && [ "$2" = "list" ]; then
  printf '%s\\n' '{"ok":true,"attempts":[{"id":"a-running","plan":"p-running","status":"running"}]}'
  exit 0
fi
exec "${sharedFakeCpd}" "$@"
`;

/**
 * The shared `launchApp` helper, extended with the one seam this scenario needs: the staged
 * checkout (`CPD_CHECKOUT`) the app's auto-update runs against. The helper itself pins the hidden
 * window, the private `userData` dir and the Linux flags (air-d68).
 */
const test = base.extend<{ electronApp: unknown }>({
  electronApp: async ({ cpdHome, userDataDir, rendererServer }, use) => {
    const app = await launchApp({
      cpdHome,
      userDataDir,
      rendererUrl: rendererServer.url,
      // The staged checkout whose fake git/pnpm/install scripts make the update hermetic and whose
      // staged prod build dirs pin the air-d95 rotation. `HOME` is staged too, so the app never
      // reads the User's home and `ensureCli` finds an already-installed CLI (air-d68).
      env: { CPD_CHECKOUT: checkout, HOME: stagedHome },
    });
    await use(app);
    await app.close();
  },
});

test.beforeAll(() => {
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-update-nowait-"));
  checkout = join(staging, "checkout");
  stagedHome = join(staging, "home");
  fakeCpd = join(staging, "cpd");
  installMarker = join(staging, "install-ran");
  binaryMarker = join(staging, "install-binary-ran");
  mkdirSync(join(stagedHome, ".local", "bin"), { recursive: true });
  mkdirSync(join(checkout, "scripts"), { recursive: true });
  mkdirSync(join(checkout, ".git"), { recursive: true });
  mkdirSync(join(checkout, "target"), { recursive: true });
  mkdirSync(join(checkout, "web", "dist-electron"), { recursive: true });
  mkdirSync(join(checkout, "web", "dist-electron.head-1"), { recursive: true });

  // An already-installed CLI, so `ensureCli` is `ready` and never touches the User's home.
  writeFileSync(join(stagedHome, ".local", "bin", "cpd"), "#!/bin/sh\nexit 0\n");
  chmodSync(join(stagedHome, ".local", "bin", "cpd"), 0o755);

  // The checkout the updater resolves: `scripts/install-cpd.sh` and `.git` must exist.
  writeFileSync(join(checkout, "scripts", "install-cpd.sh"), FAKE_INSTALL);
  chmodSync(join(checkout, "scripts", "install-cpd.sh"), 0o755);
  writeFileSync(join(checkout, "scripts", "cargo.sh"), "#!/bin/sh\nexit 0\n");
  chmodSync(join(checkout, "scripts", "cargo.sh"), 0o755);

  // The current prod build, and the previous prod build already on disk (HEAD-1, air-d95).
  writeFileSync(join(checkout, "web", "dist-electron", "main.js"), CUR_MAIN);
  writeFileSync(join(checkout, "web", "dist-electron", "preload.cjs"), "cur-preload");
  writeFileSync(join(checkout, "web", "dist-electron", "server.js"), "cur-server");
  writeFileSync(join(checkout, "web", "dist-electron.head-1", "main.js"), PREV_MAIN);

  // The house fake-cpd-on-PATH setup, then the fake `git`/`pnpm` the update shells out to.
  const { bin } = stageFakeBin(staging, fakeCpd);
  writeFileSync(fakeCpd, FAKE_CPD_WRAPPER);
  chmodSync(fakeCpd, 0o755);
  writeFileSync(join(bin, "git"), FAKE_GIT);
  chmodSync(join(bin, "git"), 0o755);
  writeFileSync(join(bin, "pnpm"), FAKE_PNPM);
  chmodSync(join(bin, "pnpm"), 0o755);

  process.env["CPD_E2E_INSTALL_MARKER"] = installMarker;
  process.env["CPD_E2E_INSTALL_BINARY_MARKER"] = binaryMarker;

  if (!existsSync(join(webRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

/** The staging checkout's file, or `null` while the app's update is still writing it. */
function readCheckoutFile(...parts: string[]): string | null {
  try {
    return readFileSync(join(checkout, ...parts), "utf8");
  } catch {
    return null;
  }
}

test("the CLI update installs with a running Attempt and the previous prod build remains (air-n81, air-d95)", async ({ electronApp }) => {
  // The full install — the follow-up-carrying step air-n81 starved — runs while the fake cpd reports
  // one running Attempt: the update never waits for the Attempts to reach zero.
  await expect.poll(() => existsSync(installMarker), { timeout: 60_000 }).toBe(true);

  // The bundle swap landed the freshly built prod build...
  await expect.poll(() => readCheckoutFile("web", "dist-electron", "main.js"), { timeout: 60_000 }).toBe("next-main");

  // ...and the previous prod build remains on disk (air-d95): the displaced build is HEAD-1 and the
  // former HEAD-1 was demoted to HEAD-2, not deleted.
  expect(readCheckoutFile("web", "dist-electron.head-1", "main.js")).toBe(CUR_MAIN);
  expect(readCheckoutFile("web", "dist-electron.head-2", "main.js")).toBe(PREV_MAIN);

  // The update ran in the app's one main process: it is still up (the swap's restart-needed state
  // never relaunched it), so no second Electron and no orphan.
  expect(await electronApp.evaluate(({ app }) => app.isReady())).toBe(true);
});
