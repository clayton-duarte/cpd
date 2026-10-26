import assert from "node:assert/strict";
import { test } from "node:test";
import { cursorFrom } from "../lib/cursor.ts";
import { ev, FakePi, Fixture, ID, OTHER, planShow, shown, type Sent, stateCalls, waitFor } from "./fake.ts";

const cpdMessage = (id: number | undefined, deliveredTo: string | undefined) => ({
  type: "custom_message",
  customType: "cpd",
  content: "x",
  display: true,
  details: { ...(id !== undefined ? { id } : {}), ...(deliveredTo !== undefined ? { delivered_to: deliveredTo } : {}) },
});

test("cursor: the greater of registered_event and the last message delivered to this Session", () => {
  assert.equal(cursorFrom(100, [], ID), 100);
  assert.equal(cursorFrom(100, [cpdMessage(90, ID)], ID), 100, "never at or below session-registered");
  assert.equal(cursorFrom(100, [cpdMessage(90, ID), cpdMessage(130, ID), cpdMessage(120, ID)], ID), 120, "the last one");
  assert.equal(cursorFrom(100, [cpdMessage(500, OTHER)], ID), 100, "a fork's file starts with its source's messages");
  assert.equal(cursorFrom(100, [cpdMessage(500, undefined)], ID), 100, "a message without delivered_to does not count");
  assert.equal(cursorFrom(100, [cpdMessage(undefined, ID)], ID), 100, "the adapter's own error message has no id");
  assert.equal(cursorFrom(100, [{ type: "custom_message", customType: "cpd-plan-decision", details: { id: 900, delivered_to: ID } }], ID), 100, "only customType cpd");
  assert.equal(cursorFrom(100, [{ type: "custom", customType: "cpd", data: { id: 900, delivered_to: ID } }, "junk", null], ID), 100);
});

test("start-up runs §15.2's steps in order and follows from the cursor", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries = [cpdMessage(500, OTHER), cpdMessage(90, ID), cpdMessage(130, ID)];
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  await f.trackerListed();

  assert.deepEqual(f.argvs(), [
    ["session", "show", ID],
    ["session", "state", "idle", "--session", ID],
    ["plan", "show", "--plan", "p1"],
    ["review", "list", "--session", ID],
  ]);
  assert.deepEqual(f.follows()[0]!.argv, ["events", "follow", "--session", ID, "--since", "130"]);
  assert.equal(f.pi.sessionName, "the lead", "step 3: the Session's name applies when pi's differs");
});

test("no replay at or below session-registered: a source's messages never lift the cursor", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries = [cpdMessage(500, OTHER)];
  f.respond("plan-show", planShow());
  f.emit(ev(99, "job-merged", { data: { turn: true } }), ev(100, "job-merged", { data: { turn: true } }), ev(101, "job-merged", { data: { turn: true } }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("event 101 delivered", () => f.pi.delivered().length >= 1);
  await f.drain(); // 99 and 100 were not printed, and what was is handled
  assert.deepEqual(f.sinces(), [100]);
  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [101]);
});

test("a working pi reports idle first, then working", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.pi.idle = false;
  f.load();
  await f.pi.fire("session_start");
  await waitFor("both state calls", () => stateCalls(f).length === 2);
  assert.deepEqual(stateCalls(f), ["idle", "working"]);
});

test("the name is left alone when it already matches", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.pi.sessionName = "the lead";
  let set = 0;
  const original = f.pi.setSessionName.bind(f.pi);
  f.pi.setSessionName = (n) => (set++, original(n));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  assert.equal(set, 0);
});

test("activation: only in a tui, only with CPD_SESSION, only when it is pi's session id", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.load();

  f.pi.mode = "rpc";
  await f.pi.fire("session_start");
  assert.deepEqual(f.calls(), [], "not a tui");

  f.pi.mode = "tui";
  delete process.env.CPD_SESSION;
  await f.pi.fire("session_start");
  assert.deepEqual(f.calls(), [], "a pi started by hand is left alone");
  assert.deepEqual(f.pi.sent, []);

  process.env.CPD_SESSION = ID;
  process.env.CPD_PLAN = "p1"; // a Lead's pane has CPD_PLAN (§6.5): this one is a Lead's, so its error is reported to the Manager (§15.1, hs-d573); the Fixture restores it
  f.pi.sessionId = OTHER;
  await f.pi.fire("session_start");
  await f.quiet();
  assert.deepEqual(f.argvs().map((a) => a.slice(0, 2)), [["session", "report"]], "inactive, so no state is reported: the one call is the report of the error");
  assert.deepEqual(f.pi.sent, [], "not delivered to the Lead's chat");
  const [r, ...more] = f.reports();
  assert.deepEqual(more, []);
  assert.deepEqual([r!.what, r!.code, r!.session], ["activation", "session_mismatch", ID]);
  assert.match(r!.message!, /^CPD_SESSION is /);
  assert.ok(f.adapterLog().some((l) => / error: activation: session_mismatch: /.test(l)), "logged");
});

test("a failing session show is delivered as an error and nothing else starts", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: false, error: { code: "session_not_found", message: "No such Session.", details: {} } }, { exit: 4 });
  f.load();
  await f.pi.fire("session_start");
  await f.quiet();
  assert.deepEqual(f.argvs(), [["session", "show", ID]], "a refusal is permanent: not retried");
  assert.equal(f.pi.sent.length, 1, "reported once");
  assert.equal(f.pi.sent[0]!.message.content, "φ adapter: session show: session_not_found: No such Session.");
  assert.equal((f.pi.sent[0]!.message.details as any).code, "session_not_found");
});

test("a session show that fails once then succeeds starts the adapter normally (pi-ci-W4)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: false, error: { code: "store_busy", message: "Store busy.", details: {} } }, { exit: 5 });
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the error", () => f.pi.errors().length === 1);
  assert.equal(f.pi.sent[0]!.message.content, "φ adapter: session show: store_busy: Store busy.");
  await waitFor("a retry", () => f.argvs().filter((a) => a[1] === "show").length >= 3);
  f.respond("session-show", shown());
  await waitFor("the follower", () => f.follows().length > 0);
  await f.trackerListed();
  assert.equal(f.pi.errors().length, 1, "the same code is reported once, however often it is retried");
  assert.deepEqual(f.argvs().slice(-3), [["session", "state", "idle", "--session", ID], ["plan", "show", "--plan", "p1"], ["review", "list", "--session", ID]]);
  assert.equal(f.pi.sessionName, "the lead");
});

test("session_shutdown ends the retry of a failing session show", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: false, error: { code: "store_busy", message: "Store busy.", details: {} } }, { exit: 5 });
  f.load();
  // The clock: a second adapter on the same fake `cpd`, retrying the same failing show with the same back-off and never shut down.
  // Each of its retries after the shutdown is a retry interval in which the first would have made a call.
  const witness = new FakePi();
  f.load({}, witness);
  try {
    await f.pi.fire("session_start");
    await witness.fire("session_start");
    await waitFor("a retry", () => f.pi.execOptions.length >= 2);
    await f.pi.fire("session_shutdown");
    await f.quiet();
    const n = f.pi.execOptions.length;
    const at = witness.execOptions.length;
    await waitFor("three more retries of the witness", () => witness.execOptions.length >= at + 3);
    await f.quiet();
    assert.equal(f.pi.execOptions.length, n, "no call after the shutdown");
  } finally {
    await witness.fire("session_shutdown");
    await waitFor("every witness cpd process to close", () => witness.childrenRunning.size === 0, 10_000);
  }
});

test("stdout that is not JSON is an error with a code", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", "boom, not json");
  f.load();
  await f.pi.fire("session_start");
  assert.match(f.pi.sent[0]!.message.content, /^φ adapter: session show: bad_output: /);
});

test("start-up: open Escalations of the Plan block the Lead before the first state", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow({ escalations: [{ id: "j-a1-q1" }, { id: "j-a2-q1" }] }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("blocked", () => stateCalls(f).length === 2);
  assert.deepEqual(stateCalls(f), ["idle", "waiting:φ: Worker question"]);
});

test("a Worker does not read the Plan", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "worker", plan: "p1", plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  assert.deepEqual(f.argvs().map((a) => a.slice(0, 2).join(" ")), ["session show", "session state"]);
  assert.deepEqual(f.pi.tools, [], "cpd_plan_review is for Plan Leads only");
});

test("a Manager reads no Plan, registers no plan-review tool, opens no plannotator and has no blocking reason", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.respond("plan-show", planShow({ escalations: [{ id: "j-a1-q1" }] }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(ev(101, "plan-review", { data: { review: "r1", status: "pending" } }), ev(102, "session-renamed", { session: ID, data: { name: "ctx-manager" } }));
  await waitFor("the rename", () => f.pi.sessionName === "ctx-manager");
  await f.quiet(); // the rename is the later Event: the plan-review before it was handled
  assert.deepEqual(f.argvs().map((a) => a.slice(0, 2).join(" ")), ["session show", "session state", "review list"], "no plan show");
  assert.deepEqual(stateCalls(f), ["idle"], "never blocked");
  assert.deepEqual(f.pi.tools, []);
  assert.deepEqual(f.pi.emitted, [], "nothing is asked of plannotator");
  assert.deepEqual(f.pi.delivered(), []);
});

test("a hanging cpd call ends at the timeout and is reported as a failed call (pi-ci-W5)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown(), { delay: 10_000 });
  f.load({ timing: { callTimeoutMs: 300 } });
  const began = Date.now();
  await f.pi.fire("session_start"); // returns when `session show` has failed
  assert.ok(Date.now() - began < 5000, "session_start was not held by the hang");
  const [e] = f.pi.errors();
  assert.equal((e!.message.details as any).code, "tool_timeout");
  assert.match(e!.message.content, /session show/);
  assert.deepEqual(f.follows(), [], "a failed start follows nothing");
});

test("every cpd call carries pi's timeout and the abort signal of the run, aborted at session_shutdown (pi-ci-W5)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  const calls = f.pi.execOptions.filter((c) => c.args[0] !== "events");
  assert.ok(calls.length >= 3);
  for (const c of calls) assert.equal(c.options.timeout, 10_000, c.args.join(" "));
  const show = calls.find((c) => c.args[0] === "session" && c.args[1] === "show")!;
  assert.ok(show.options.signal instanceof AbortSignal);
  assert.equal(show.options.signal.aborted, false);
  await f.pi.fire("session_shutdown");
  assert.equal(show.options.signal.aborted, true);
});

test("a call in flight at session_shutdown is aborted, not waited for, and reported as failed (pi-ci-W5)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown(), { delay: 10_000 });
  f.load();
  const starting = f.pi.fire("session_start");
  await waitFor("session show running", () => f.pi.execsRunning.size > 0);
  await f.pi.fire("session_shutdown");
  const began = Date.now();
  await starting;
  assert.ok(Date.now() - began < 5000, "the call was aborted, not waited for");
  assert.equal((f.pi.errors()[0]!.message.details as any).code, "aborted");
});
