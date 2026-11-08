// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: the WebUI loads with LIVE DATA — a non-empty Context tree, a non-empty Attention Inbox,
// the status bar and a selected session's transcript — on BOTH surfaces (air-d53, air-d68, air-d87;
// web/CONTRACT.md §20.2).
//
// The empty cockpit the User saw on 2026-11-07 (a painted shell, no tree, no inbox, no status bar,
// no transcript) came from a surface that never applied a snapshot: a dead transport, a start the
// ws gate let through too early, or a serve stream that never delivered. §20.2 pins the acceptance
// for both transports: the browser (the Vite dev server's WebUI over the same-origin `/cpd-ws`
// endpoint, web/CONTRACT.md §15.1) and cpd desktop (the Electron renderer over the main process's
// `/cpd-ws`). A surface on which any of the four stays empty after the first snapshot fails here,
// whatever the reason.
//
// The suite runs the real (hidden, isolated) Electron app against a scripted fake `cpd`
// (`CPD_ELECTRON_HIDDEN=1`, air-d68) — no window ever appears on the User's screen, and the per-run
// temp `CPD_HOME`/`--user-data-dir` keep the User's profile untouched. No test sleeps or retries:
// Playwright's `expect` polls.
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { expect, test } from "./e2e-fixtures";

const webRoot = fileURLToPath(new URL("../..", import.meta.url));
/** The PATH the runner started with, before any spec prepends its private bin dir. */
const basePath = process.env["PATH"] ?? "";

/** The non-empty state the fake serves: one Context, one Plan, its Lead session, one Inbox row. */
const CONTEXT_ID = "e2e-cockpit-ctx";
const PLAN_ID = "e2e-cockpit-plan";
const SESSION_ID = "e2e-cockpit-lead";
const TRANSCRIPT_TEXT = "live transcript body";

// A scripted cpd: every one-shot list AND the `cpd cockpit serve` snapshot carry the same non-empty
// state, so the tree/inbox can fill as soon as the first snapshot applies — the exact opposite of a
// healthy-but-empty WebUI. The Lead session's `file` points at a JSONL the spec wrote, so attaching
// it yields the transcript. Self-contained: no real cpd is ever spawned.
const FAKE_CPD_SOURCE = `#!/usr/bin/env node
"use strict";
const CONTEXT_ID = ${JSON.stringify(CONTEXT_ID)};
const PLAN_ID = ${JSON.stringify(PLAN_ID)};
const SESSION_ID = ${JSON.stringify(SESSION_ID)};
const SESSION_FILE = process.env.CPD_E2E_SESSION_FILE;
const cursor = 0;
const emit = (value) => { process.stdout.write(JSON.stringify(value) + "\\n"); };
const counts = () => ({ draft: 0, queued: 0, running: 1, merged: 0, failed: 0, cancelled: 0 });
const contextPane = () => ({ ok: true, events_cursor: cursor, contexts: [{ id: CONTEXT_ID, title: "E2E cockpit context", archived_at: null, records: 3, open_questions: 0 }] });
const planPane = () => ({ ok: true, events_cursor: cursor, plans: [{ id: PLAN_ID, context: CONTEXT_ID, title: "E2E cockpit plan", phase: "running", archived_at: null, created_at: "2026-01-01T00:00:00.000Z", counts: counts(), lead_session: SESSION_ID, workflows: [] }] });
const sessionPane = () => ({ ok: true, events_cursor: cursor, sessions: [{ id: SESSION_ID, runner: "pi", role: "lead", state: "idle", context: CONTEXT_ID, plan: PLAN_ID, plan_lead: true, archived_at: null, file: SESSION_FILE, tier: "G1", title: "E2E cockpit lead", cwd: null, key: null, name: null }] });
const inboxPane = () => ({ ok: true, events_cursor: cursor, items: [{ id: 1, kind: "review-requested", label: "Review requested", urgency: "yellow", opened_at: "2026-01-01T00:00:00.000Z", age_seconds: 0, context: CONTEXT_ID, plan: PLAN_ID, session: SESSION_ID, job: null, url: null, snoozed_until: null, dismissed_at: null, claimed_at: null, workflow: null }], hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, engine: { running: true } });
const snapshot = (seq) => ({ t: "snapshot", seq, panes: { context: contextPane(), plan: planPane(), session: sessionPane(), inbox: inboxPane() } });
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
    else if (request.t === "plan_show") emit({ t: "plan_show", id: typeof request.id === "string" ? request.id : null, value: { ok: true, events_cursor: cursor, plan: { id: typeof request.plan === "string" ? request.plan : PLAN_ID, context: CONTEXT_ID, title: "E2E cockpit plan", phase: "running", archived_at: null, lead_session: SESSION_ID }, counts: counts(), jobs: [], workflows: [], review: null, escalations: [], reviewed_pr: null } });
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
else if (command === "incident") emit({ ok: true, on: false });
else if (command === "attempt") emit({ ok: true, attempts: [] });
else if (command === "--version") emit({ ok: true, version: "0.0.0", protocol: 24 });
else emit({ ok: true });
`;

/** The Lead session's transcript, in pi's JSONL: one user turn and one assistant text turn. */
const SESSION_JSONL =
  [
    JSON.stringify({ type: "message", id: "u0", message: { role: "user", content: "hello from the e2e" } }),
    JSON.stringify({ type: "message", id: "a1", message: { role: "assistant", timestamp: 2, stopReason: "stop", model: "e2e/model", content: [{ type: "text", text: TRANSCRIPT_TEXT }] } }),
  ].join("\n") + "\n";

let staging = "";
let sessionFile = "";
/** The in-process Vite dev server that hosts the browser WebUI and its same-origin `/cpd-ws`. */
let vite: Awaited<ReturnType<typeof createServer>> | null = null;
let browserUrl = "";

/** The browser + desktop WebUI's mount points; both render the same `[data-shell]` cockpit. */
const shell = (page: import("playwright").Page) => page.locator("[data-shell]");

test.beforeAll(async () => {
  // A private bin dir with the scripted fake cpd as `cpd`, and a fake login shell reporting the
  // app's PATH unchanged (the app resolves the login PATH; the real shell would drop the fake dir).
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-cockpit-"));
  const fakeBin = join(staging, "bin");
  const fakeCpd = join(staging, "cpd");
  const fakeShell = join(staging, "shell");
  sessionFile = join(staging, "lead.jsonl");
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
  // The dev server runs the same boundaries the desktop build does; point it at its own temp home.
  process.env["CPD_HOME"] = join(staging, "home");
  if (!existsSync(join(webRoot, "dist-electron", "main.js"))) {
    execFileSync("pnpm", ["--filter", "cpd-web-client", "run", "build:electron"], { cwd: webRoot, stdio: "inherit" });
  }

  // The browser surface: the Vite dev server (web/vite.config.ts mounts `cpdWebPlugin`, which hosts
  // the same-origin `/cpd-ws` endpoint on the dev server's own httpServer, web/CONTRACT.md §15.1).
  vite = await createServer({
    root: webRoot,
    logLevel: "silent",
    server: { host: "127.0.0.1", port: 0, strictPort: false },
  });
  await vite.listen();
  const url = vite.resolvedUrls?.local?.[0];
  if (url === undefined || url === "") throw new Error("the Vite dev server did not report a local URL");
  browserUrl = url;
});

test.afterAll(async () => {
  // Tear down what this suite started: the dev server (its plugin stops the cockpit children) first,
  // then the private tree. Nothing is left running and nothing outside the worktree is touched.
  if (vite !== null) {
    await vite.close();
    vite = null;
  }
  process.env["PATH"] = basePath;
  rmSync(staging, { recursive: true, force: true });
});

/**
 * The four live-data assertions of web/CONTRACT.md §20.2, on whichever surface `page` shows: a
 * non-empty Context tree, a non-empty Attention Inbox, the status bar, and a selected session's
 * transcript. Playwright's `expect` polls; there is no sleep and no retry.
 */
async function assertLiveData(page: import("playwright").Page): Promise<void> {
  const root = shell(page);
  await expect(root).toBeVisible();

  // Tree: the fake's Context and its Plan rendered.
  await expect(root.locator(`[data-context="${CONTEXT_ID}"]`)).toBeVisible();
  await expect(root.locator(`[data-plan="${PLAN_ID}"]`)).toBeVisible();

  // Inbox: the fake's Attention row rendered.
  await expect(root.locator(`[data-part="inbox-panel"] [data-kind="review-requested"]`)).toBeVisible();

  // Status bar: present, and showing the machine's runner pill (a live read, `cpd config`).
  const bar = root.locator(`[data-part="status-bar"]`);
  await expect(bar).toBeVisible();
  await expect(bar.locator(`[data-part="runner"]`)).toHaveText("pi", { timeout: 60_000 });

  // Transcript: selecting the Plan selects its Lead session, whose JSONL the fake's `file` names.
  await root.locator(`[data-plan="${PLAN_ID}"] [data-select="plan"]`).click();
  await expect(root.locator(`[data-kind="assistant"]`)).toContainText(TRANSCRIPT_TEXT, { timeout: 30_000 });
}

test("the browser WebUI loads live data — tree, inbox, status bar and a transcript (air-d53, air-d87)", async ({ page }) => {
  // The browser build connects same-origin to the dev server's `/cpd-ws` (web/CONTRACT.md §15.1):
  // loading the Vite-served WebUI over http is the surface a User's browser uses.
  await page.goto(browserUrl, { waitUntil: "domcontentloaded" });
  await assertLiveData(page);
});

test("cpd desktop loads live data — tree, inbox, status bar and a transcript (air-d68)", async ({ page }) => {
  // The Electron renderer over the main process's `/cpd-ws` endpoint, hidden and isolated (air-d68).
  await assertLiveData(page);
});
