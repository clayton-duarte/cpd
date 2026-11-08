// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the Workers segment and the incident pill read live serve-status data (decision D3,
// air-d87). The status path carries a `status` pane on the `cpd cockpit serve` snapshot (CONTRACT
// §20): the engine's Worker counts and incident state, off the serve stream. This spec boots the
// real Electron app against the scripted fake cpd and pins that the Workers segment shows the
// pane's live counts as the TUI text (long form, separator U+F0798), and -- in a second case --
// that the incident pill shows `Incident Mode: <elapsed>` while the fake reports an incident open.
// No fake tmux is staged: the point is that the WebUI no longer scrapes tmux for the Workers segment.
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchApp, stageFakeCpd, test as base, expect } from "./e2e-fixtures";

const webRoot = fileURLToPath(new URL("../..", import.meta.url));

/** The known counts the fake's serve status pane carries (`e2e-fake-cpd`). */
const WORKERS = { queued: 3, waiting: 2, running: 4, limit: 6 };
/**
 * The Workers segment's TUI text for those counts: `workers Q·R·N/M`, long form with the Nerd Font
 * separator U+F0798 (what the engine writes and the bar re-renders). `waiting` shows as the ready
 * count and `running` as the tenant count.
 */
const WORKERS_TEXT = `workers ${WORKERS.queued}\u{f0798}${WORKERS.waiting}\u{f0798}${WORKERS.running}/${WORKERS.limit}`;

let staging = "";
/** The file the fake's serve stream waits for before its first snapshot (CPD_FAKE_GATE). */
let gate = "";

/**
 * The shared `launchApp` helper, extended with the one seam this scenario needs: `incident` sets
 * `CPD_FAKE_INCIDENT`, which tells the fake's serve status pane to report an open incident. A case
 * selects it with `test.use({ incident: true })`. The shared fixture still pins the hidden window,
 * the private `userData` dir and the Linux flags (air-d68).
 */
const test = base.extend<{ incident: boolean }>({
  incident: [false, { option: true }],
  electronApp: async ({ cpdHome, userDataDir, rendererServer, incident }, use) => {
    const app = await launchApp({
      cpdHome,
      userDataDir,
      rendererUrl: rendererServer.url,
      env: { CPD_CHECKOUT: webRoot, CPD_FAKE_GATE: gate, ...(incident ? { CPD_FAKE_INCIDENT: "1" } : {}) },
    });
    await use(app);
    await app.close();
  },
});

test.beforeAll(() => {
  // The private data plane: a `cpd` symlink to the scripted fake, plus the fake login shell. No fake
  // tmux (the Workers segment no longer scrapes it).
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-workers-"));
  gate = join(staging, "serve-gate");
  stageFakeCpd(staging);
  if (!existsSync(join(webRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }
});

test.afterEach(() => {
  // Leave no gate behind: the next case's app must launch with the serve stream held.
  rmSync(gate, { force: true });
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

/**
 * Open the serve gate and wait for the pane's status to land. The app's initial status read (before
 * any serve pane) has already emitted when the mode pill first renders, so releasing the snapshot
 * only now makes the pane-triggered read the last write -- no last-writer-wins race.
 */
async function openGate(page: import("playwright").Page): Promise<void> {
  await expect(page.locator('[data-part="status-bar"]').locator('[data-part="mode"]')).toBeVisible();
  writeFileSync(gate, "");
}

test("the Workers segment shows the serve status pane's live counts (long form)", async ({ page }) => {
  await expect(page.locator("#webui-root")).toBeVisible();
  await openGate(page);
  // The pill renders only when the status carries workers. The fake answers one-shot lists empty and
  // carries the numbers only in its `cpd cockpit serve` snapshot's `status` pane, so the known text
  // can appear only once that pane reached the bar. Playwright's expect polls; no sleep, no retry.
  await expect(page.locator('[data-part="status-bar"]').locator('[data-part="workers"]')).toHaveText(WORKERS_TEXT);
});

test.describe("while the fake reports an incident open", () => {
  test.use({ incident: true });

  test("the incident pill shows Incident Mode with the elapsed time", async ({ page }) => {
    await expect(page.locator("#webui-root")).toBeVisible();
    await openGate(page);
    // started_at is now at snapshot time, so the elapsed text is 0m for the whole first minute.
    await expect(page.locator('[data-part="status-bar"]').locator('[data-part="incident"]')).toHaveText("Incident Mode: 0m");
  });
});
