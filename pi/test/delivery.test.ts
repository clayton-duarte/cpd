import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { CpdError } from "../lib/cpd.ts";
import { KINDS } from "../lib/event.ts";
import { createHost } from "../lib/host.ts";
import { Run } from "../lib/run.ts";
import { ev, Fixture, ID, OTHER, planShow, sessionShow, shown, stateCalls, waitFor } from "./fake.ts";

// Pin the process timezone so the expected local times are deterministic and prove the rendering is local, not UTC.
process.env.TZ = "Asia/Kolkata";

const DATA: Record<string, Record<string, unknown>> = {
  "job-merged": { turn: true },
  escalation: { escalation: "j-a1-q1", question: "q?" },
  "escalation-answered": { escalation: "j-a1-q1" },
  "escalation-expired": { escalation: "j-a1-q1", reason: "timeout" },
  "plan-review": { review: "r1", fingerprint: "ff", status: "approved" },
  "session-renamed": { name: "new name" },
  "attempt-nudge": { prompt: "Submit your work." },
  "session-message": { from: null, text: "hello", wake: false },
};
const event = (id: number, kind: string, over: Record<string, unknown> = {}) => ev(id, kind, { data: DATA[kind] ?? {}, ...over });

// commands.md §15.3: the kinds that are a message with a turn for a Plan Lead; every other kind is no message (hs-d514).
const LEAD: Record<string, "turn"> = {
  "job-merged": "turn",
  "job-failed": "turn",
  "pr-feedback": "turn",
  "plan-done": "turn",
  error: "turn",
  escalation: "turn",
};

/** Last Events of a `drain` for a Worker (which only reads its own nudge) and for a Session that only reads its own messages. */
const nudge = (id: number) => event(id, "attempt-nudge", { session: ID });
const asMessage = (id: number) => message(id, null);

async function started(f: Fixture, role: "lead" | "worker") {
  f.respond("plan-show", planShow());
  if (role === "worker") f.respond("session-show", shown({ role: "worker", plan_lead: false }));
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
  await f.drain(); // every Event above was handled

  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), expected);
  for (const s of f.pi.delivered()) {
    const d = s.message.details as any;
    assert.equal(s.message.customType, "cpd");
    assert.equal(s.message.display, true);
    assert.equal(s.message.content, `text of ${d.kind} ${d.id}`, "content is the Event's text");
    assert.equal(d.delivered_to, ID);
    const urgent = d.kind === "pr-feedback" || d.kind === "escalation";
    assert.deepEqual(s.options, { triggerTurn: LEAD[d.kind] === "turn", deliverAs: urgent ? "steer" : "followUp" }, d.kind);
  }
  // Silent kinds: no message, no turn, nothing else sent.
  assert.equal(f.pi.sent.length, expected.length);
});

test("job-merged with data.turn false adds no message and starts no turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "job-merged", { data: { turn: false } }));
  await f.drain();
  assert.deepEqual(f.pi.delivered(), []);
  assert.deepEqual(f.pi.sent, []);
});

test("job-merged with data.turn true starts a turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "job-merged", { data: { turn: false } }), event(102, "job-merged", { data: { turn: true } }));
  await waitFor("a delivery", () => f.pi.delivered().length >= 1);
  await f.drain();
  assert.deepEqual(f.pi.delivered().map((s) => [(s.message.details as any).id, s.options]), [[102, { triggerTurn: true, deliverAs: "followUp" }]]);
});

test("workflow-started, pr-merged, workflow-base-changed, job-cancelled and attempt-retry add no message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(...["workflow-started", "pr-merged", "workflow-base-changed", "job-cancelled", "attempt-retry"].map((kind, i) => event(101 + i, kind)));
  await f.drain();
  assert.deepEqual(f.pi.delivered(), []);
  assert.deepEqual(f.pi.sent, []);
});

test("Plan Lead: a busy Lead gets pr-feedback (every reason) and escalation steered, job-merged as a follow-up, and an old all-merged nothing; each still triggers a turn (hs-d276)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  await f.pi.busy();
  const reasons = ["checks-failed", "changes-requested", "closed", "conflict"];
  f.emit(...reasons.map((reason, i) => event(101 + i, "pr-feedback", { data: { reason } })), event(105, "escalation"), event(106, "all-merged"), event(107, "job-merged", { data: { turn: true } }));
  await waitFor("six deliveries", () => f.pi.delivered().length === 6);
  assert.deepEqual(
    f.pi.delivered().map((s) => [(s.message.details as any).kind, s.options]),
    [
      ...reasons.map((): [string, unknown] => ["pr-feedback", { triggerTurn: true, deliverAs: "steer" }]),
      ["escalation", { triggerTurn: true, deliverAs: "steer" }],
      ["job-merged", { triggerTurn: true, deliverAs: "followUp" }],
    ],
  );
});

test("Plan Lead: the details are the whole Event plus delivered_to", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  const e = ev(101, "job-merged", { job: "j", attempt: "j-a1", data: { sha: "c0ffee1", tier: "O3", turn: true } });
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
  await waitFor("waiting", () => stateCalls(f).length === 2);
  assert.deepEqual(stateCalls(f), ["idle", "waiting:φ: Worker question"]);
  assert.equal(f.pi.delivered().length, 2, "both are messages with a turn");

  f.emit(event(103, "escalation-answered"), event(104, "escalation-answered"));
  await waitFor("idle again", () => stateCalls(f).length === 3);
  await f.drain();
  assert.deepEqual(stateCalls(f), ["idle", "waiting:φ: Worker question", "idle"]);
  assert.equal(f.pi.delivered().length, 2, "answered is no message");

  // A state change wanted while a call is in flight replaces the one wanted before: let the last call finish first.
  await waitFor("no call in flight", () => f.pi.execsRunning.size === 0);
  f.emit(event(106, "escalation", { data: { escalation: "j-a1-q2", question: "?" } }), event(107, "escalation-expired", { data: { escalation: "j-a1-q2", reason: "timeout" } }));
  await waitFor("blocked and closed", () => stateCalls(f).length === 5);
  assert.deepEqual(stateCalls(f).slice(3), ["waiting:φ: Worker question", "idle"]);
  assert.equal(f.pi.delivered().length, 3, "expired is no message");
});

test("session-renamed for itself: setSessionName and a notice, no message, no turn, no echo to cpd", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "session-renamed", { session: ID }), event(102, "session-renamed", { session: OTHER, data: { name: "not mine" } }));
  await waitFor("the name", () => f.pi.sessionName === "new name");
  await f.pi.fire("session_info_changed", { name: "new name" });
  await f.drain();
  assert.deepEqual(f.pi.notices, ['\x1b[1mφ\x1b[22m: session renamed to "new name"']);
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
    else if (kind === "mcp-attached") f.emit(event(id++, kind, { session: OTHER, data: { name: "slack", server: { url: "https://mcp.slack.com/mcp" } } }));
    else f.emit(event(id++, kind));
  }
  await waitFor("the nudge", () => f.pi.delivered().length === 1);
  await waitFor("the rename", () => f.pi.sessionName === "new name");
  await f.drain(nudge); // handled in order: what is not delivered by now is ignored

  assert.equal(f.pi.sent.length, 1);
  const s = f.pi.delivered()[0]!;
  assert.equal(s.message.content, "Submit your work.");
  assert.equal((s.message.details as any).kind, "attempt-nudge");
  assert.equal((s.message.details as any).delivered_to, ID);
  assert.deepEqual(s.options, { triggerTurn: true, deliverAs: "followUp" });
  assert.deepEqual(f.pi.notices, ['\x1b[1mφ\x1b[22m: session renamed to "new name"']);
});

test("a message delivered to this Session is what a restart's cursor reads back", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "job-merged", { data: { turn: true } }), event(102, "job-failed"));
  await waitFor("two messages", () => f.pi.delivered().length === 2);
  await f.pi.fire("session_shutdown");

  // pi resumes the same session file: its entries hold the delivered messages, and the follower starts again.
  await f.pi.fire("session_start");
  await waitFor("the second follower", () => f.follows().length === 2);
  assert.deepEqual(f.sinces(), [100, 102]);
});

const message = (id: number, from: string | null, over: Record<string, unknown> = {}) =>
  event(id, "session-message", { session: ID, text: "ship it", data: { from, text: "ship it", wake: false }, ...over });

test("session-message for this Lead: one cpd message that starts a turn and steers, from the sender name and id", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond(`session-show-${OTHER}`, { ok: true, session: { id: OTHER, name: "the manager" } });
  f.emit(message(101, OTHER));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await f.drain();

  assert.equal(f.pi.sent.length, 1);
  const s = f.pi.delivered()[0]!;
  assert.equal(s.message.customType, "cpd");
  assert.equal(s.message.content, `Message from the manager (${OTHER}) (03:11 +05:30): ship it`);
  assert.equal((s.message.details as any).delivered_to, ID);
  assert.equal((s.message.details as any).id, 101);
  assert.deepEqual(s.options, { triggerTurn: true, deliverAs: "steer" });
  assert.equal(f.argvs().filter((a) => a[1] === "show" && a[2] === OTHER).length, 1, "the sender is read once");
});

test("every session-message starts a turn and steers, whatever data wake", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(
    message(101, null, { data: { from: null, text: "ship it", wake: true } }),
    message(102, null, { data: { from: null, text: "ship it", wake: false } }),
    message(103, null, { data: { from: null, text: "ship it" } }),
    message(104, null, { data: { from: null, text: "ship it", wake: "yes" } }),
  );
  await waitFor("four messages", () => f.pi.delivered().length === 4);
  assert.deepEqual(
    f.pi.delivered().map((s) => s.options),
    Array(4).fill({ triggerTurn: true, deliverAs: "steer" }),
  );
});

test("session-message: from null is the User; an unreadable sender is named by its id; a message for another Session is ignored", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond("session-show-gone", { ok: false, error: { code: "session_not_found", message: "no such session" } }, { exit: 4 });
  f.emit(message(101, null), message(102, "gone"), message(103, null, { session: OTHER }));
  await waitFor("two messages", () => f.pi.delivered().length === 2);
  await f.drain();
  assert.equal(f.pi.delivered()[0]!.message.content, "Message from the User (03:11 +05:30): ship it");
  assert.equal(f.pi.delivered()[1]!.message.content, "Message from gone (03:11 +05:30): ship it");
  assert.equal(f.pi.sent.length, 2);
});

test("session-message arrives in id order though the sender is read first", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond(`session-show-${OTHER}`, { ok: true, session: { id: OTHER, name: "slow" } }, { delay: 100 });
  f.emit(message(101, OTHER), event(102, "job-failed"));
  await waitFor("both", () => f.pi.delivered().length === 2);
  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [101, 102]);
});

test("session-message reaches a side fork too, steered", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(message(101, null), event(102, "job-merged"));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await f.drain(asMessage);
  assert.equal(f.pi.sent.length, 1, "the Plan's Events are not its");
  assert.deepEqual(f.pi.delivered()[0]!.options, { triggerTurn: true, deliverAs: "steer" });
});

test("session-message reaches a Manager without a Plan too, steered", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(message(101, null));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  assert.deepEqual(f.pi.delivered()[0]!.options, { triggerTurn: true, deliverAs: "steer" });
});

test("a Manager session-message starts a turn, steered", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(message(101, null, { data: { from: null, text: "ship it", wake: true } }));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  assert.deepEqual(f.pi.delivered()[0]!.options, { triggerTurn: true, deliverAs: "steer" });
});

test("a Worker ignores session-message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "worker");
  f.emit(message(101, null));
  await f.drain(nudge);
  assert.deepEqual(f.pi.sent, []);
});

/** commands.md §15.1 "Errors" (hs-d552 G1, hs-d573): `host.deliver` on a real Host and a Lead's Run, with a fake pi and the fake `cpd` of the Fixture: a Lead's error is reported to the Manager, never delivered. */
async function errorHost(t: { after(fn: () => unknown): void }) {
  const f = new Fixture();
  t.after(() => f.close());
  const pi = f.pi;
  const host = createHost(pi, {});
  host.run = new Run({
    id: ID,
    info: { role: "lead", context: "c", plan: "p1", planLead: true, registeredEvent: 0, name: null, key: null, byUser: false, loopGuardTurns: 20 },
    facts: {} as never,
    reporter: {} as never,
    abort: new AbortController(),
    log: () => {},
    follow: { hooks: () => ({}) as never, timing: host.timing },
  });
  const reports = async (n: number) => {
    await waitFor(`${n} report(s)`, () => f.reports().length >= n, 3000);
    await f.quiet();
    return f.reports();
  };
  return { host, pi, f, reports };
}

test("G1: an adapter error starts no turn", async (t) => {
  const { host, pi, reports } = await errorHost(t);
  host.deliver("policy show", new CpdError("bad_output", "one"));
  assert.deepEqual(pi.sent, [], "a Lead's chat shows no error and no turn starts");
  assert.deepEqual((await reports(1)).map((r) => [r.what, r.code, r.message]), [["policy show", "bad_output", "one"]]);
});

test("G1: the same what and code twice is reported once", async (t) => {
  const { host, pi, reports } = await errorHost(t);
  host.deliver("policy show", new CpdError("bad_output", "first message"));
  host.deliver("policy show", new CpdError("bad_output", "another message"));
  assert.equal((await reports(1)).length, 1);
  assert.deepEqual(pi.sent, []);
});

test("G1: a different error is reported, then the first is reported again", async (t) => {
  const { host, pi, reports } = await errorHost(t);
  const a = new CpdError("bad_output", "a");
  const b = new CpdError("lock_timeout", "b");
  host.deliver("policy show", a);
  await reports(1);
  host.deliver("policy show", b);
  await reports(2);
  host.deliver("policy show", a);
  assert.deepEqual((await reports(3)).map((r) => r.code), ["bad_output", "lock_timeout", "bad_output"]);
  assert.deepEqual(pi.sent, []);
});

test("a session-message names the zone after the time", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.respond(`session-show-${OTHER}`, { ok: true, session: { id: OTHER, name: "the manager" } });
  const at = "2026-10-25T22:29:00.000Z";
  f.emit(message(101, null, { at }), message(102, OTHER, { at }));
  await f.drain();
  assert.equal(f.pi.delivered()[0]!.message.content, "Message from the User (03:59 +05:30): ship it");
  assert.equal(f.pi.delivered()[1]!.message.content, `Message from the manager (${OTHER}) (03:59 +05:30): ship it`);
});

test("a plan-done with data.turn false is no message and no turn for a Lead", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "plan-done", { data: { workflows: [], turn: false } }));
  await f.quiet(); // not drain: its marker Event starts a turn, which resets the counters
  assert.deepEqual(f.pi.delivered(), []);
  assert.deepEqual(f.pi.sent, []);
});

test("a plan-done without data.turn is a follow-up turn for a Lead", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  f.emit(event(101, "plan-done", { data: { workflows: [] } }));
  await f.drain();
  assert.deepEqual(f.pi.delivered().map((s) => [(s.message.details as any).id, s.options]), [[101, { triggerTurn: true, deliverAs: "followUp" }]]);
});

// ---- commands.md §15.15 L8: a delivery that starts a turn resets the intra-turn counters ----

async function guarded(f: Fixture) {
  f.respond("session-show", sessionShow({ loop_guard_turns: 20 }));
  await started(f, "lead");
}
let callNo = 0;
const readCall = async (f: Fixture) =>
  (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: `d${callNo++}`, input: { command: "rg foo" } }))[0] as { block?: boolean } | undefined;
const readsPass = async (f: Fixture, n: number, what: string) => {
  for (let i = 1; i <= n; i++) assert.ok(!(await readCall(f))?.block, `${what}: read ${i} passes`);
};

/** 12 reads, then a delivery of `e`: the streak is fresh, so 12 more reads pass and the 13th is refused. */
async function resetsOn(t: { after(fn: () => unknown): void }, e: ReturnType<typeof event>) {
  const f = new Fixture();
  t.after(() => f.close());
  await guarded(f);
  await readsPass(f, 12, "before");
  f.emit(e);
  await f.drain();
  assert.equal(f.pi.delivered().length, 1);
  assert.equal(f.pi.delivered()[0]!.options.triggerTurn, true, "the delivery starts a turn");
  await readsPass(f, 12, "after the delivery");
  assert.ok((await readCall(f))?.block, "the 13th read is refused");
}

test("a steered delivery that starts a turn resets the loop guard counters", async (t) => {
  await resetsOn(t, event(101, "escalation"));
});

test("a follow-up delivery that starts a turn resets the loop guard counters", async (t) => {
  await resetsOn(t, event(101, "job-merged", { data: { turn: true } }));
});

test("a delivery gated to nextTurn does not reset the loop guard counters", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await guarded(f);
  // control: a delivery that starts a turn resets
  await readsPass(f, 12, "before");
  f.emit(event(101, "escalation"));
  await f.drain();
  assert.equal(f.pi.delivered().length, 1);
  await readsPass(f, 12, "after the control delivery");
  // three refusals trip the breaker, so the next delivery is gated
  for (let i = 0; i < 3; i++) assert.ok((await readCall(f))?.block);
  await f.quiet();
  f.emit(event(103, "escalation"));
  await f.drain();
  assert.equal(f.pi.delivered().length, 2);
  assert.deepEqual(f.pi.delivered()[1]!.options, { triggerTurn: false, deliverAs: "nextTurn" });
  assert.ok((await readCall(f))?.block, "the streak kept its value");
});

/** A live view marker in the fixture's `$CPD_HOME` (never the real User's), read by the tag. */
const markView = (f: Fixture, view: "tui" | "webui" | "webui-preview"): void => {
  mkdirSync(join(f.cpdHome, "run"), { recursive: true });
  writeFileSync(join(f.cpdHome, "run", "ui-view.json"), JSON.stringify({ view, pid: process.pid }));
};

test("a webui session-message is prefixed with the tag", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  markView(f, "webui");
  f.emit(message(101, null, { data: { from: null, text: "ship it", origin: "webui" } }));
  await waitFor("the message", () => f.pi.delivered().length === 1);
  await f.drain();
  assert.equal(f.pi.delivered()[0]!.message.content, "Message from the User (03:11 +05:30): [via WebUI · view: WebUI] ship it");
});

test("an engine session-message is untagged", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, "lead");
  markView(f, "webui");
  // an engine message carries no origin; a WebUI one does, and is the control that keeps this red until the tag exists
  f.emit(
    message(101, null, { data: { from: null, text: "ship it" } }),
    message(102, null, { data: { from: null, text: "ship it", origin: "webui" } }),
  );
  await waitFor("both messages", () => f.pi.delivered().length === 2);
  await f.drain();
  assert.equal(f.pi.delivered()[0]!.message.content, "Message from the User (03:11 +05:30): ship it", "an engine message is untagged");
  assert.equal(f.pi.delivered()[1]!.message.content, "Message from the User (03:11 +05:30): [via WebUI · view: WebUI] ship it");
});

test("a delivery that starts no turn does not reset the loop guard counters", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await guarded(f);
  // control: a delivery that starts a turn resets
  await readsPass(f, 12, "before");
  f.emit(event(101, "escalation"));
  await f.drain();
  assert.equal(f.pi.delivered().length, 1);
  await readsPass(f, 12, "after the control delivery");
  f.emit(event(102, "job-merged", { data: { turn: false } }), event(103, "plan-done", { data: { workflows: [], turn: false } }));
  await f.quiet(); // not drain: its marker Event starts a turn, which resets the counters
  assert.equal(f.pi.delivered().length, 1, "no message, no turn");
  assert.ok((await readCall(f))?.block, "the streak kept its value");
});
