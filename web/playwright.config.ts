import { join } from "node:path";
import { tmpdir } from "node:os";
import { defineConfig } from "@playwright/test";

/**
 * Playwright config for the Electron end-to-end suite.
 *
 * The suite launches the real desktop app (the pinned Electron devDependency)
 * against the built `dist-electron/` bundle; see `src/testing/e2e-fixtures.ts`.
 * Electron needs no downloaded browser, so no Playwright browser install step
 * exists. One worker and zero retries keep runs deterministic (no flakes, no
 * retries): every result is either reproducible or a real defect.
 */
export default defineConfig({
  testDir: "./src/testing",
  testMatch: "e2e-*.spec.ts",
  outputDir: join(tmpdir(), "cpd-web-e2e"),
  // Once per run, before any worker: the app's pty host needs node-pty's `spawn-helper` executable
  // (e2e-setup.ts), which the frozen install does not guarantee.
  globalSetup: "./src/testing/e2e-setup.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: true,
  reporter: "list",
  timeout: 120_000,
});
