// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: create a Plan from the tree (a Context row's "New Plan…" menu item) and see its Lead
// appear in the tree (guards pro-d46: the tree row context menus, and the plan.new reply path).
//
// The WebUI's Context row menu offers "New Plan…" (ContextTree.tsx newPlanItem), which opens a
// prompt dialog and, on OK, dispatches `{ kind: "plan.new", ... }`. The cockpit runs
// `cpd plan new <id> --context=<ctx> --title=<title> --by-user`, then refreshes the tree; the reply
// names the new Plan and its Lead session, and the renderer selects and attaches that Lead (connection
// selectCreated). This spec drives that whole path through a scripted fake cpd: right-click a Context
// row, choose "New Plan…", type a title, confirm, then assert the new Plan row is in the tree, is the
// selected row, and carries its Lead's Dot (the Lead session read into the tree).
//
// The suite is hidden and isolated (air-d68: no window on the User's screen), points the app at the
// built renderer with a per-run temp `CPD_HOME`, and drives a scripted fake cpd, so no real cpd or
// tmux is ever spawned and nothing leaks.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "./e2e-fixtures";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

/** The Context the tree starts with, and the Plan the menu creates from it. */
const CONTEXT_ID = "e2e-ctx";
const PLAN_ID = "e2e-lead-plan";

// A scripted cpd: a fixed Context row, and empty Plans/Sessions until `cpd plan new` writes the new
// Plan and its Lead to a state file every read (and the `cockpit serve` snapshot) then answers from.
// Self-contained, so no real cpd is ever spawned.
const FAKE_CPD_SOURCE = `#!/usr/bin/env node
"use strict";
const fs = require("node:fs");
const CONTEXT_ID = ${JSON.stringify(CONTEXT_ID)};
const STATE = process.env.CPD_E2E_MENU_STATE;
const cursor = 0;
const emit = (v) => process.stdout.write(JSON.stringify(v) + "\\n");
const empty = () => ({ plan: null, lead: null });
const readState = () => { try { return JSON.parse(fs.readFileSync(STATE, "utf8")); } catch { return empty(); } };
const writeState = (s) => fs.writeFileSync(STATE, JSON.stringify(s));
const counts = () => ({ draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 });
const contextRow = () => ({ id: CONTEXT_ID, title: "E2E menu context", archived_at: null, records: 0, open_questions: 0 });
const contextPane = () => ({ ok: true, events_cursor: cursor, contexts: [contextRow()] });
const planPane = () => {
  const s = readState();
  const plans = s.plan === null ? [] : [{
    id: s.plan.id, context: CONTEXT_ID, title: s.plan.title, phase: "draft", archived_at: null,
    created_at: "2026-01-01T00:00:00.000Z", lead_session: s.lead === null ? null : s.lead.id, workflows: [], counts: counts(),
  }];
  return { ok: true, events_cursor: cursor, plans };
};
const sessionPane = () => {
  const s = readState();
  const sessions = s.plan === null ? [] : [{
    id: s.lead.id, runner: "pi", role: "lead", state: "waiting", context: CONTEXT_ID, plan: s.plan.id,
    plan_lead: true, archived_at: null, file: null, tier: null, title: null, cwd: null, key: null, name: null,
  }];
  return { ok: true, events_cursor: cursor, sessions };
};
const inboxPane = () => ({ ok: true, events_cursor: cursor, items: [], hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, engine: { running: false } });
const snapshot = (seq) => ({ t: "snapshot", seq, panes: { context: contextPane(), plan: planPane(), session: sessionPane(), inbox: inboxPane() } });
function planShow(id) {
  const s = readState();
  return {
    ok: true, events_cursor: cursor,
    plan: { id, context: CONTEXT_ID, title: s.plan === null ? "E2E lead plan" : s.plan.title, phase: "draft", archived_at: null, lead_session: s.lead === null ? null : s.lead.id },
    counts: counts(), jobs: [], workflows: [], review: null, escalations: [], reviewed_pr: null,
  };
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
  readLines((line) => {
    let request;
    try { request = JSON.parse(line); } catch { emit({ t: "error", id: null, error: { code: "bad_request", message: "not json" } }); return; }
    if (request.t === "refresh") { seq += 1; emit(snapshot(seq)); }
    else if (request.t === "plan_show") emit({ t: "plan_show", id: typeof request.id === "string" ? request.id : null, value: planShow(typeof request.plan === "string" ? request.plan : "") });
    else emit({ t: "error", id: null, error: { code: "bad_request", message: "unknown" } });
  });
}
function stayAlive() {
  const timer = setInterval(() => {}, 1 << 30);
  const stop = () => { clearInterval(timer); process.exit(0); };
  process.on("SIGTERM", stop);
  process.on("SIGINT", stop);
  process.stdout.on("error", stop);
}
function planNew(args) {
  const planId = args[0];
  const contextId = args.find((a) => a.startsWith("--context="));
  const title = args.find((a) => a.startsWith("--title="));
  const leadId = planId + "-lead";
  writeState({ plan: { id: planId, context: contextId === undefined ? CONTEXT_ID : contextId.slice(10), title: title === undefined ? planId : title.slice(8) }, lead: { id: leadId } });
  emit({ ok: true, plan: { id: planId }, lead: { session: leadId } });
}
function configCommand(args) {
  if (args[0] === "experimental" && args.length === 1) {
    emit({ ok: true, experimental: { durable_workers: false, write_curation: false, failure_triage: false, turn_question_guard: false } });
    return;
  }
  if (args[0] === "show") { emit({ ok: true, config: { experimental: { durable_workers: false } } }); return; }
  if (args.length === 0) { emit({ ok: true, config: { context_distill_tier: "G1", worker_tier: "G1", lead_tier: "G1", manager_tier: "G1" } }); return; }
  emit({ ok: true });
}
const argv = process.argv.slice(2);
const command = argv[0];
const rest = argv.slice(1);
switch (command) {
  case "cockpit": if (rest[0] === "serve") cockpitServe(); else emit({ ok: true }); break;
  case "plan":
    if (rest[0] === "list") emit(planPane());
    else if (rest[0] === "new") planNew(rest.slice(1));
    else if (rest[0] === "show") { const at = rest.indexOf("--plan"); emit(planShow(at >= 0 ? rest[at + 1] : rest[1] ?? "")); }
    else emit({ ok: true });
    break;
  case "context": if (rest[0] === "list") emit(contextPane()); else emit({ ok: true }); break;
  case "session": if (rest[0] === "list") emit(sessionPane()); else emit({ ok: true }); break;
  case "inbox": if (rest[0] === "list") emit(inboxPane()); else emit({ ok: true }); break;
  case "config": configCommand(rest); break;
  case "events":
    if (rest[0] === "follow" && rest.includes("--until-idle")) emit({ ok: true, id: 1, kind: "watch-started", at: "1970-01-01T00:00:00.000Z", text: "", plan: null, context: null, session: null, job: null, attempt: null, data: {} });
    else if (rest[0] === "follow") stayAlive();
    else emit({ ok: true, events: [] });
    break;
  case "attempt": emit({ ok: true, attempts: [] }); break;
  case "incident": emit({ ok: true, on: false }); break;
  case "--version": emit({ ok: true, version: "0.0.0", protocol: 24 }); break;
  default: emit({ ok: true });
}
`;

let staging = "";

test.beforeAll(() => {
  // A private bin dir with the scripted fake cpd as `cpd`, and a fake login shell that reports the
  // app's PATH unchanged. `resolveLoginPath` runs `$SHELL -ilc`; the real one would drop the fake
  // dir, so the pty (`cpd tui --attach`) would spawn a real cpd. The fake shell keeps the fake first.
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-tree-menu-"));
  const fakeBin = join(staging, "bin");
  const fakeCpd = join(staging, "cpd");
  const fakeShell = join(staging, "shell");
  const state = join(staging, "state.json");
  mkdirSync(fakeBin, { recursive: true });
  writeFileSync(
    fakeShell,
    `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`,
  );
  chmodSync(fakeShell, 0o755);
  writeFileSync(fakeCpd, FAKE_CPD_SOURCE);
  chmodSync(fakeCpd, 0o755);
  symlinkSync(fakeCpd, join(fakeBin, "cpd"));
  process.env["CPD_BIN"] = fakeCpd;
  process.env["CPD_E2E_MENU_STATE"] = state;
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
  if (!existsSync(join(repoRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: repoRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

test("creating a Plan from a Context row's menu shows the new Plan's Lead in the tree (pro-d46)", async ({ electronApp, page }) => {
  // Hidden + isolated launch (air-d68): no window is visible on screen, and the app's own userData
  // lives in the per-run temp dir, never the User's real profile or lock.
  const launch = await electronApp.evaluate(({ app, BrowserWindow }) => ({
    visible: BrowserWindow.getAllWindows().map((w) => w.isVisible()),
    userData: app.getPath("userData"),
  }));
  expect(launch.visible).not.toContain(true);
  expect(launch.userData).toContain("cpd-e2e-userdata-");

  const root = page.locator("#webui-root");
  await expect(root).toBeVisible();
  await expect(root.locator("[data-shell]")).toBeVisible();

  // The tree loaded the fake's Context, and the Plan does not exist yet.
  const contextRow = root.locator(`[data-context="${CONTEXT_ID}"] > button[data-select="context"]`);
  await expect(contextRow).toBeVisible();
  await expect(root.locator(`[data-plan="${PLAN_ID}"]`)).toHaveCount(0);

  // Create the Plan the way a User does: right-click the Context row, choose "New Plan…".
  await contextRow.click({ button: "right" });
  const menu = page.locator('[data-slot="context-menu-content"]');
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "New Plan…" })).toBeVisible();
  await menu.getByRole("menuitem", { name: "New Plan…" }).click();

  // The prompt dialog: type a title and confirm.
  const dialog = page.locator('[data-slot="dialog-content"]');
  await expect(dialog).toBeVisible();
  await dialog.locator('input[aria-label="Title:"]').fill("E2E lead plan");
  await dialog.getByRole("button", { name: "OK" }).click();

  // The new Plan appears in the tree under its Context, and it is the selected row (the renderer
  // selected the Plan the reply named).
  const planRow = root.locator(`[data-context="${CONTEXT_ID}"] [data-plan="${PLAN_ID}"]`);
  await expect(planRow).toBeVisible();
  const planButton = planRow.locator('button[data-select="plan"]');
  await expect(planButton).toHaveAttribute("aria-current", "true");

  // Its Lead session was read into the tree: the Plan row carries the Lead's Dot (the fake's Lead is
  // "waiting", distinct from the "stopped" fallback a Plan with no Lead renders).
  await expect(planButton.locator('[data-dot]')).toHaveAttribute("data-dot", "waiting");
  await expect(planButton.locator('[data-part="plan-id"]')).toHaveText(PLAN_ID);
});
