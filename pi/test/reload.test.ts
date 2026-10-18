import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { type Options } from "../lib/adapter.ts";
import { pollAdapter, refusal, RELOAD_COMMAND, type Guard } from "../lib/reload.ts";
import { Fixture, planShow, settle, waitFor } from "./fake.ts";

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
  await settle();
  assert.deepEqual(f.pi.userMessages, [], "an unchanged directory triggers nothing");
  install(f);
  await waitFor("the reload", () => f.pi.reloads > 0);
  await settle(200);
  assert.deepEqual(f.pi.userMessages, [{ content: `/${RELOAD_COMMAND}`, options: { expandPromptTemplates: true } }]);
  assert.ok(f.pi.notices.includes("cpd: adapter updated, reloading"));
  assert.equal(f.pi.reloads, 1, "the new load records the new identity: no second reload");
  assert.deepEqual(f.pi.commandErrors, [], "nothing used the context after reload()");
  assert.deepEqual(f.pi.sent.filter((s) => s.message.customType !== "cpd"), [], "nothing reaches the model");
});

test("ENOENT is the install in progress, not a change", async (t) => {
  const f = await started(t);
  rmSync(f.adapterDir, { recursive: true });
  await settle(200);
  assert.deepEqual(f.pi.userMessages, []);
  mkdirSync(f.adapterDir);
  writeFileSync(join(f.adapterDir, "extension.ts"), GOOD);
  writeFileSync(join(f.adapterDir, "package.json"), "{}");
  touch(f.adapterDir);
  await waitFor("the trigger", () => f.pi.userMessages.length > 0);
});

const conditions: [string, (f: Fixture) => void, (f: Fixture) => void][] = [
  ["pi is busy", (f) => (f.pi.idle = false), (f) => (f.pi.idle = true)],
  ["messages are pending", (f) => (f.pi.pendingMessages = true), (f) => (f.pi.pendingMessages = false)],
  ["text is in the editor", (f) => (f.pi.editorText = "unsent"), (f) => (f.pi.editorText = "")],
  ["an extension signalled herdr:blocked", (f) => f.pi.events.emit("herdr:blocked", { active: true, label: "dialog" }), (f) => f.pi.events.emit("herdr:blocked", { active: false })],
];

for (const [name, block, clear] of conditions) {
  test(`no reload while ${name}; a later tick retries`, async (t) => {
    const f = await started(t);
    block(f);
    install(f);
    await settle(200);
    assert.deepEqual(f.pi.userMessages, [], "refused at the trigger");
    assert.equal(f.pi.reloads, 0);
    clear(f);
    await waitFor("the reload", () => f.pi.reloads > 0);
    assert.equal(f.pi.userMessages.length, 1);
  });

  test(`the command itself refuses while ${name}, and the poll triggers again`, async (t) => {
    const f = await started(t);
    const handler = f.pi.commands.get(RELOAD_COMMAND)!;
    block(f);
    await handler("", f.pi.commandCtx());
    assert.equal(f.pi.reloads, 0);
    clear(f);
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
  await f.pi.fire("session_shutdown");
  install(f);
  await settle(200);
  assert.deepEqual(f.pi.userMessages, []);
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
    allowed: () => true,
    trigger: () => void calls.push("trigger"),
    onRefused: () => void calls.push("refused"),
    onError: (e) => void calls.push(`error ${e}`),
  });
  t.after(() => poll.stop());
  const tick = async () => {
    t.mock.timers.tick(10);
    await settle(0);
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

const refused: [string, Record<string, string>, string][] = [
  ["extension.ts is missing", { "package.json": "{}" }, "entry files missing"],
  ["package.json is missing", { "extension.ts": GOOD }, "entry files missing"],
  ["extension.ts throws when loaded", { "extension.ts": 'throw new Error("boom");\n', "package.json": "{}" }, "boom"],
  ["extension.ts has no default export function", { "extension.ts": "export default 3;\n", "package.json": "{}" }, "the default export of extension.ts is not a function"],
];

for (const [name, files, reason] of refused) {
  test(`no reload while ${name}: one visible error, retried at a later poll`, async (t) => {
    const f = await started(t);
    install(f, files);
    await waitFor("the error", () => f.pi.errorNotices.length > 0);
    await settle(400); // many polls
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
    await settle(200);
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
