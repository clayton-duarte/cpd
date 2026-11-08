import { chmodSync, existsSync, readdirSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** The web/ package root (two levels up from web/src/testing/). */
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

/**
 * Make node-pty's `spawn-helper` executable before the suite launches Electron.
 *
 * The pinned install (pnpm-workspace.yaml sets `allowBuilds.node-pty = false`) can leave the
 * helper without its executable bit; Linux node-pty builds place it under `build/Release`, the
 * bundled prebuilds under `prebuilds/<platform>-<arch>/`. A non-executable helper makes the app's
 * pty host fail at boot. The `electron` dev script (web/package.json) does the same preparation for
 * a hand-run app; the e2e run must not depend on it.
 */
function makeSpawnHelpersExecutable(): void {
  const helpers = [join(webRoot, "node_modules", "node-pty", "build", "Release", "spawn-helper")];
  const prebuilds = join(webRoot, "node_modules", "node-pty", "prebuilds");
  if (existsSync(prebuilds)) {
    for (const platform of readdirSync(prebuilds)) {
      helpers.push(join(prebuilds, platform, "spawn-helper"));
    }
  }
  for (const helper of helpers) {
    if (existsSync(helper)) chmodSync(helper, statSync(helper).mode | 0o111);
  }
}

/**
 * Fail early, with a clear message, when node-pty's native module is missing.
 *
 * node-pty 1.1.0 ships Node-API prebuilds for darwin and win32 only, and pnpm-workspace.yaml
 * disables its build script (`allowBuilds.node-pty: false`), so on Linux there is no `pty.node`
 * unless something built it. The app imports node-pty at load, so without it no window is ever
 * created and the failure shows as a bare `firstWindow` timeout. The e2e job builds it (see
 * `.github/workflows/e2e.yml`, "node-pty native module"); this check turns a missing build into a
 * named error before any test runs.
 */
function assertNodePtyNative(): void {
  try {
    createRequire(import.meta.url)("node-pty");
  } catch (error) {
    throw new Error(
      `node-pty failed to load (${(error as Error).message}); build its native module before running the e2e suite`,
    );
  }
}

/** Playwright globalSetup: runs once before any worker, before any Electron launch. */
export default function globalSetup(): void {
  assertNodePtyNative();
  makeSpawnHelpersExecutable();
}
