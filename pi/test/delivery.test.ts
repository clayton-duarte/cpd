import assert from "node:assert/strict";
import { test } from "node:test";
import { KINDS } from "../lib/event.ts";
import { ev, Fixture, ID, OTHER, planShow, sessionShow, settle, stateCalls, waitFor } from "./fake.ts";

const DATA: Record<string, Record<string, unknown>> = {
  escalation: { escalation: "j-a1-q1", question: "q?" },
  "escalation-answered": { escalation: "j-a1-q1" },
  "escalation-expired": { escalation: "j-a1-q1", reason: "timeout" },
  "plan-review": { review: "r1", fingerprint: "ff", status: "approved" },
  "session-renamed": { name: "new name" },
  "attempt-nudge": { prompt: "Submit your work." },
  "session-message": { from: null, text: "hello", wake: false },
};
const event = (id: number, kind: string, over: Record<string, unknown> = {}) => ev(id, kind, { data: DATA[kind] ?? {}, ...over });

// commands.md §15.3: message and turn, per kind, for a Plan Lead.
const LEAD: Record<string, "turn" | "no turn"> = {
  "job-merged": "turn",
  "job-failed": "turn",
  "all-merged": "turn",
  "pr-feedback": "turn",
  "plan-done": "turn",
  error: "turn",
  escalation: "turn",
  "attempt-retry": "no turn",
  "job-cancelled": "no turn",
  "pr-started": "no turn",
  "pr-merged": "no turn",
};

async function started(f: Fixture, role: "lead" | "worker") {
  f.respond("plan-show", planShow());
  if (role === "worker") f.respond("session-show", sessionShow({ role: "worker", plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

test("Plan Lead: every kind of §15.3's table", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");

  const expected: number[] = [];
  let id = 101;
  for (const kind of KINDS) {
    f.emit(event(id, kind));
    if (kind in LEAD) expected.push(id);
    id++;
  }
  await waitFor("the last delivery", () => f.pi.delivered().length === expected.length);
  await settle();

  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), expected);
  for (const s of f.pi.delivered()) {
    const d = s.message.details as any;
    assert.equal(s.message.customType, "cpd");
    assert.equal(s.message.display, true);
    assert.equal(s.message.content, `text of ${d.kind} ${d.id}`, "content is the Event's text");
    assert.equal(d.delivered_to, ID);
    assert.deepEqual(s.options, { triggerTurn: LEAD[d.kind] === "turn", deliverAs: "followUp" }, d.kind);
  }
  // Silent kinds: no message, no turn, nothing else sent.
  assert.equal(f.pi.sent.length, expected.length);
});

test("Plan Lead: the details are the whole Event plus delivered_to", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  const e = ev(101, "job-merged", { job: "j", attempt: "j-a1", data: { sha: "c0ffee1", tier: "O3" } });
  f.emit(e);
  await waitFor("delivery", () => f.pi.delivered().length === 1);
  assert.deepEqual(f.pi.delivered()[0]!.message.details, { ...e, delivered_to: ID });
});

test("Plan Lead: escalation opens `escalation:<id>`, answered and expired close it, replays change nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  await waitFor("idle", () => stateCalls(f).length === 1);

  f.emit(event(101, "escalation"), event(102, "escalation", { text: "again" }));
  // The two lines may be read in different polls: `blocked` follows the first, the second is delivered on its own time.
  await waitFor("both escalations delivered", () => f.pi.delivered().length === 2);
  await waitFor("blocked", () => stateCalls(f).length === 2);
  assert.deepEqual(stateCalls(f), ["idle", "blocked:cpd: Worker question"]);
  assert.equal(f.pi.delivered().length, 2, "both are messages with a turn");

  f.emit(event(103, "escalation-answered"), event(104, "escalation-answered"));
  await waitFor("idle again", () => stateCalls(f).length === 3);
  await settle();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:cpd: Worker question", "idle"]);
  assert.equal(f.pi.delivered().length, 2, "answered is no message");

  // A state change wanted while a call is in flight replaces the one wanted before: let the last call finish first.
  await waitFor("no call in flight", () => f.pi.execsRunning.size === 0);
  f.emit(event(105, "escalation", { data: { escalation: "j-a1-q2", question: "?" } }), event(106, "escalation-expired", { data: { escalation: "j-a1-q2", reason: "timeout" } }));
  await waitFor("blocked and closed", () => stateCalls(f).length === 5);
  assert.deepEqual(stateCalls(f).slice(3), ["blocked:cpd: Worker question", "idle"]);
  assert.equal(f.pi.delivered().length, 3, "expired is no message");
});

test("session-renamed for itself: setSessionName and a notice, no message, no turn, no echo to cpd", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "session-renamed", { session: ID }), event(102, "session-renamed", { session: OTHER, data: { name: "not mine" } }));
  await waitFor("the name", () => f.pi.sessionName === "new name");
  await f.pi.fire("session_info_changed", { name: "new name" });
  await settle();
  assert.deepEqual(f.pi.notices, ['cpd: session renamed to "new name"']);
  assert.deepEqual(f.pi.sent, []);
  assert.equal(f.argvs().some((a) => a[1] === "rename"), false, "the session-renamed it causes comes back and changes nothing");
});

test("Worker: attempt-nudge for itself is a message with data.prompt and a turn; session-renamed applies; the rest is ignored", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "worker");

  let id = 101;
  for (const kind of KINDS) {
    if (kind === "attempt-nudge") f.emit(event(id++, kind, { session: OTHER }), event(id++, kind, { session: ID }));
    else if (kind === "session-renamed") f.emit(event(id++, kind, { session: ID }));
    else f.emit(event(id++, kind));
  }
  await waitFor("the nudge", () => f.pi.delivered().length === 1);
  await waitFor("the rename", () => f.pi.sessionName === "new name");
  await settle();

  assert.equal(f.pi.sent.length, 1);
  const s = f.pi.delivered()[0]!;
  assert.equal(s.message.content, "Submit your work.");
  assert.equal((s.message.details as any).kind, "attempt-nudge");
  assert.equal((s.message.details as any).delivered_to, ID);
  assert.deepEqual(s.options, { triggerTurn: true, deliverAs: "followUp" });
  assert.deepEqual(f.pi.notices, ['cpd: session renamed to "new name"']);
});

test("a message delivered to this Session is what a restart's cursor reads back", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "job-merged"), event(102, "attempt-retry"));
  await waitFor("two messages", () => f.pi.delivered().length === 2);
  await f.pi.fire("session_shutdown");

  // pi resumes the same session file: its entries hold the delivered messages, and the follower starts again.
  await f.pi.fire("session_start");
  await waitFor("the second follower", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 102]);
});

const message = (id: number, from: string | null, over: Record<string, unknown> = {}) =>
  event(id, "session-message", { session: ID, text: "ship it", data: { from, text: "ship it", wake: false }, ...over });

test("session-message for this Lead: one cpd message with no turn, followUp, from the sender's name and id", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond(`session-show-${OTHER}`, { ok: true, session: { id: OTHER, name: "the manager" } });
  f.emit(message(101, OTHER));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await settle();

  assert.equal(f.pi.sent.length, 1);
  const s = f.pi.delivered()[0]!;
  assert.equal(s.message.customType, "cpd");
  assert.equal(s.message.content, `Message from the manager (${OTHER}): ship it`);
  assert.equal((s.message.details as any).delivered_to, ID);
  assert.equal((s.message.details as any).id, 101);
  assert.deepEqual(s.options, { triggerTurn: false, deliverAs: "followUp" });
  assert.equal(f.argvs().filter((a) => a[1] === "show" && a[2] === OTHER).length, 1, "the sender is read once");
});

test("session-message: a turn only when data.wake is true; a missing wake is false (hs-d209, D3)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(message(101, null, { data: { from: null, text: "ship it", wake: true } }), message(102, null, { data: { from: null, text: "ship it" } }), message(103, null, { data: { from: null, text: "ship it", wake: "yes" } }));
  await waitFor("three messages", () => f.pi.delivered().length === 3);
  assert.deepEqual(
    f.pi.delivered().map((s) => s.options),
    [{ triggerTurn: true, deliverAs: "followUp" }, { triggerTurn: false, deliverAs: "followUp" }, { triggerTurn: false, deliverAs: "followUp" }],
  );
});

test("session-message: from null is the User; an unreadable sender is named by its id; a message for another Session is ignored", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond("session-show-gone", { ok: false, error: { code: "session_not_found", message: "no such session" } }, { exit: 4 });
  f.emit(message(101, null), message(102, "gone"), message(103, null, { session: OTHER }));
  await waitFor("two messages", () => f.pi.delivered().length === 2);
  await settle();
  assert.equal(f.pi.delivered()[0]!.message.content, "Message from the User: ship it");
  assert.equal(f.pi.delivered()[1]!.message.content, "Message from gone: ship it");
  assert.equal(f.pi.sent.length, 2);
});

test("session-message arrives in id order though the sender is read first", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond(`session-show-${OTHER}`, { ok: true, session: { id: OTHER, name: "slow" } }, { delay: 100 });
  f.emit(message(101, OTHER), event(102, "job-merged"));
  await waitFor("both", () => f.pi.delivered().length === 2);
  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [101, 102]);
});

test("session-message reaches a side fork (role lead, not the Plan's Lead) too", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", sessionShow({ plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(message(101, null), event(102, "job-merged"));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await settle();
  assert.equal(f.pi.sent.length, 1, "the Plan's Events are not its");
  assert.deepEqual(f.pi.delivered()[0]!.options, { triggerTurn: false, deliverAs: "followUp" });
});

test("session-message reaches a Manager (no Plan) too", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", sessionShow({ role: "manager", plan: null, plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(message(101, null));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  assert.deepEqual(f.pi.delivered()[0]!.options, { triggerTurn: false, deliverAs: "followUp" });
});

test("a Manager's session-message with wake starts a turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", sessionShow({ role: "manager", plan: null, plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(message(101, null, { data: { from: null, text: "ship it", wake: true } }));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  assert.deepEqual(f.pi.delivered()[0]!.options, { triggerTurn: true, deliverAs: "followUp" });
});

test("a Worker ignores session-message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "worker");
  f.emit(message(101, null));
  await settle();
  assert.deepEqual(f.pi.sent, []);
});
