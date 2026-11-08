// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: boot renders the desktop window AND the fake snapshot's data (guards air-n31 + the
// healthy-but-empty WebUI defect).
//
// Bug air-n31: after an auto-update, the renderer was newer than the running main process; the
// renderer's startup IPC called `pty:hosts`, the old main.js had no handler, and the window came
// back blank.
//
// webui-empty (2026-11-07 18:11 EST): the renderer's Origin file:// was rejected by ws.ts
// acceptsOrigin, so no cockpit snapshot frame arrived, yet `health:ok` still fired on mount and
// `$CPD_HOME/run/preview-health.json` read healthy -- a healthy-but-zero-frames WebUI.
//
// This spec boots the real Electron app against a scripted fake cpd and pins both contracts:
//  - the renderer root renders visible content, with no missing-IPC-handler page error (air-n31);
//  - the tree actually rendered the fake snapshot's rows (a positive data assertion), before the
//    test treats health as green, so a WebUI that reports healthy but renders zero snapshot frames
//    fails here.
// The fake answers every one-shot tree list empty and carries the known rows ONLY in its
// `cpd cockpit serve` snapshot: the rows can appear only when that snapshot frame is applied.
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "./e2e-fixtures";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

/** The Context and Plan the fake's cockpit snapshot carries; the tree must render both. */
const CONTEXT_ID = "e2e-boot-ctx";
const PLAN_ID = "e2e-boot-plan";

// A scripted cpd: every one-shot list answers empty and the tree's rows live only in the
// `cpd cockpit serve` snapshot. Self-contained, so no real cpd is ever spawned.
const FAKE_CPD_SOURCE = `#!/usr/bin/env node
"use strict";
const CONTEXT_ID = ${JSON.stringify(CONTEXT_ID)};
const PLAN_ID = ${JSON.stringify(PLAN_ID)};
const cursor = 0;
const emit = (value) => { process.stdout.write(JSON.stringify(value) + "\\n"); };
const planRow = () => ({
  id: PLAN_ID, context: CONTEXT_ID, title: "E2E boot plan", phase: "draft",
  archived_at: null, created_at: "2026-01-01T00:00:00.000Z", lead_session: null, workflows: [],
  counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
});
const contextRow = () => ({ id: CONTEXT_ID, title: "E2E boot context", archived_at: null, records: 0, open_questions: 0 });
const inboxPane = () => ({ ok: true, events_cursor: cursor, items: [], hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, engine: { running: false } });
// The one-shot list commands answer empty: the tree can fill ONLY from the cockpit snapshot frame.
const contextList = () => ({ ok: true, events_cursor: cursor, contexts: [] });
const planList = () => ({ ok: true, events_cursor: cursor, plans: [] });
const sessionList = () => ({ ok: true, events_cursor: cursor, sessions: [] });
// The snapshot carries the known rows.
const snapshot = (seq) => ({
  t: "snapshot", seq,
  panes: {
    context: { ok: true, events_cursor: cursor, contexts: [contextRow()] },
    plan: { ok: true, events_cursor: cursor, plans: [planRow()] },
    session: sessionList(),
    inbox: inboxPane(),
  },
});
function planShow(id) {
  return {
    ok: true, events_cursor: cursor,
    plan: { id, context: CONTEXT_ID, title: "E2E boot plan", phase: "draft", archived_at: null, lead_session: null },
    counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
    jobs: [], workflows: [], review: null, escalations: [], reviewed_pr: null,
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
  emit(snapshot(1));
  readLines((line) => {
    let request;
    try { request = JSON.parse(line); } catch { emit({ t: "error", id: null, error: { code: "bad_request", message: "not json" } }); return; }
    if (request.t === "refresh") emit(snapshot(2));
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
function eventsCommand(args) {
  if (args[0] === "follow" && args.includes("--until-idle")) {
    emit({ ok: true, id: 1, kind: "watch-started", at: "1970-01-01T00:00:00.000Z", text: "", plan: null, context: null, session: null, job: null, attempt: null, data: {} });
    return;
  }
  // A live follow must not emit events: the tree's rows come from the snapshot alone. It just stays up.
  if (args[0] === "follow") { stayAlive(); return; }
  emit({ ok: true, events: [] });
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
function tuiCommand() {
  process.stdout.write("cpd tui --attach (fake)\\n");
  stayAlive();
}
const argv = process.argv.slice(2);
const command = argv[0];
const rest = argv.slice(1);
switch (command) {
  case "cockpit": if (rest[0] === "serve") cockpitServe(); else emit({ ok: true }); break;
  case "events": eventsCommand(rest); break;
  case "context":
    if (rest[0] === "list") emit(contextList());
    else if (rest[0] === "curation") emit({ ok: true, retire: [] });
    else emit({ ok: true });
    break;
  case "plan":
    if (rest[0] === "list") emit(planList());
    else if (rest[0] === "show") { const at = rest.indexOf("--plan"); emit(planShow(at >= 0 ? rest[at + 1] : rest[1] ?? "")); }
    else emit({ ok: true });
    break;
  case "session": if (rest[0] === "list") emit(sessionList()); else emit({ ok: true }); break;
  case "inbox": if (rest[0] === "list") emit(inboxPane()); else emit({ ok: true }); break;
  case "config": configCommand(rest); break;
  case "attempt": emit({ ok: true, attempts: [] }); break;
  case "incident": emit({ ok: true, on: false }); break;
  case "tui": tuiCommand(); break;
  case "--version": emit({ ok: true, version: "0.0.0", protocol: 24 }); break;
  default: emit({ ok: true });
}
`;

let staging = "";

/** The app's health record, or `null` when it is missing or malformed. */
function readHealth(cpdHome: string): { status?: string } | null {
  try {
    return JSON.parse(readFileSync(join(cpdHome, "run", "preview-health.json"), "utf8"));
  } catch {
    return null;
  }
}

test.beforeAll(() => {
  // A private bin dir with the scripted fake cpd as `cpd`, and a fake login shell that reports the
  // app's PATH unchanged. `resolveLoginPath` runs `$SHELL -ilc`; the real one would drop the fake
  // dir, so the pty (`cpd tui --attach`) would spawn a real cpd. The fake shell keeps the fake first.
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-boot-"));
  const fakeBin = join(staging, "bin");
  const fakeCpd = join(staging, "cpd");
  const fakeShell = join(staging, "shell");
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
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
  if (!existsSync(join(repoRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: repoRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  rmSync(staging, { recursive: true, force: true });
});

test("boot renders the snapshot's rows before health is green, with no missing IPC handler error (air-n31, webui-empty)", async ({ cpdHome, electronApp, page }) => {
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

  // The positive data assertion: the tree rendered the fake snapshot's known Context and Plan.
  // The fake answers every one-shot tree list empty and carries these rows only in its
  // `cpd cockpit serve` snapshot, so they can appear only once that snapshot frame was applied.
  // A healthy-but-zero-frames WebUI (the webui-empty defect) renders no rows and fails right here,
  // before the health assertion below. Playwright's expect polls; there is no sleep and no retry.
  await expect(root.locator(`[data-context="${CONTEXT_ID}"]`)).toBeVisible();
  await expect(root.locator(`[data-plan="${PLAN_ID}"]`)).toBeVisible();
  await expect(root.locator(`[data-part="plan-id"]`)).toHaveText(PLAN_ID);

  // Health is green (the renderer's post-mount `health:ok`): a boot that reports healthy while
  // rendering zero snapshot frames can never satisfy the row assertions above.
  await expect.poll(() => readHealth(cpdHome)?.status ?? null, { timeout: 30_000 }).toBe("healthy");

  // The startup IPC resolved against the real main process: no page error about a missing handler.
  const missingHandler = /No handler registered|pty:hosts/i;
  expect(pageErrors.filter((e) => missingHandler.test(e))).toEqual([]);
});
