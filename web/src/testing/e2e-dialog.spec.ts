// @ts-nocheck -- Playwright's Electron types are resolved through the web package; kept loose here.
// Scenario: opening the Agent Tiers dialog must not blank the WebUI, and the WebUI must have no
// hidden tab-bar offset (guards air-n74 and air-n68).
//
// Bug air-n74: when the app still used the Radix primitives, every modal scroll-locked the page,
// setting `body[data-scroll-locked]{position:relative!important}`. `electron.html` laid the WebUI out as
// `#webui-root{position:absolute;top:30px;bottom:0}` against a `body` with no height of its own, so
// once `body` became the positioned containing block the root's inset box collapsed to height 0 and
// the app vanished behind the floating dialog. `height:100%` on `html, body` keeps the viewport box
// under the lock. air-n68: the host tab bar is hidden in WebUI (air-d50), but `#webui-root` kept its
// 30px top offset, leaving an empty strip; the `#tab-bar[hidden] ~ #webui-root{top:0}` rule drops it.
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

let staging = "";

// A scripted cpd: an empty cockpit snapshot (enough for the shell and its Settings button), the
// `cpd config` / `cpd config experimental` reads the Agent Tiers dialog issues, and empty answers
// for every other one-shot list. Self-contained, so no real cpd is ever spawned.
const FAKE_CPD_SOURCE = `#!/usr/bin/env node
"use strict";
const cursor = 0;
const emit = (value) => { process.stdout.write(JSON.stringify(value) + "\\n"); };
const contextPane = () => ({ ok: true, events_cursor: cursor, contexts: [] });
const planPane = () => ({ ok: true, events_cursor: cursor, plans: [] });
const sessionPane = () => ({ ok: true, events_cursor: cursor, sessions: [] });
const inboxPane = () => ({ ok: true, events_cursor: cursor, items: [], hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, engine: { running: false } });
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
  emit(snapshot(1));
  readLines((line) => {
    let request;
    try { request = JSON.parse(line); } catch { emit({ t: "error", id: null, error: { code: "bad_request", message: "not json" } }); return; }
    if (request.t === "refresh") emit(snapshot(2));
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
function configCommand(args) {
  if (args[0] === "experimental" && args.length === 1) {
    emit({ ok: true, experimental: { durable_workers: false, write_curation: false, failure_triage: false, turn_question_guard: false } });
    return;
  }
  if (args.length === 0) {
    emit({ ok: true, config: { context_distill_tier: "G1", worker_tier: "G1", lead_tier: "G1", manager_tier: "G1" } });
    return;
  }
  emit({ ok: true });
}
const argv = process.argv.slice(2);
const command = argv[0];
const rest = argv.slice(1);
switch (command) {
  case "cockpit": if (rest[0] === "serve") cockpitServe(); else emit({ ok: true }); break;
  case "config": configCommand(rest); break;
  case "events": if (rest[0] === "follow") stayAlive(); else emit({ ok: true, id: 1, kind: "watch-started", at: "1970-01-01T00:00:00.000Z", text: "", plan: null, context: null, session: null, job: null, attempt: null, data: {} }); break;
  case "context": if (rest[0] === "list") emit(contextPane()); else if (rest[0] === "curation") emit({ ok: true, retire: [] }); else emit({ ok: true }); break;
  case "plan": if (rest[0] === "list") emit(planPane()); else emit({ ok: true }); break;
  case "session": if (rest[0] === "list") emit(sessionPane()); else emit({ ok: true }); break;
  case "inbox": if (rest[0] === "list") emit(inboxPane()); else emit({ ok: true }); break;
  case "incident": emit({ ok: true, on: false }); break;
  case "attempt": emit({ ok: true, attempts: [] }); break;
  case "--version": emit({ ok: true, version: "0.0.0", protocol: 25 }); break;
  default: emit({ ok: true });
}
`;

test.beforeAll(() => {
  // A private bin dir with the scripted fake cpd as `cpd`, and a fake login shell that reports the
  // app's PATH unchanged. `resolveLoginPath` runs `$SHELL -ilc`; the real one would drop the fake
  // dir, so the pty would spawn a real cpd. The fake shell keeps the fake first.
  staging = mkdtempSync(join(tmpdir(), "cpd-e2e-dialog-"));
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

test("opening the Agent Tiers dialog keeps the WebUI root full-height under the scroll lock, with no tab-bar offset (air-n74, air-n68)", async ({ page, electronApp }) => {
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

  // WebUI: the host tab bar is hidden (air-d50) and the WebUI root starts at the very top of the
  // window, with no empty strip above it (air-n68).
  await expect(page.locator("#tab-bar")).toBeHidden();
  const before = await page.evaluate(() => {
    const r = document.getElementById("webui-root").getBoundingClientRect();
    return { top: r.top, height: r.height };
  });
  expect(before.top).toBe(0);
  expect(before.height).toBeGreaterThan(0);

  // Open the Agent Tiers dialog the way a User does: the Settings button in the status bar.
  await page.locator('[data-part="settings"]').click();
  await expect(page.locator('[data-part="agent-tiers-dialog"]')).toBeVisible();

  // Base UI's modal scroll lock is live. Unlike react-remove-scroll (Radix), Base UI does not use
  // a `data-scroll-locked` attribute: @base-ui/utils/useScrollLock writes inline styles on the
  // viewport scroller (and, on the inset-scrollbar path, an `data-base-ui-scroll-locked` attribute
  // on <html>). In this app electron.html already pins `html, body { overflow: hidden }`, so Base UI
  // reads the page as already locked and leaves its styles untouched. Assert the observable lock
  // instead: the document does not scroll while the dialog is open.
  await expect
    .poll(
      () =>
        page.evaluate(() => {
          window.scrollTo(0, 1000);
          return window.scrollY;
        }),
      { timeout: 30_000 },
    )
    .toBe(0);

  // While the lock is on, the app is still laid out: the WebUI root keeps its full height and still
  // starts at the window top (no hidden tab-bar offset). Before the fix, body{position:relative}
  // collapsed this inset box to height 0 and the whole app vanished behind the dialog.
  const during = await page.evaluate(() => {
    const r = document.getElementById("webui-root").getBoundingClientRect();
    return { top: r.top, height: r.height, tabsHidden: document.getElementById("tab-bar").hidden };
  });
  expect(during.height).toBeGreaterThan(0);
  expect(during.top).toBe(0);
  expect(during.tabsHidden).toBe(true);
});
