// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the tree updates on a new Plan (guards air-n13/air-n15: tree frozen, new Plans missing).
//
// Bug air-n13/air-n15: the WebUI tree stopped updating, so new Plans never appeared without a full
// reload. This spec drives the fake cockpit to emit one `plan-created` event and asserts the tree
// shows the new Plan with no page reload. The fake answers `cpd cockpit serve` with an empty
// snapshot, then on the test's trigger emits the event and answers `cpd plan list` with the Plan —
// exactly the live path the freeze broke.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "./e2e-fixtures";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";
const PLAN_ID = "e2e-new-plan";

const staging = mkdtempSync(join(tmpdir(), "cpd-e2e-tree-"));
const fakeBin = join(staging, "bin");
const fakeCpd = join(staging, "cpd");
const fakeShell = join(staging, "shell");
const trigger = join(staging, "trigger");
mkdirSync(fakeBin, { recursive: true });
writeFileSync(
  fakeShell,
  `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`,
);
chmodSync(fakeShell, 0o755);
symlinkSync(fakeCpd, join(fakeBin, "cpd"));

// A scripted cpd: empty snapshot until the trigger file appears, then a `plan-created` event and a
// `plan list` that carries the new Plan. Self-contained so no real cpd is ever spawned.
writeFileSync(
  fakeCpd,
  `#!/usr/bin/env node
"use strict";
const fs = require("node:fs");
const PLAN_ID = ${JSON.stringify(PLAN_ID)};
const TRIGGER = process.env.CPD_E2E_TREE_TRIGGER;
const cursor = 0;
const emit = (v) => process.stdout.write(JSON.stringify(v) + "\\n");
const plan = () => ({ id: PLAN_ID, context: "cpd", title: "E2E tree plan", phase: "draft", archived_at: null, created_at: "2026-01-01T00:00:00.000Z", lead_session: null, workflows: [], counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 } });
const contextPane = () => ({ ok: true, events_cursor: cursor, contexts: [] });
const planPane = () => ({ ok: true, events_cursor: cursor, plans: fs.existsSync(TRIGGER) ? [plan()] : [] });
const sessionPane = () => ({ ok: true, events_cursor: cursor, sessions: [] });
const inboxPane = () => ({ ok: true, events_cursor: cursor, items: [], hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, engine: { running: false } });
const snapshot = (seq) => ({ t: "snapshot", seq, panes: { context: contextPane(), plan: planPane(), session: sessionPane(), inbox: inboxPane() } });
const event = (kind) => ({ ok: true, id: 1, kind, at: "2026-01-01T00:00:00.000Z", text: "", plan: PLAN_ID, context: "cpd", session: null, job: null, attempt: null, data: {} });
function planShow(id) {
  return { ok: true, events_cursor: cursor, plan: { ...plan(), id }, counts: plan().counts, jobs: [], workflows: [], review: null, escalations: [], reviewed_pr: null };
}
function readLines(onLine) {
  let buffered = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    buffered += chunk;
    let nl = buffered.indexOf("\\n");
    while (nl >= 0) {
      const line = buffered.slice(0, nl);
      buffered = buffered.slice(nl + 1);
      if (line.trim() !== "") onLine(line);
      nl = buffered.indexOf("\\n");
    }
  });
  process.stdin.on("end", () => {});
  process.stdin.on("error", () => {});
  process.stdin.resume();
}
function cockpitServe() {
  emit({ t: "hello", protocol: 1 });
  let seq = 1;
  emit(snapshot(seq));
  let fired = false;
  const fire = () => { if (fired) return; fired = true; seq += 1; emit(snapshot(seq)); };
  if (fs.existsSync(TRIGGER)) fire();
  else fs.watchFile(TRIGGER, { interval: 100 }, (curr) => { if (curr.mtimeMs > 0) fire(); });
  readLines((line) => {
    let req;
    try { req = JSON.parse(line); } catch { emit({ t: "error", id: null, error: { code: "bad_request", message: "not json" } }); return; }
    if (req.t === "refresh") { seq += 1; emit(snapshot(seq)); }
    else if (req.t === "plan_show") { emit({ t: "plan_show", id: typeof req.id === "string" ? req.id : null, value: planShow(typeof req.plan === "string" ? req.plan : "") }); }
    else emit({ t: "error", id: null, error: { code: "bad_request", message: "unknown" } });
  });
}
function stayAlive() {
  const timer = setInterval(() => {}, 1 << 30);
  const stop = () => { clearInterval(timer); fs.unwatchFile(TRIGGER); process.exit(0); };
  process.on("SIGTERM", stop);
  process.on("SIGINT", stop);
  process.stdout.on("error", stop);
}
function eventsFollow(args) {
  if (args.includes("--until-idle")) { emit(event("watch-started")); return; }
  let fired = false;
  const fire = () => { if (fired) return; fired = true; emit(event("plan-created")); };
  if (fs.existsSync(TRIGGER)) fire();
  else fs.watchFile(TRIGGER, { interval: 100 }, (curr) => { if (curr.mtimeMs > 0) fire(); });
  stayAlive();
}
const argv = process.argv.slice(2);
const command = argv[0];
const rest = argv.slice(1);
if (command === "cockpit" && rest[0] === "serve") cockpitServe();
else if (command === "events" && rest[0] === "follow") eventsFollow(rest);
else if (command === "plan" && rest[0] === "list") emit(planPane());
else if (command === "plan" && rest[0] === "show") {
  const at = rest.indexOf("--plan");
  emit(planShow(at >= 0 ? rest[at + 1] : rest[1] ?? ""));
} else if (command === "context" && rest[0] === "list") emit(contextPane());
else if (command === "session" && rest[0] === "list") emit(sessionPane());
else if (command === "inbox" && rest[0] === "list") emit(inboxPane());
else emit({ ok: true });
`,
);
chmodSync(fakeCpd, 0o755);

test.beforeAll(() => {
  process.env["CPD_BIN"] = fakeCpd;
  process.env["CPD_E2E_TREE_TRIGGER"] = trigger;
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
  if (!existsSync(join(repoRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: repoRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

test("the tree shows a new Plan delivered by a live event, with no reload (air-n13, air-n15)", async ({ page }) => {
  const root = page.locator("#webui-root");
  // The tree loaded against the fake's empty snapshot: the Plan is not there yet.
  await expect(root.locator('[data-part="tree-section"] [data-part="empty"]')).toHaveText("No contexts");
  await expect(page.locator(`[data-plan="${PLAN_ID}"]`)).toHaveCount(0);

  // A marker on the live window; a page reload would drop it.
  await page.evaluate(() => {
    (window as unknown as { __cpdNoReload?: boolean }).__cpdNoReload = true;
  });

  // Drive the fake: this triggers the `plan-created` event and the Plan in `cpd plan list`.
  writeFileSync(trigger, "go");

  await expect(page.locator(`[data-plan="${PLAN_ID}"]`)).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __cpdNoReload?: boolean }).__cpdNoReload)).toBe(true);
});
