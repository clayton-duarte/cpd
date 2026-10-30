import type { ChildProcess, spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, renameSync, symlinkSync, mkdtempSync, readdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test, type TestContext } from "node:test";
import { type Options } from "../lib/adapter.ts";
import { fileURLToPath } from "node:url";
import { identityOf, LONG_REFUSAL_NOTICE, LOAD_TIMEOUT_MS, pollAdapter, refusal, RELOAD_COMMAND, reloadLogPath, validatePackage, type Guard } from "../lib/reload.ts";
import { Fixture, ID, notify, planShow, waitFor } from "./fake.ts";

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
  for (const skill of ["cpd-lead", "cpd-manager"]) {
    mkdirSync(join(f.adapterDir, "skills", skill), { recursive: true });
    writeFileSync(join(f.adapterDir, "skills", skill, "SKILL.md"), `the ${skill} skill\n`);
  }
  touch(f.adapterDir);
};

const noticeFor = (reason: string) => `\x1b[1mφ\x1b[22m: adapter update not loaded: ${reason}`;

/** An extension the fake pi never finishes loading (a top-level await that never settles). */
const HANGS = "await new Promise(() => {});\nexport default function () {}\n";

/**
 * Stands for pi in the pre-validation (`Validation.spawn`; the real pi is not ours): a child that, in process, answers as pi does for the package in `-e <dir>`:
 * exit 0 when `extension.ts` loads, 1 with pi's message on stderr when it throws or has no default export function, and never ends when it never finishes loading.
 * Its `kill` ends it, like SIGKILL. It has no `pid`, so the product's group kill (`process.kill(-pid)`) cannot reach a real process.
 */
function scriptedPi() {
  type Run = { command: string; args: string[]; options: { cwd?: string; env?: NodeJS.ProcessEnv }; scratchExisted: boolean; killed: string[] };
  const runs: Run[] = [];
  const start = ((command: string, args: string[], options: Run["options"]) => {
    const child = Object.assign(new EventEmitter(), { stderr: Object.assign(new EventEmitter(), { setEncoding: () => {} }), pid: undefined });
    const run: Run = { command, args, options, scratchExisted: existsSync(options.cwd ?? ""), killed: [] };
    runs.push(run);
    let ended = false;
    const end = (code: number | null, signal: string | null) => {
      if (ended) return;
      ended = true;
      child.emit("close", code, signal);
    };
    Object.assign(child, {
      kill: (signal: string) => {
        run.killed.push(signal);
        queueMicrotask(() => end(null, signal)); // a process's `close` follows its kill, it is not part of it
        return true;
      },
    });
    const entry = join(args[args.indexOf("-e") + 1]!, "extension.ts");
    const source = readFileSync(entry, "utf8");
    const failed = (why: string) => {
      child.stderr.emit("data", `Error: Failed to load extension "${entry}": ${why}\nHint: Start without extensions using "pi -ne".\n`);
      end(1, null);
    };
    queueMicrotask(() => {
      const thrown = /^throw new Error\("(.*)"\);/.exec(source);
      if (source.includes("await new Promise(() => {})")) return; // never finishes loading
      if (thrown) failed(thrown[1]!);
      else if (!/export default function/.test(source)) failed(`Extension does not export a valid factory function: ${entry}`);
      else end(0, null);
    });
    return child as unknown as ChildProcess;
  }) as unknown as typeof spawn;
  return { runs, spawn: start };
}

/** Never ends: the adapter's shutdown wait (`Options.sleep`) is not a real timer in these tests; `reporter.close` is what ends it. */
const neverSleep = (() => new Promise<undefined>(() => {})) as NonNullable<Options["sleep"]>;

/** The interval of the adapter's poll in these tests (`Fixture.load` sets 20). */
const TICK = 20;

/** `options` for `Fixture.load`: the poll's `setInterval` (and `setTimeout` when `timers` says) on `t.mock.timers`, pi's load in process, no real shutdown wait. Make it before the adapter starts. */
function arm(t: TestContext, options: Options = {}, timers: ("setInterval" | "setTimeout")[] = ["setInterval"]): Options {
  t.mock.timers.enable({ apis: timers });
  const reload = (options.reload ??= {});
  if (!("spawn" in reload)) reload.spawn = scriptedPi().spawn;
  return { sleep: neverSleep, ...options, reload };
}

/** Resolves when no `cpd` call is in flight and no command runs: everything the last tick set going has run to its end. */
async function quiesce(f: Fixture): Promise<void> {
  await f.quiet();
  await waitFor("no command running", () => f.pi.commandsRunning.size === 0);
  await f.quiet();
}

/** Advances the poll's virtual clock one interval at a time, letting what each tick started finish, until `cond` holds: no real time passes, and a hang is a count of ticks. */
async function until<T>(t: TestContext, f: Fixture, what: string, cond: () => T | undefined | null | false, max = 200): Promise<T> {
  for (let n = 0; ; n++) {
    await quiesce(f);
    const v = cond();
    if (v) return v;
    if (n >= max) throw new Error(`${max} polls passed without ${what}`);
    t.mock.timers.tick(TICK);
  }
}

/** A started adapter whose `ctx.reload()` behaves like pi's: shutdown, then start of the new load. */
async function started(t: TestContext, options: Options = {}, timers?: ("setInterval" | "setTimeout")[]): Promise<Fixture> {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.pi.onReload = async () => {
    await f.pi.fire("session_shutdown", { reason: "reload" });
    await f.pi.fire("session_start", { reason: "reload" });
  };
  f.load(arm(t, options, timers));
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
function clock(t: TestContext & Hooks, f: Fixture) {
  let retries = 0;
  const poll = pollAdapter({
    dir: f.adapterDir,
    intervalMs: 20,
    validate: async () => (retries++, notify(), "never validates"),
    refusal: () => undefined,
    trigger: () => {},
    onRefused: () => {},
    onError: () => {},
    log: () => {},
    onLongRefusal: () => {},
  });
  t.after(() => poll.stop());
  return { after: (n: number) => { const at = retries; return until(t, f, `${n} more polls`, () => retries >= at + n); } };
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
  await until(t, f, "the reload", () => f.pi.reloads > 0);
  await waitFor("the command's end", () => f.pi.commandsRunning.size === 0);
  assert.deepEqual(f.pi.userMessages, [{ content: `/${RELOAD_COMMAND}`, options: { expandPromptTemplates: true } }]);
  assert.ok(!f.pi.notices.some((n) => n.includes("φ: adapter updated")), "no notice on a normal reload");
  assert.equal(f.pi.reloads, 1);
  const watching = logLines(f).filter((l) => l.includes("poll: watching"));
  assert.equal(watching.length, 2, "the new load starts a poll");
  assert.ok(watching[1]!.includes(`(${identityOf(f.adapterDir)})`), "which records the new identity as its baseline: no second reload");
  assert.deepEqual(f.pi.commandErrors, [], "nothing used the context after reload()");
  assert.deepEqual(f.pi.sent.filter((s) => s.message.customType !== "cpd"), [], "nothing reaches the model");
});

test("the poll watches the adapter symlink: repointing it to a new directory and pruning the old one is a change, not an install in progress", async (t) => {
  // contract §3.7 "The adapter swap": the adapter runs from `share/pi.<id>`; the installed path is the `pi` link beside it.
  const root = mkdtempSync(join(tmpdir(), "pi-swap-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const repo = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  symlinkSync(join(repo, "node_modules"), join(root, "node_modules")); // so the copy resolves pi-tui as the checkout does
  const share = join(root, "share");
  mkdirSync(share);
  const copy = (id: string) => {
    mkdirSync(join(share, id));
    for (const n of ["extension.ts", "package.json", "lib", "compact-tools"]) cpSync(join(repo, "pi", n), join(share, id, n), { recursive: true });
  };
  copy("pi.1");
  symlinkSync("pi.1", join(share, "pi"));
  const { adapter } = (await import(join(share, "pi.1", "lib", "adapter.ts"))) as typeof import("../lib/adapter.ts");

  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.pi.onReload = async () => {
    await f.pi.fire("session_shutdown", { reason: "reload" });
    await f.pi.fire("session_start", { reason: "reload" });
  };
  f.load(arm(t, { reload: { dir: undefined } }), f.pi, adapter); // no `dir`: the adapter's own default path
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);

  // The install: a new version, the link renamed over `pi`, the old version pruned.
  copy("pi.2");
  symlinkSync("pi.2", join(share, "pi.link"));
  renameSync(join(share, "pi.link"), join(share, "pi"));
  rmSync(join(share, "pi.1"), { recursive: true });

  await until(t, f, "the reload", () => f.pi.reloads > 0);
  assert.ok(
    logLines(f).some((l) => l.includes("poll: watching") && l.includes(join(share, "pi")) && !l.includes("pi.1")),
    "the poll watches the link",
  );
  assert.equal(logLines(f).some((l) => l.endsWith("the adapter directory is missing (an install in progress); waiting")), false);
});

test("ENOENT is the install in progress, not a change", async (t) => {
  const f = await started(t);
  rmSync(f.adapterDir, { recursive: true });
  await until(t, f, "the poll to see it missing", () => logLines(f).some((l) => l.endsWith("the adapter directory is missing (an install in progress); waiting")));
  assert.deepEqual(f.pi.userMessages, []);
  mkdirSync(f.adapterDir);
  writeFileSync(join(f.adapterDir, "extension.ts"), GOOD);
  writeFileSync(join(f.adapterDir, "package.json"), "{}");
  touch(f.adapterDir);
  await until(t, f, "the trigger", () => f.pi.userMessages.length > 0);
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
    await until(t, f, "the refusal", () => logLines(f).some((l) => l.endsWith(`poll: refused: ${why}`)));
    assert.deepEqual(f.pi.userMessages, [], "refused at the trigger");
    assert.equal(f.pi.reloads, 0);
    await clear(f);
    await until(t, f, "the reload", () => f.pi.reloads > 0);
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
  await until(t, f, "two triggers", () => f.pi.userMessages.length >= 2);
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

/** pi's own refusal, as the fake pi prints it (its first stderr line). */
const failedToLoad = (f: Fixture, why: string) => `Error: Failed to load extension "${join(f.adapterDir, "extension.ts")}": ${why}`;

const refused: [string, Record<string, string>, (f: Fixture) => string][] = [
  ["extension.ts is missing", { "package.json": "{}" }, () => "entry files missing"],
  ["package.json is missing", { "extension.ts": GOOD }, () => "entry files missing"],
  ["extension.ts throws when loaded", { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" }, (f) => failedToLoad(f, "boom")],
  [
    "extension.ts has no default export function",
    { "extension.ts": "export default 3;\n", "package.json": "{}" },
    (f) => failedToLoad(f, `Extension does not export a valid factory function: ${join(f.adapterDir, "extension.ts")}`),
  ],
];

for (const [name, files, reasonOf] of refused) {
  test(`no reload while ${name}: one visible error, retried at a later poll`, async (t) => {
    const f = await started(t);
    const c = clock(t, f);
    install(f, files);
    const reason = reasonOf(f);
    await until(t, f, "the error", () => f.pi.errorNotices.length > 0);
    await c.after(5); // many polls
    assert.deepEqual(f.pi.errorNotices, [noticeFor(reason)], "once, not once per poll");
    assert.equal(f.pi.reloads, 0);
    assert.deepEqual(f.pi.userMessages, []);
    assert.equal(f.pi.notices.includes("φ: adapter updated, reloading"), false);
    assert.deepEqual(f.argvs().filter((a) => a[0] !== "plan" && a[0] !== "session" && a[0] !== "review"), [], "no cpd command for it");
    assert.deepEqual(f.pi.sent.filter((s) => s.message.customType !== "cpd"), []);
    // The same directory, repaired: the new identity is checked again and reloaded once.
    for (const [file, content] of Object.entries({ "extension.ts": GOOD, "package.json": "{}" })) writeFileSync(join(f.adapterDir, file), content);
    touch(f.adapterDir);
    await until(t, f, "the reload", () => f.pi.reloads > 0);
    await waitFor("the command's end", () => f.pi.commandsRunning.size === 0);
    assert.equal(f.pi.reloads, 1);
    assert.equal(f.pi.userMessages.length, 1);
    assert.equal(f.pi.errorNotices.length, 1, "a package that passes shows no error");
  });
}

test("a load that hangs is refused after the timeout, and tried again at a later poll", async (t) => {
  // The timeout is read at every poll (`validatePackage(dir, options.reload)`). The hanging package never ends: the clock passes the timeout, nothing else does.
  const pi = scriptedPi();
  const reload = { timeoutMs: 300, spawn: pi.spawn };
  const f = await started(t, { reload }, ["setInterval", "setTimeout"]);
  install(f, { "extension.ts": HANGS, "package.json": "{}" });
  await until(t, f, "pi to start loading the package", () => pi.runs.length > 0);
  t.mock.timers.tick(299);
  await quiesce(f);
  assert.deepEqual(f.pi.errorNotices, [], "not yet: 299 ms of 300");
  assert.deepEqual(pi.runs[0]!.killed, []);
  t.mock.timers.tick(1);
  await quiesce(f);
  assert.deepEqual(f.pi.errorNotices, [noticeFor("load timed out after 300 ms")]);
  assert.deepEqual(pi.runs[0]!.killed, ["SIGKILL"], "the hung load is killed");
  assert.equal(f.pi.reloads, 0);
  // The next poll passes.
  writeFileSync(join(f.adapterDir, "extension.ts"), GOOD);
  touch(f.adapterDir);
  reload.timeoutMs = LOAD_TIMEOUT_MS;
  await until(t, f, "the reload", () => f.pi.reloads > 0);
  assert.equal(f.pi.errorNotices.length, 1, "a timeout is a refusal shown once, not a failure");
});

test("validatePackage runs pi with no extensions, no session, the package, rpc mode, offline and an empty scratch agent directory it removes", async () => {
  const dir = mkdtempSync(join(tmpdir(), "pi-validate-"));
  try {
    for (const [file, content] of Object.entries({ "extension.ts": GOOD, "package.json": "{}" })) writeFileSync(join(dir, file), content);
    const pi = scriptedPi();
    assert.equal(await validatePackage(dir, { spawn: pi.spawn }), undefined, "pi exits 0: the package loads");
    assert.equal(pi.runs.length, 1, "one pi run");
    const [call] = pi.runs;
    assert.equal(call!.command, "pi");
    assert.deepEqual(call!.args, ["--no-extensions", "--no-session", "-e", dir, "--mode", "rpc"]);
    assert.equal(call!.options.env!.PI_OFFLINE, "1");
    assert.equal(call!.scratchExisted, true);
    assert.equal(call!.options.env!.PI_CODING_AGENT_DIR, call!.options.cwd);
    assert.equal(dirname(call!.options.cwd!), tmpdir());
    assert.notEqual(call!.options.cwd, process.env.PI_CODING_AGENT_DIR);
    assert.equal(existsSync(call!.options.cwd!), false, "the scratch directory is removed");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the same refusal is shown again after a poll that passed", async (t) => {
  const f = await started(t);
  const bad = { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" };
  install(f, bad);
  await until(t, f, "the first error", () => f.pi.errorNotices.length === 1);
  f.pi.onReload = () => {}; // the good package passes but pi does not reload it
  install(f);
  await until(t, f, "the trigger", () => f.pi.userMessages.length > 0);
  install(f, bad);
  await until(t, f, "the second error", () => f.pi.errorNotices.length === 2);
  assert.deepEqual(f.pi.errorNotices, [noticeFor(failedToLoad(f, "boom")), noticeFor(failedToLoad(f, "boom"))]);
});

// ---- a busy Lead: its idle gaps are shorter than the poll interval ----

test("a validated install is tried when pi settles, though no tick ever sees pi idle", async (t) => {
  const f = await started(t);
  f.pi.idle = false;
  install(f);
  await until(t, f, "the refusal", () => logLines(f).some((l) => l.endsWith("poll: refused: busy")));
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
  await until(t, f, "the refusal", () => logLines(f).some((l) => l.endsWith("poll: refused: busy")));
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
  await until(t, f, "the reload", () => f.pi.reloads > 0);
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
  const failed = `poll: pre-validation failed: ${failedToLoad(f, "boom")}`;
  await until(t, f, "the failure", () => decisions(f).includes(failed));
  await c.after(5); // many polls: the same line is not repeated
  assert.equal(decisions(f).filter((l) => l === failed).length, 1);
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
  await until(t, f, "the refusal", () => decisions(f).includes("poll: refused: pending"));
  await c.after(5);
  assert.equal(decisions(f).filter((l) => l === "poll: refused: pending").length, 1);
});

test("a refusal of more than the limit tells the User once per install; a reload before it says nothing", async (t) => {
  let now = 0; // the adapter's clock (`reload.now`): it moves when the test says so
  const f = await started(t, { reload: { longRefusalMs: 250, now: () => now } });
  const c = clock(t, f);
  f.pi.idle = false;
  install(f);
  await until(t, f, "the refusal", () => logLines(f).some((l) => l.endsWith("poll: refused: busy")));
  assert.equal(f.pi.notices.includes(LONG_REFUSAL_NOTICE), false, "not yet: no time has passed");
  now = 250;
  await c.after(3);
  assert.equal(f.pi.notices.includes(LONG_REFUSAL_NOTICE), false, "exactly the limit is not more than it");
  now = 251;
  await until(t, f, "the notice", () => f.pi.notices.includes(LONG_REFUSAL_NOTICE));
  await c.after(10); // many more polls
  assert.equal(f.pi.notices.filter((n) => n === LONG_REFUSAL_NOTICE).length, 1);
  assert.ok(decisions(f).some((l) => l.startsWith("poll: not reloaded") && l.endsWith("after the package validated; told the User")));
  assert.equal(f.pi.errorNotices.length, 0);
  f.pi.idle = true;
  await until(t, f, "the reload", () => f.pi.reloads > 0);
});

test("a package that does not validate is not a long refusal (a reload must not be advised)", async (t) => {
  let now = 0;
  const f = await started(t, { reload: { longRefusalMs: 100, now: () => now } });
  const c = clock(t, f);
  install(f, { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" });
  await until(t, f, "the failure", () => decisions(f).includes(`poll: pre-validation failed: ${failedToLoad(f, "boom")}`));
  now = 1_000_000; // far past the limit
  await c.after(10);
  assert.equal(f.pi.notices.includes(LONG_REFUSAL_NOTICE), false);
});

test("a log that cannot be written is notified once per run, starts no turn, and the reload still happens", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  const failures = () => f.pi.errorNotices.filter((n) => n.startsWith("\x1b[1mφ\x1b[22m: cannot write the reload log"));
  let beforeReload = -1;
  f.pi.onReload = async () => {
    beforeReload = failures().length; // every line of the first run (watching, changed, stable, validated, trigger, reloading) is written by now
    await f.pi.fire("session_shutdown", { reason: "reload" });
    await f.pi.fire("session_start", { reason: "reload" });
  };
  mkdirSync(join(f.cpdHome, "run"));
  writeFileSync(join(f.cpdHome, "run", "adapter"), "a file where the log directory should be");
  f.load(arm(t));
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  await waitFor("the notice", () => failures().length > 0);
  const c = clock(t, f);
  install(f);
  await until(t, f, "the reload", () => f.pi.reloads > 0);
  // `reloads` counts the call to `ctx.reload()`: the old run's shutdown and the new run's start (each a `cpd` child) come after it.
  await waitFor("the new run's notice", () => failures().length > 1);
  await waitFor("the command's end", () => f.pi.commandsRunning.size === 0);
  await c.after(5); // many polls of the new run: and it is only one
  assert.equal(failures().length, 2, "the new run notifies again");
  assert.deepEqual(f.pi.errors(), [], "no message, so no turn");
});
