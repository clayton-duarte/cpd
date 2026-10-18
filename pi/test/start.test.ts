import assert from "node:assert/strict";
import { test } from "node:test";
import { cursorFrom } from "../lib/cursor.ts";
import { ev, Fixture, ID, OTHER, planShow, sessionShow, settle, type Sent, stateCalls, waitFor } from "./fake.ts";

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

  assert.deepEqual(f.argvs(), [
    ["session", "show", ID],
    ["session", "state", "idle", "--session", ID],
    ["plan", "show", "--plan", "p1"],
  ]);
  assert.deepEqual(f.follows()[0]!.argv, ["events", "follow", "--session", ID, "--since", "130"]);
  assert.equal(f.pi.sessionName, "the lead", "step 3: the Session's name applies when pi's differs");
});

test("no replay at or below session-registered: a source's messages never lift the cursor", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries = [cpdMessage(500, OTHER)];
  f.respond("plan-show", planShow());
  f.emit(ev(99, "job-merged"), ev(100, "job-merged"), ev(101, "job-merged"));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("event 101 delivered", () => f.pi.delivered().length >= 1);
  await settle();
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
  f.pi.sessionId = OTHER;
  await f.pi.fire("session_start");
  await settle();
  assert.deepEqual(f.calls(), [], "inactive: it would report another Session's state");
  assert.equal(f.pi.sent.length, 1);
  const m: Sent = f.pi.sent[0]!;
  assert.match(m.message.content, /^cpd adapter: activation: session_mismatch: /);
  assert.deepEqual(m.options, { triggerTurn: true, deliverAs: "followUp" });
  assert.equal((m.message.details as any).kind, "error");
});

test("a failing session show is delivered as an error and nothing else starts", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: false, error: { code: "session_not_found", message: "No such Session.", details: {} } }, { exit: 4 });
  f.load();
  await f.pi.fire("session_start");
  await settle();
  await settle(400); // more than the restart delays of the fast timing
  assert.deepEqual(f.argvs(), [["session", "show", ID]], "a refusal is permanent: not retried");
  assert.equal(f.pi.sent.length, 1, "reported once");
  assert.equal(f.pi.sent[0]!.message.content, "cpd adapter: session show: session_not_found: No such Session.");
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
  assert.equal(f.pi.sent[0]!.message.content, "cpd adapter: session show: store_busy: Store busy.");
  await waitFor("a retry", () => f.argvs().filter((a) => a[1] === "show").length >= 3);
  f.respond("session-show", sessionShow());
  await waitFor("the follower", () => f.follows().length > 0);
  assert.equal(f.pi.errors().length, 1, "the same code is reported once, however often it is retried");
  assert.deepEqual(f.argvs().slice(-2), [["session", "state", "idle", "--session", ID], ["plan", "show", "--plan", "p1"]]);
  assert.equal(f.pi.sessionName, "the lead");
});

test("session_shutdown ends the retry of a failing session show", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: false, error: { code: "store_busy", message: "Store busy.", details: {} } }, { exit: 5 });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("a retry", () => f.argvs().length >= 2);
  await f.pi.fire("session_shutdown");
  await settle(100);
  const n = f.argvs().length;
  await settle(300);
  assert.equal(f.argvs().length, n, "no call after the shutdown");
});

test("stdout that is not JSON is an error with a code", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", "boom, not json");
  f.load();
  await f.pi.fire("session_start");
  assert.match(f.pi.sent[0]!.message.content, /^cpd adapter: session show: bad_output: /);
});

test("start-up: open Escalations of the Plan block the Lead before the first state", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow({ escalations: [{ id: "j-a1-q1" }, { id: "j-a2-q1" }] }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("blocked", () => stateCalls(f).length === 2);
  assert.deepEqual(stateCalls(f), ["idle", "blocked:cpd: Worker question"]);
});

test("a Worker does not read the Plan", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", sessionShow({ role: "worker", plan: "p1", plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  assert.deepEqual(f.argvs().map((a) => a.slice(0, 2).join(" ")), ["session show", "session state"]);
  assert.deepEqual(f.pi.tools, [], "cpd_plan_review is for Plan Leads only");
});

test("a Manager reads no Plan, registers no plan-review tool, opens no plannotator and has no blocking reason", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", sessionShow({ role: "manager", plan: null, plan_lead: false }));
  f.respond("plan-show", planShow({ escalations: [{ id: "j-a1-q1" }] }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(ev(101, "plan-review", { data: { review: "r1", status: "pending" } }), ev(102, "session-renamed", { session: ID, data: { name: "ctx-manager" } }));
  await waitFor("the rename", () => f.pi.sessionName === "ctx-manager");
  await settle();
  assert.deepEqual(f.argvs().map((a) => a.slice(0, 2).join(" ")), ["session show", "session state"], "no plan show");
  assert.deepEqual(stateCalls(f), ["idle"], "never blocked");
  assert.deepEqual(f.pi.tools, []);
  assert.deepEqual(f.pi.emitted, [], "nothing is asked of plannotator");
  assert.deepEqual(f.pi.delivered(), []);
});
