import assert from "node:assert/strict";
import { mkdirSync, rmSync, utimesSync } from "node:fs";
import { test } from "node:test";
import { refusal, RELOAD_COMMAND, type Guard } from "../lib/reload.ts";
import { Fixture, planShow, settle, waitFor } from "./fake.ts";

let installs = 0;
/** What `scripts/install-cpd.sh` does to the adapter directory: replaced as a whole (new mtime even if the inode is reused). */
const install = (f: Fixture) => {
  rmSync(f.adapterDir, { recursive: true });
  mkdirSync(f.adapterDir);
  const t = new Date(Date.UTC(2030, 0, 1) + ++installs * 1000);
  utimesSync(f.adapterDir, t, t);
};

/** A started adapter whose `ctx.reload()` behaves like pi's: shutdown, then start of the new load. */
async function started(t: { after(fn: () => Promise<void>): void }): Promise<Fixture> {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.pi.onReload = async () => {
    await f.pi.fire("session_shutdown", { reason: "reload" });
    await f.pi.fire("session_start", { reason: "reload" });
  };
  f.load();
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
  utimesSync(f.adapterDir, new Date(Date.UTC(2031, 0, 1)), new Date(Date.UTC(2031, 0, 1)));
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
