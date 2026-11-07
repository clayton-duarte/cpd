import { chmodSync, existsSync, readdirSync, statSync } from "node:fs";
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

/** Playwright globalSetup: runs once before any worker, before any Electron launch. */
export default function globalSetup(): void {
  makeSpawnHelpersExecutable();
}
