import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { type Options } from "../lib/adapter.ts";
import { identityOf, LONG_REFUSAL_NOTICE, pollAdapter, refusal, RELOAD_COMMAND, reloadLogPath, type Guard } from "../lib/reload.ts";
import { Fixture, ID, planShow, waitFor } from "./fake.ts";

const GOOD = "export default function (pi: unknown): void {}\n";

let installs = 0;
/** A new mtime, as a new install has (even if the inode is reused). */
const touch = (dir: string) => {
  const t = new Date(Date.UTC(2030, 0, 1) + ++installs * 1000);
  utimesSync(dir, t, t);
};
/** What `scripts/install-cpd.sh` does to the adapter directory: replaced as a whole, here with `files` (default: a package that loads). */
const install = (f: Fixture, files: Record<string, string> = { "extension.ts": GOOD, "package.json": "{}" }) => {
  rmSync(f.adapterDir, { recursive: true });
  mkdirSync(f.adapterDir);
  for (const [name, content] of Object.entries(files)) writeFileSync(join(f.adapterDir, name), content);
  touch(f.adapterDir);
};

const noticeFor = (reason: string) => `cpd: adapter update not loaded: ${reason}`;

/** A started adapter whose `ctx.reload()` behaves like pi's: shutdown, then start of the new load. */
async function started(t: { after(fn: () => Promise<void>): void }, options: Options = {}): Promise<Fixture> {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.pi.onReload = async () => {
    await f.pi.fire("session_shutdown", { reason: "reload" });
    await f.pi.fire("session_start", { reason: "reload" });
  };
  f.load(options);
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return f;
}

/** Lets every promise settle and the callbacks they queue run (the polls under mocked timers are `async`). */
const flush = () => new Promise((resolve) => setImmediate(resolve));

type Hooks = { after(fn: () => void | Promise<void>): void };

/**
 * A clock made of the adapter's own polling: a second poll of the same directory at the same interval, which sees a package that never validates
 * and so retries at every tick. `after(n)` resolves when it has retried `n` more times: that many poll intervals passed since, a fact to wait for
 * where the test claims "nothing happens however many polls". Make it before the install it times.
 */
function clock(t: Hooks, f: Fixture) {
  let retries = 0;
  const poll = pollAdapter({
    dir: f.adapterDir,
    intervalMs: 20,
    validate: async () => (retries++, "never validates"),
    refusal: () => undefined,
    trigger: () => {},
    onRefused: () => {},
    onError: () => {},
    log: () => {},
    onLongRefusal: () => {},
  });
  t.after(() => poll.stop());
  return { after: (n: number) => { const at = retries; return waitFor(`${n} more polls`, () => retries >= at + n); } };
}

const ok: Guard = { idle: true, pending: false, state: "idle", editorText: "" };

test("guard: each condition refuses, all clear allows", () => {
  assert.equal(refusal(ok), undefined);
  assert.equal(refusal({ ...ok, idle: false }), "busy");
  assert.equal(refusal({ ...ok, pending: true }), "pending");
  assert.equal(refusal({ ...ok, state: "blocked" }), "state");
  assert.equal(refusal({ ...ok, state: "working" }), "state");
  assert.equal(refusal({ ...ok, editorText: "x" }), "typing");
});

test("a changed adapter directory triggers its command once, with expandPromptTemplates, and reloads", async (t) => {
  const f = await started(t);
  install(f);
  await waitFor("the reload", () => f.pi.reloads > 0);
  await waitFor("the command's end", () => f.pi.commandsRunning.size === 0);
  assert.deepEqual(f.pi.userMessages, [{ content: `/${RELOAD_COMMAND}`, options: { expandPromptTemplates: true } }]);
  assert.ok(f.pi.notices.includes("cpd: adapter updated, reloading"));
  assert.equal(f.pi.reloads, 1);
  const watching = logLines(f).filter((l) => l.includes("poll: watching"));
  assert.equal(watching.length, 2, "the new load starts a poll");
  assert.ok(watching[1]!.includes(`(${identityOf(f.adapterDir)})`), "which records the new identity as its baseline: no second reload");
  assert.deepEqual(f.pi.commandErrors, [], "nothing used the context after reload()");
  assert.deepEqual(f.pi.sent.filter((s) => s.message.customType !== "cpd"), [], "nothing reaches the model");
});

test("ENOENT is the install in progress, not a change", async (t) => {
  const f = await started(t);
  rmSync(f.adapterDir, { recursive: true });
  await waitFor("the poll to see it missing", () => logLines(f).some((l) => l.endsWith("the adapter directory is missing (an install in progress); waiting")));
  assert.deepEqual(f.pi.userMessages, []);
  mkdirSync(f.adapterDir);
  writeFileSync(join(f.adapterDir, "extension.ts"), GOOD);
  writeFileSync(join(f.adapterDir, "package.json"), "{}");
  touch(f.adapterDir);
  await waitFor("the trigger", () => f.pi.userMessages.length > 0);
});

const conditions: [string, string, (f: Fixture) => unknown, (f: Fixture) => unknown][] = [
  ["pi is busy", "busy", (f) => (f.pi.idle = false), (f) => (f.pi.idle = true)],
  ["messages are pending", "pending", (f) => (f.pi.pendingMessages = true), (f) => (f.pi.pendingMessages = false)],
  ["text is in the editor", "typing", (f) => (f.pi.editorText = "unsent"), (f) => (f.pi.editorText = "")],
  ["a pi dialog is open", "state", (f) => f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" }), (f) => f.pi.fire("ui_prompt_end")],
];

for (const [name, why, block, clear] of conditions) {
  test(`no reload while ${name}; a later tick retries`, async (t) => {
    const f = await started(t);
    await block(f);
    install(f);
    await waitFor("the refusal", () => logLines(f).some((l) => l.endsWith(`poll: refused: ${why}`)));
    assert.deepEqual(f.pi.userMessages, [], "refused at the trigger");
    assert.equal(f.pi.reloads, 0);
    await clear(f);
    await waitFor("the reload", () => f.pi.reloads > 0);
    assert.equal(f.pi.userMessages.length, 1);
  });

  test(`the command itself refuses while ${name}, and the poll triggers again`, async (t) => {
    const f = await started(t);
    const handler = f.pi.commands.get(RELOAD_COMMAND)!;
    await block(f);
    await handler("", f.pi.commandCtx());
    assert.equal(f.pi.reloads, 0);
    await clear(f);
    await handler("", f.pi.commandCtx());
    assert.equal(f.pi.reloads, 1);
  });
}

test("a reload that did nothing is retried at the next tick", async (t) => {
  const f = await started(t);
  f.pi.onReload = () => {}; // like pi's mid-turn reload: resolves, nothing happens
  install(f);
  await waitFor("two triggers", () => f.pi.userMessages.length >= 2);
  assert.ok(f.pi.reloads >= 2);
  assert.deepEqual(f.pi.commandErrors, []);
});

test("the command does nothing without an active adapter", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.load();
  await f.pi.commands.get(RELOAD_COMMAND)!("", f.pi.commandCtx());
  assert.equal(f.pi.reloads, 0);
});

test("the poll stops at session_shutdown", async (t) => {
  const f = await started(t);
  const c = clock(t, f);
  await f.pi.fire("session_shutdown");
  install(f);
  await c.after(4); // it would have noticed the install at its first tick
  assert.deepEqual(f.pi.userMessages, []);
  assert.equal(logLines(f).some((l) => l.includes("the identity changed")), false);
});

// ---- pre-validation (commands.md §15.6) ----

test("a directory that changes between two polls is not validated or reloaded until the identity repeats", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const dir = mkdtempSync(join(tmpdir(), "pi-poll-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const calls: string[] = [];
  const poll = pollAdapter({
    dir,
    intervalMs: 10,
    validate: async () => void calls.push("validate"),
    refusal: () => undefined,
    trigger: () => void calls.push("trigger"),
    onRefused: () => void calls.push("refused"),
    onError: (e) => void calls.push(`error ${e}`),
    log: () => {},
    onLongRefusal: () => void calls.push("long"),
  });
  t.after(() => poll.stop());
  const tick = async () => {
    t.mock.timers.tick(10);
    await flush();
  };
  touch(dir);
  await tick(); // a new identity: remembered
  touch(dir);
  await tick(); // another one: still not the same twice
  touch(dir);
  await tick();
  assert.deepEqual(calls, []);
  await tick(); // the same identity on two consecutive polls
  assert.deepEqual(calls, ["validate", "trigger"]);
  rmSync(dir, { recursive: true });
  await tick(); // ENOENT forgets what was seen
  mkdirSync(dir);
  touch(dir);
  poll.retry();
  await tick();
  assert.deepEqual(calls, ["validate", "trigger"], "the identity seen before the ENOENT does not count");
});

test("an unchanged directory is never validated or triggered, however many polls", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const dir = mkdtempSync(join(tmpdir(), "pi-poll-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const calls: string[] = [];
  const poll = pollAdapter({
    dir,
    intervalMs: 10,
    validate: async () => void calls.push("validate"),
    refusal: () => undefined,
    trigger: () => void calls.push("trigger"),
    onRefused: () => void calls.push("refused"),
    onError: (e) => void calls.push(`error ${e}`),
    log: () => {},
    onLongRefusal: () => void calls.push("long"),
  });
  t.after(() => poll.stop());
  for (let i = 0; i < 20; i++) {
    t.mock.timers.tick(10);
    await flush();
  }
  assert.deepEqual(calls, []);
});

const refused: [string, Record<string, string>, string][] = [
  ["extension.ts is missing", { "package.json": "{}" }, "entry files missing"],
  ["package.json is missing", { "extension.ts": GOOD }, "entry files missing"],
  ["extension.ts throws when loaded", { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" }, "boom"],
  ["extension.ts has no default export function", { "extension.ts": "export default 3;\n", "package.json": "{}" }, "the default export of extension.ts is not a function"],
];

for (const [name, files, reason] of refused) {
  test(`no reload while ${name}: one visible error, retried at a later poll`, async (t) => {
    const f = await started(t);
    const c = clock(t, f);
    install(f, files);
    await waitFor("the error", () => f.pi.errorNotices.length > 0);
    await c.after(5); // many polls
    assert.deepEqual(f.pi.errorNotices, [noticeFor(reason)], "once, not once per poll");
    assert.equal(f.pi.reloads, 0);
    assert.deepEqual(f.pi.userMessages, []);
    assert.equal(f.pi.notices.includes("cpd: adapter updated, reloading"), false);
    assert.deepEqual(f.argvs().filter((a) => a[0] !== "plan" && a[0] !== "session"), [], "no cpd command for it");
    assert.deepEqual(f.pi.sent.filter((s) => s.message.customType !== "cpd"), []);
    // The same directory, repaired: the new identity is checked again and reloaded once.
    for (const [file, content] of Object.entries({ "extension.ts": GOOD, "package.json": "{}" })) writeFileSync(join(f.adapterDir, file), content);
    touch(f.adapterDir);
    await waitFor("the reload", () => f.pi.reloads > 0);
    await waitFor("the command's end", () => f.pi.commandsRunning.size === 0);
    assert.equal(f.pi.reloads, 1);
    assert.equal(f.pi.userMessages.length, 1);
    assert.equal(f.pi.errorNotices.length, 1, "a package that passes shows no error");
  });
}

test("a load that hangs is refused after the timeout", async (t) => {
  const f = await started(t, { reload: { timeoutMs: 300 } });
  install(f, { "extension.ts": "setInterval(() => {}, 1000);\nexport default function () {}\n", "package.json": "{}" });
  await waitFor("the error", () => f.pi.errorNotices.length > 0, 5000);
  assert.deepEqual(f.pi.errorNotices, [noticeFor("load timed out after 300 ms")]);
  assert.equal(f.pi.reloads, 0);
});

test("the same refusal is shown again after a poll that passed", async (t) => {
  const f = await started(t);
  const bad = { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" };
  install(f, bad);
  await waitFor("the first error", () => f.pi.errorNotices.length === 1);
  f.pi.onReload = () => {}; // the good package passes but pi does not reload it
  install(f);
  await waitFor("the trigger", () => f.pi.userMessages.length > 0);
  install(f, bad);
  await waitFor("the second error", () => f.pi.errorNotices.length === 2);
  assert.deepEqual(f.pi.errorNotices, [noticeFor("boom"), noticeFor("boom")]);
});

// ---- a busy Lead: its idle gaps are shorter than the poll interval ----

test("a validated install is tried when pi settles, though no tick ever sees pi idle", async (t) => {
  const f = await started(t);
  f.pi.idle = false;
  install(f);
  await waitFor("the refusal", () => logLines(f).some((l) => l.endsWith("poll: refused: busy")));
  // The idle gap: pi is idle only while `agent_settled` runs. Timers cannot run in between, so no tick sees it.
  f.pi.idle = true;
  await f.pi.fire("agent_settled");
  f.pi.idle = false;
  assert.equal(f.pi.userMessages.length, 1, "triggered by the settle, not by a tick");
  assert.equal(f.pi.reloads, 1);
  assert.ok(logLines(f).some((l) => l.endsWith("(/cpd-reload-after-install, at a nudge)")));
});

test("pi settling before the package validated, or while it is not idle, triggers nothing", async (t) => {
  const f = await started(t);
  await f.pi.fire("agent_settled");
  assert.deepEqual(f.pi.userMessages, [], "nothing changed");
  f.pi.idle = false;
  install(f);
  await waitFor("the refusal", () => logLines(f).some((l) => l.endsWith("poll: refused: busy")));
  await f.pi.fire("agent_settled"); // settled, but pi says it is not idle: the guard decides
  assert.deepEqual(f.pi.userMessages, []);
  assert.equal(f.pi.reloads, 0);
});

test("nudge: only for a package that validated and is still the directory's identity", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const dir = mkdtempSync(join(tmpdir(), "pi-poll-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const calls: string[] = [];
  let why: string | undefined = "busy";
  const poll = pollAdapter({
    dir,
    intervalMs: 10,
    validate: async () => void calls.push("validate"),
    refusal: () => why,
    trigger: () => void calls.push("trigger"),
    onRefused: () => {},
    onError: (e) => void calls.push(`error ${e}`),
    log: () => {},
    onLongRefusal: () => {},
  });
  t.after(() => poll.stop());
  const tick = async () => {
    t.mock.timers.tick(10);
    await flush();
  };
  why = undefined;
  poll.nudge();
  touch(dir);
  poll.nudge();
  assert.deepEqual(calls, [], "no change yet, and a change not yet validated, are not nudged");
  why = "busy";
  await tick();
  await tick(); // validated, refused
  assert.deepEqual(calls, ["validate"]);
  touch(dir); // replaced again after it validated
  why = undefined;
  poll.nudge();
  assert.deepEqual(calls, ["validate"], "the validated identity is gone: the ticks start over");
  await tick();
  await tick();
  assert.deepEqual(calls, ["validate", "validate", "trigger"]);
  poll.nudge();
  assert.deepEqual(calls, ["validate", "validate", "trigger"], "one trigger in flight");
  poll.retry();
  poll.nudge();
  assert.deepEqual(calls, ["validate", "validate", "trigger", "trigger"]);
});

// ---- the decision log (commands.md §15.6 "Log") ----

const logLines = (f: Fixture): string[] => {
  const path = reloadLogPath(f.cpdHome, ID);
  return existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : [];
};
/** The log without its timestamps and identities. */
const decisions = (f: Fixture): string[] => logLines(f).map((l) => l.replace(/^\S+ /, "").replace(/\d+:\d+:[\d.]+/g, "<id>"));

test("the log path is built from CPD_HOME and the session", () => {
  assert.equal(reloadLogPath("/h", "s1"), "/h/run/adapter/s1.log");
});

test("every decision of an install that reloads is logged, one line each, with a time", async (t) => {
  const f = await started(t);
  install(f);
  await waitFor("the reload", () => f.pi.reloads > 0);
  await waitFor("the command's end", () => decisions(f).includes("command: ctx.reload() returned"));
  assert.match(logLines(f)[0]!, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z start: unknown$/);
  const d = decisions(f).filter((l) => !l.startsWith("poll: watching"));
  assert.deepEqual(d, [
    "start: unknown",
    "poll: the identity changed <id> -> <id>; not stable yet",
    "poll: <id> is stable; validating the package",
    "poll: pre-validation passed",
    "poll: trigger sent (/cpd-reload-after-install, at a tick)",
    "command: reloading",
    "stop",
    "start: reload",
    "command: ctx.reload() returned",
  ]);
});

test("a pre-validation failure and the command's refusal are logged with their reason", async (t) => {
  const f = await started(t);
  const c = clock(t, f);
  install(f, { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" });
  await waitFor("the failure", () => decisions(f).includes("poll: pre-validation failed: boom"));
  await c.after(5); // many polls: the same line is not repeated
  assert.equal(decisions(f).filter((l) => l === "poll: pre-validation failed: boom").length, 1);
  f.pi.editorText = "unsent";
  const handler = f.pi.commands.get(RELOAD_COMMAND)!;
  await handler("", f.pi.commandCtx());
  assert.ok(decisions(f).includes("command: refused: typing"));
});

test("a refusal that lasts is logged once, not at every poll", async (t) => {
  const f = await started(t);
  const c = clock(t, f);
  f.pi.pendingMessages = true;
  install(f);
  await waitFor("the refusal", () => decisions(f).includes("poll: refused: pending"));
  await c.after(5);
  assert.equal(decisions(f).filter((l) => l === "poll: refused: pending").length, 1);
});

test("a refusal of more than the limit tells the User once per install; a reload before it says nothing", async (t) => {
  const f = await started(t, { reload: { longRefusalMs: 250 } });
  const c = clock(t, f);
  f.pi.idle = false;
  install(f);
  await waitFor("the notice", () => f.pi.notices.includes(LONG_REFUSAL_NOTICE), 5000);
  await c.after(10); // many more polls
  assert.equal(f.pi.notices.filter((n) => n === LONG_REFUSAL_NOTICE).length, 1);
  assert.ok(decisions(f).some((l) => l.startsWith("poll: not reloaded") && l.endsWith("after the package validated; told the User")));
  assert.equal(f.pi.errorNotices.length, 0);
  f.pi.idle = true;
  await waitFor("the reload", () => f.pi.reloads > 0);
});

test("a package that does not validate is not a long refusal (a reload must not be advised)", async (t) => {
  const f = await started(t, { reload: { longRefusalMs: 100 } });
  const c = clock(t, f);
  install(f, { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" });
  const from = Date.now();
  await waitFor("the failure", () => decisions(f).includes("poll: pre-validation failed: boom"));
  await c.after(10);
  await waitFor("longer than the limit", () => Date.now() - from > 100);
  assert.equal(f.pi.notices.includes(LONG_REFUSAL_NOTICE), false);
});

test("a log that cannot be written is notified once per run, starts no turn, and the reload still happens", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  const failures = () => f.pi.errorNotices.filter((n) => n.startsWith("cpd: cannot write the reload log"));
  let beforeReload = -1;
  f.pi.onReload = async () => {
    beforeReload = failures().length; // every line of the first run (watching, changed, stable, validated, trigger, reloading) is written by now
    await f.pi.fire("session_shutdown", { reason: "reload" });
    await f.pi.fire("session_start", { reason: "reload" });
  };
  mkdirSync(join(f.cpdHome, "run"));
  writeFileSync(join(f.cpdHome, "run", "adapter"), "a file where the log directory should be");
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  await waitFor("the notice", () => failures().length > 0);
  install(f);
  await waitFor("the reload", () => f.pi.reloads > 0);
  assert.equal(beforeReload, 1, "once per run, not once per line");
  await waitFor("the command's end", () => f.pi.commandsRunning.size === 0);
  assert.equal(failures().length, 2, "the new run notifies again");
  assert.deepEqual(f.pi.errors(), [], "no message, so no turn");
});
