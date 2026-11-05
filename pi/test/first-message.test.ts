import assert from "node:assert/strict";
import { test } from "node:test";
import { contextCursorFrom } from "../lib/updates.ts";
import { ev, Fixture, ID, OTHER, shown, waitFor } from "./fake.ts";

/** commands.md §15.7, "the first message of a by-user Session" (hs-d542 A3). */
const TEXT = "You are the Lead of Plan p1 \"T\". Follow the cpd-lead skill.\n\nContext ctx digest.";
const firstMessage = (event = 120) => ({ ok: true, session: ID, role: "lead", text: TEXT, file: "/run/p1/lead-message.md", event });
const firstMessageCalls = (f: Fixture) => f.argvs().filter((a) => a[0] === "session" && a[1] === "first-message");

async function lead(f: Fixture, over: Record<string, unknown> = {}) {
  f.respond("session-show", shown(over));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.respond("session-first-message", firstMessage());
  f.respond("events-list@context-record", { ok: true, events: [], next: 0 });
  f.respond("events-list@pr-opened", { ok: true, events: [], next: 0 });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

const run = async (f: Fixture) => ((await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" })) as any[]).flatMap((r) => (r?.message ? [r.message] : []));
const first = (ms: any[]) => ms.filter((m) => m.customType === "cpd-first-message");
const delivered = (event: number, to: string) => ({ type: "custom_message", customType: "cpd-first-message", content: "x", display: false, details: { event, delivered_to: to } });

test("a by-user Session's first run adds the first message hidden", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, { by_user: true });
  const ms = first(await run(f));
  assert.equal(ms.length, 1);
  assert.equal(ms[0].display, false);
  assert.equal(ms[0].content, TEXT);
  assert.deepEqual(ms[0].details, { event: 120, delivered_to: ID });
  assert.deepEqual(firstMessageCalls(f), [["session", "first-message", "--session", ID]]);
});

test("the first message is added once", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries = [delivered(120, ID)];
  await lead(f, { by_user: true });
  assert.deepEqual(first(await run(f)), []);
  assert.deepEqual(firstMessageCalls(f), []);

  // the fork-safe rule: a message delivered to another Session does not count, so this one still gets its own
  const g = new Fixture();
  t.after(() => g.close());
  g.pi.entries = [delivered(120, OTHER)];
  await lead(g, { by_user: true });
  assert.equal(first(await run(g)).length, 1);
});

test("a by-user Session queues turn-starting messages until the first prompt", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, { by_user: true });
  f.emit(ev(101, "job-merged", { data: { turn: true } }));
  const queued = await waitFor("the queued message", () => f.pi.sent.find((s) => (s.message.details as any)?.id === 101));
  assert.equal(queued.options.deliverAs, "nextTurn");
  assert.ok(!queued.options.triggerTurn, "no turn before the first prompt");

  const ms = await run(f);
  for (const m of ms) f.pi.entries.push({ type: "custom_message", ...m }); // pi keeps what the hook returned
  assert.equal(first(ms).length, 1);

  f.emit(ev(102, "job-merged", { data: { turn: true } }));
  const after = await waitFor("the next message", () => f.pi.sent.find((s) => (s.message.details as any)?.id === 102));
  assert.deepEqual(after.options, { triggerTurn: true, deliverAs: "followUp" }, "as today");
});

test("a by-user Session starts its first turn for a session-message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, { by_user: true });
  f.emit(ev(101, "session-message", { session: ID, text: "hello", data: { from: null, text: "hello", wake: false } }));
  await f.drain();
  const sm = f.pi.sent.find((s) => (s.message.details as any)?.id === 101);
  assert.ok(sm, "the session-message is sent");
  assert.deepEqual(sm.options, { triggerTurn: true, deliverAs: "steer" });

  f.emit(ev(102, "job-merged", { data: { turn: true } }));
  await f.drain();
  const other = f.pi.sent.find((s) => (s.message.details as any)?.id === 102);
  assert.ok(other, "the job-merged message is sent");
  assert.equal(other.options.deliverAs, "nextTurn");
  assert.ok(!other.options.triggerTurn, "every other Event stays queued");

  assert.equal(first(await run(f)).length, 1);
});

test("a Session not by-user gets no first message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, { by_user: false });
  assert.deepEqual(first(await run(f)), []);
  assert.deepEqual(firstMessageCalls(f), []);
  f.emit(ev(101, "job-merged", { data: { turn: true } }));
  const sent = await waitFor("the message", () => f.pi.sent.find((s) => (s.message.details as any)?.id === 101));
  assert.deepEqual(sent.options, { triggerTurn: true, deliverAs: "followUp" });

  // the same Session by-user does get one: the difference is the flag
  const g = new Fixture();
  t.after(() => g.close());
  await lead(g, { by_user: true });
  assert.equal(first(await run(g)).length, 1);
});

test("the first message advances the context cursor", () => {
  const fm = (event: number | undefined, to: string | undefined, customType = "cpd-first-message") => ({
    type: "custom_message", customType, content: "x", display: false, details: { ...(event !== undefined ? { event } : {}), ...(to !== undefined ? { delivered_to: to } : {}) },
  });
  assert.equal(contextCursorFrom(100, [fm(130, ID)], ID), 130);
  assert.equal(contextCursorFrom(100, [fm(130, ID), { ...fm(150, ID), customType: "cpd-context" }, fm(140, ID)], ID), 150, "the greatest of both kinds");
  assert.equal(contextCursorFrom(100, [fm(500, OTHER)], ID), 100, "a fork's file starts with its source's messages");
  assert.equal(contextCursorFrom(100, [fm(500, undefined)], ID), 100);
  assert.equal(contextCursorFrom(100, [fm(90, ID)], ID), 100, "never below registered_event");
});

test("a failed first-message read adds nothing and retries at the next run", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, { by_user: true });
  f.respond("session-first-message", { ok: false, error: { code: "store_busy", message: "database is locked" } }, { exit: 1 });
  assert.deepEqual(first(await run(f)), []);
  await waitFor("the report", () => f.reports().length > 0, 3000);
  await f.quiet();
  assert.deepEqual(f.reports().map((r) => [r.what, r.code, r.session]), [["first message", "store_busy", ID]], "reported once, to the Manager");
  assert.deepEqual(f.pi.errors(), [], "not delivered to the Lead's chat");
  assert.ok(f.adapterLog().some((l) => / error: first message: store_busy: /.test(l)), "logged");
  assert.equal(firstMessageCalls(f).length, 1);

  f.respond("session-first-message", firstMessage(130));
  const ms = first(await run(f));
  assert.equal(firstMessageCalls(f).length, 2, "retried at the next run");
  assert.equal(ms.length, 1);
  assert.deepEqual(ms[0].details, { event: 130, delivered_to: ID });
});
