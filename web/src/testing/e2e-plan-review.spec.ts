// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: a pending Plan review fills the chat pane and scrolls (bug b75, issue #1247).
//
// PR #1184 meant the review to REPLACE the chat view edge to edge, with the header and the actions
// always reachable and one scroll region in the middle. jsdom does no layout, so its tests only
// checked class presence while the real Chromium layout still showed a bordered card whose Jobs
// table was cut off. This spec drives the real (hidden, isolated) Electron app against a scripted
// fake cpd, selects a Lead session whose pi JSONL carries a pending `cpd-plan-review` entry (8 Jobs
// across 2 Workflows in the summary), and asserts on computed layout: the section is the pane below
// its header (within 1px), has no border of its own and none of its ancestors up to the pane do, the
// body scroller overflows and reaches the last Job row, and approve/reject stay in the viewport.
// No real cpd or tmux, a per-run temp CPD_HOME, no sleep: Playwright's `expect` polls.
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { expect, test as base, webRoot as fixturesWebRoot } from "./e2e-fixtures";

const webRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
// The WebSocket endpoint accepts only a `file://` page or an http origin on port 3333/33333
// (web/src/server/ws.ts acceptsOrigin), and the shared fixture serves the renderer on an ephemeral
// port, so it would be refused and the cockpit would stay empty. Load the built renderer from disk,
// the way the production renderer is loaded (its assets are relative), instead of over http.
const test = base.extend({
  rendererServer: async ({}, use) => {
    await use({ url: pathToFileURL(join(fixturesWebRoot, "dist-electron", "renderer", "electron.html")).href, close: async () => {} });
  },
});

const basePath = process.env["PATH"] ?? "";

const CONTEXT_ID = "e2e-review-ctx";
const PLAN_ID = "e2e-review-plan";
const SESSION_ID = "e2e-review-lead";

// A scripted cpd serving one Lead session whose `file` is the JSONL the spec wrote.
const FAKE_CPD_SOURCE = `#!/usr/bin/env node
"use strict";
const CONTEXT_ID = ${JSON.stringify(CONTEXT_ID)};
const PLAN_ID = ${JSON.stringify(PLAN_ID)};
const SESSION_ID = ${JSON.stringify(SESSION_ID)};
const SESSION_FILE = process.env.CPD_E2E_SESSION_FILE;
const cursor = 0;
const emit = (value) => { process.stdout.write(JSON.stringify(value) + "\\n"); };
const counts = () => ({ draft: 0, queued: 0, running: 1, merged: 0, failed: 0, cancelled: 0 });
const contextPane = () => ({ ok: true, events_cursor: cursor, contexts: [{ id: CONTEXT_ID, title: "E2E plan review context", archived_at: null, records: 3, open_questions: 0 }] });
const planPane = () => ({ ok: true, events_cursor: cursor, plans: [{ id: PLAN_ID, context: CONTEXT_ID, title: "E2E plan review plan", phase: "running", archived_at: null, created_at: "2026-01-01T00:00:00.000Z", counts: counts(), lead_session: SESSION_ID, workflows: [] }] });
const sessionPane = () => ({ ok: true, events_cursor: cursor, sessions: [{ id: SESSION_ID, runner: "pi", role: "lead", state: "idle", context: CONTEXT_ID, plan: PLAN_ID, plan_lead: true, archived_at: null, file: SESSION_FILE, tier: "G1", title: "E2E plan review lead", cwd: null, key: null, name: null }] });
const inboxPane = () => ({ ok: true, events_cursor: cursor, items: [{ id: 1, kind: "review-requested", label: "Review requested", urgency: "yellow", opened_at: "2026-01-01T00:00:00.000Z", age_seconds: 0, context: CONTEXT_ID, plan: PLAN_ID, session: SESSION_ID, job: null, url: null, snoozed_until: null, dismissed_at: null, claimed_at: null, workflow: null }], hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, engine: { running: true } });
const statusPane = () => ({ ok: true, workers: { queued: 0, waiting: 0, running: 0, limit: 6 }, incident: { on: false, started_at: null, lead: null } });
const snapshot = (seq) => ({ t: "snapshot", seq, panes: { context: contextPane(), plan: planPane(), session: sessionPane(), inbox: inboxPane(), status: statusPane() } });
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
    else if (request.t === "plan_show") emit({ t: "plan_show", id: typeof request.id === "string" ? request.id : null, value: { ok: true, events_cursor: cursor, plan: { id: typeof request.plan === "string" ? request.plan : PLAN_ID, context: CONTEXT_ID, title: "E2E plan review plan", phase: "running", archived_at: null, lead_session: SESSION_ID }, counts: counts(), jobs: [], workflows: [], review: null, escalations: [], reviewed_pr: null } });
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
const argv = process.argv.slice(2);
const command = argv[0];
const rest = argv.slice(1);
if (command === "cockpit" && rest[0] === "serve") cockpitServe();
else if (command === "context" && rest[0] === "list") emit(contextPane());
else if (command === "plan" && rest[0] === "list") emit(planPane());
else if (command === "session" && rest[0] === "list") emit(sessionPane());
else if (command === "session" && rest[0] === "show") emit({ ok: true, session: { id: SESSION_ID, model: "e2e/model", thinking: "low", tier: "G1", file: SESSION_FILE } });
else if (command === "inbox" && rest[0] === "list") emit(inboxPane());
else if (command === "events" && rest[0] === "follow") { emit({ ok: true, id: 1, kind: "watch-started", at: "1970-01-01T00:00:00.000Z", text: "", plan: null, context: null, session: null, job: null, attempt: null, data: {} }); if (!rest.includes("--until-idle")) stayAlive(); }
else if (command === "config" && rest.length === 0) emit({ ok: true, config: { context_distill_tier: "G1", worker_tier: "G1", lead_tier: "G1", manager_tier: "G1", experimental: { durable_workers: false } } });
else if (command === "config" && rest[0] === "show") emit({ ok: true, config: { experimental: { durable_workers: false } } });
else if (command === "events" && rest[0] === "list") emit({ ok: true, events: [] });
else if (command === "resource" && rest[0] === "list") emit({ ok: true, resources: [] });
else if (command === "incident") emit({ ok: true, on: false });
else if (command === "attempt") emit({ ok: true, attempts: [] });
else if (command === "--version") emit({ ok: true, version: "0.0.0", protocol: 24 });
else emit({ ok: true });
`;

/** The Plan summary: 2 Workflows, 8 Jobs (a table row and a detail paragraph each), overlaps and risks. */
const JOBS = Array.from({ length: 8 }, (_, i) => ({ name: `job-${String(i + 1).padStart(2, "0")}`, workflow: i < 4 ? "wf-alpha" : "wf-beta" }));
const LAST_JOB = JOBS[JOBS.length - 1].name;
const table = (wf) => ["| Job | Role | Tier |", "| --- | --- | --- |", ...JOBS.filter((j) => j.workflow === wf).map((j) => `| ${j.name} | worker | S1 |`)].join("\n");
// The Jobs tables come last, so scrolling to the bottom must reveal the last Job row.
const SUMMARY = [
  "## Goal",
  "Ship the thing in two Workflows.",
  "## Risks",
  "- Overlap between wf-alpha and wf-beta on shared files.",
  ...["wf-alpha", "wf-beta"].flatMap((wf) => [
    `## Workflow ${wf}`,
    ...JOBS.filter((j) => j.workflow === wf).flatMap((j) => [
      `### ${j.name} details`,
      "A paragraph of detail long enough to take real vertical space in the review body, so the whole summary is taller than the window. ".repeat(3),
    ]),
    table(wf),
  ]),
].join("\n\n");

const SESSION_JSONL =
  [
    JSON.stringify({ type: "message", id: "u0", message: { role: "user", content: "plan it" } }),
    JSON.stringify({ type: "custom", id: "r1", customType: "cpd-plan-review", data: { review: "r1", status: "pending", title: "E2E layout plan", summary: SUMMARY } }),
  ].join("\n") + "\n";

let staging = "";

test.beforeAll(() => {
  // A private bin dir with the scripted fake cpd as `cpd`, and a fake login shell reporting the
  // app's PATH unchanged (the real one would drop the fake dir).
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-plan-review-"));
  const fakeBin = join(staging, "bin");
  const fakeCpd = join(staging, "cpd");
  const fakeShell = join(staging, "shell");
  const sessionFile = join(staging, "lead.jsonl");
  mkdirSync(fakeBin, { recursive: true });
  writeFileSync(fakeShell, `#!/bin/sh\nprintf '%s' '__CPD_PATH_BEGIN__'; printf '%s' "$PATH"; printf '%s' '__CPD_PATH_END__'\n`);
  chmodSync(fakeShell, 0o755);
  writeFileSync(fakeCpd, FAKE_CPD_SOURCE);
  chmodSync(fakeCpd, 0o755);
  symlinkSync(fakeCpd, join(fakeBin, "cpd"));
  writeFileSync(sessionFile, SESSION_JSONL);
  process.env["CPD_BIN"] = fakeCpd;
  process.env["CPD_E2E_SESSION_FILE"] = sessionFile;
  process.env["SHELL"] = fakeShell;
  process.env["PATH"] = `${fakeBin}:${basePath}`;
  if (!existsSync(join(webRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }
});

test.afterAll(() => {
  process.env["PATH"] = basePath;
  rmSync(staging, { recursive: true, force: true });
});

test("a pending Plan review fills the pane edge to edge, has no inner border, and scrolls with its actions reachable (b75)", async ({ page }) => {
  const root = page.locator("[data-shell]");
  await expect(root).toBeVisible();
  await root.locator(`[data-plan="${PLAN_ID}"] [data-select="plan"]`).click();
  const review = page.locator('[data-dialog="cpd_plan_review"]');
  await expect(review).toBeVisible({ timeout: 30_000 });
  const approve = review.locator('[data-action="approve"]');
  const reject = review.locator('[data-action="reject"]');
  await expect(approve).toBeVisible();

  // 1. Edge to edge: the section is the pane below the chat header, within 1px.
  const geometry = await review.evaluate((section) => {
    const pane = section.closest("[data-chat-dropzone]");
    const box = (el) => { const r = el.getBoundingClientRect(); return { top: r.top, left: r.left, right: r.right, bottom: r.bottom }; };
    const header = pane.firstElementChild;
    const between = [];
    for (let el = section.parentElement; el !== null && el !== pane; el = el.parentElement) between.push(parseFloat(getComputedStyle(el).borderTopWidth) + parseFloat(getComputedStyle(el).borderRightWidth) + parseFloat(getComputedStyle(el).borderBottomWidth) + parseFloat(getComputedStyle(el).borderLeftWidth));
    const cs = getComputedStyle(section);
    return {
      pane: box(pane),
      header: box(header),
      section: box(section),
      sectionBorder: [cs.borderTopWidth, cs.borderRightWidth, cs.borderBottomWidth, cs.borderLeftWidth],
      sectionMargin: [cs.marginTop, cs.marginRight, cs.marginBottom, cs.marginLeft],
      sectionRadius: cs.borderTopLeftRadius,
      ancestorBorders: between,
    };
  });
  expect.soft(Math.abs(geometry.section.left - geometry.pane.left)).toBeLessThanOrEqual(1);
  expect.soft(Math.abs(geometry.section.right - geometry.pane.right)).toBeLessThanOrEqual(1);
  expect.soft(Math.abs(geometry.section.bottom - geometry.pane.bottom)).toBeLessThanOrEqual(1);
  expect.soft(Math.abs(geometry.section.top - geometry.header.bottom)).toBeLessThanOrEqual(1);

  // 2. One border: the section has none (no margin, no rounding either) and no ancestor up to the pane does.
  expect.soft(geometry.sectionBorder).toEqual(["0px", "0px", "0px", "0px"]);
  expect.soft(geometry.sectionMargin).toEqual(["0px", "0px", "0px", "0px"]);
  expect.soft(geometry.sectionRadius).toBe("0px");
  expect.soft(geometry.ancestorBorders.every((w) => w === 0)).toBe(true);

  // 3. The body scroller overflows, and scrolling it to the bottom reveals the last Job row.
  const scroller = await review.locator('[data-part="body"]').evaluateHandle((body) => {
    for (let el = body.parentElement; el !== null; el = el.parentElement) {
      if (/(auto|scroll)/.test(getComputedStyle(el).overflowY) && el.scrollHeight > el.clientHeight) return el;
    }
    return null;
  });
  expect(await scroller.evaluate((el) => el !== null)).toBe(true);
  const metrics = await scroller.evaluate((el) => ({ scrollHeight: el.scrollHeight, clientHeight: el.clientHeight }));
  expect.soft(metrics.scrollHeight).toBeGreaterThan(metrics.clientHeight);

  // 4. Approve and reject stay inside the viewport and visible, before and after scrolling.
  const reachable = async () => {
    const viewport = await page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight }));
    for (const button of [approve, reject]) {
      await expect(button).toBeVisible();
      const b = await button.boundingBox();
      expect(b.x).toBeGreaterThanOrEqual(0);
      expect(b.y).toBeGreaterThanOrEqual(0);
      expect(b.x + b.width).toBeLessThanOrEqual(viewport.width);
      expect(b.y + b.height).toBeLessThanOrEqual(viewport.height);
    }
  };
  await reachable();
  await scroller.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  const lastRow = review.getByRole("cell", { name: LAST_JOB, exact: true });
  await expect(lastRow).toBeInViewport();
  const lastBox = await lastRow.boundingBox();
  const paneBox = geometry.pane;
  expect(lastBox.y + lastBox.height).toBeLessThanOrEqual(paneBox.bottom + 1);
  await reachable();
});
