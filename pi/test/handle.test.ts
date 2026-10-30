import assert from "node:assert/strict";
import { test } from "node:test";
import { ev, Fixture, ID, planShow, shown, waitFor } from "./fake.ts";

// commands.md §15.2: a `store_too_new` failure line of a follower started before an install is not delivered (the restart runs the new cpd).

const failure = JSON.stringify({ ok: false, error: { code: "store_too_new", message: "store_too_new happened.", details: { x: 1 } } });
/** A Lead's adapter errors are reported to the Manager, never delivered to its chat (commands.md §15.1, hs-d573). */
const errorsOf = (f: Fixture) => f.reports().filter((r) => r.code === "store_too_new");

async function started(f: Fixture, delays: number[]) {
  f.respond("plan-show", planShow());
  f.load({ schedule: (run, ms) => (delays.push(ms), setTimeout(run, 0)) });
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

test("store_too_new from a follower started before an install is not delivered and the follower restarts", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const delays: number[] = [];
  await started(f, delays);
  f.idLessOnce(); // the new cpd no longer fails: the restarted follower does not print the line again
  f.reinstall(); // the run started with identity A; the installed cpd is now B
  f.emit(failure);
  f.exitNext(3); // the old cpd prints the line and exits
  await waitFor("the restart", () => f.follows().length === 2);
  await f.quiet();
  assert.deepEqual(errorsOf(f), [], "nothing is reported");
  assert.equal(f.pi.errors().length, 0, "the exit that follows adds nothing");
  assert.deepEqual(delays, [30], "restarted after the initial delay");
});

test("store_too_new with the same installed cpd is still reported once", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const delays: number[] = [];
  await started(f, delays);
  f.emit(failure);
  f.exitNext(3);
  await waitFor("the restart", () => f.follows().length === 2);
  await waitFor("the error", () => errorsOf(f).length === 1);
  f.emit(failure);
  f.exitNext(3);
  await waitFor("a third start", () => f.follows().length === 3);
  await f.quiet();
  assert.equal(errorsOf(f).length, 1, "once per code");
  assert.deepEqual(f.pi.errors(), [], "reported to the Manager, not delivered to the Lead's chat");
});

// commands.md §15.14: the mention ids are reloaded only on an Event that changes them (hs-n423), and at `session_start`.
const lists = (f: Fixture) => f.calls().filter((c) => c.phase === "start" && c.argv[1] === "list" && ["session", "plan", "context"].includes(c.argv[0]!)).map((c) => c.argv[0]!);

async function leadStarted(f: Fixture) {
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  await f.quiet();
  assert.deepEqual(lists(f).sort(), ["context", "plan", "session"], "session_start refreshes the ids once");
}

test("mentions: a job-merged Event runs no refresh", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await leadStarted(f);
  f.emit(ev(101, "job-merged", { data: { turn: true } }));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await f.quiet();
  assert.deepEqual(lists(f).sort(), ["context", "plan", "session"], "no list call after session_start");
});

test("mentions: a session-message Event runs no refresh", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await leadStarted(f);
  f.emit(ev(101, "session-message", { session: ID, data: { from: null, text: "ship it", wake: false } }));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await f.quiet();
  assert.deepEqual(lists(f).sort(), ["context", "plan", "session"], "no list call after session_start");
});

test("mentions: a plan-created Event refreshes the ids", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await leadStarted(f);
  f.emit(ev(101, "plan-created"));
  await waitFor("the three list calls", () => lists(f).length === 6);
  await f.quiet();
  assert.deepEqual(lists(f).slice(3).sort(), ["context", "plan", "session"], "once each");
});

test("a Lead is delivered an Event id once when the follower prints it twice", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  await f.pi.busy();

  const feedback = ev(101, "pr-feedback", { data: { reason: "checks-failed" } });
  const merged = ev(102, "job-merged", { data: { turn: true } });
  const message = ev(103, "session-message", { session: ID, data: { from: null, text: "ship it", wake: false } });
  const other = ev(104, "job-merged", { data: { turn: true } });
  f.emit(feedback, feedback, merged, merged, message, message, other);
  await f.drain();

  const sent = f.pi.delivered();
  assert.deepEqual(sent.map((s) => (s.message.details as any).id), [101, 102, 103, 104]);
  assert.deepEqual(sent[0]!.options, { triggerTurn: true, deliverAs: "steer" });
  assert.deepEqual(sent[1]!.options, { triggerTurn: true, deliverAs: "followUp" });
});

test("a Worker is delivered an attempt-nudge id once when the follower prints it twice", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown({ role: "worker", plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();

  const nudge = ev(101, "attempt-nudge", { session: ID, data: { prompt: "Submit your work.", turns: 1 } });
  f.emit(nudge, nudge);
  await f.drain(nudge2);

  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [101]);
});

const nudge2 = (id: number) => ev(id, "attempt-nudge", { session: ID, data: { prompt: "Submit your work.", turns: 1 } });
