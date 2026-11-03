import assert from "node:assert/strict";
import { test } from "node:test";
import { Reporter } from "../lib/state.ts";
import { ev, Fixture, ID, planShow, shown, stateCalls, waitFor } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

async function started(f: Fixture) {
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("idle", () => stateCalls(f).length >= 1);
}

test("agent_start reports working; agent_settled reports idle only when pi is idle", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);

  await f.pi.fire("agent_start");
  await waitFor("working", () => stateCalls(f).length === 2);
  f.pi.idle = false; // settled, but a queued follow-up keeps pi busy
  await f.pi.fire("agent_settled", { aborted: false });
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle", "working"]);

  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false });
  await waitFor("idle", () => stateCalls(f).length === 3);
  assert.deepEqual(stateCalls(f), ["idle", "working", "idle"]);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "idle", "--session", ID], "idle takes no --label");
});

test("calls are serialised: one in flight, only the latest wanted state is sent next", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: true }, { delay: 120 });

  await f.pi.fire("agent_start"); // working: in flight
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" }); // superseded
  await f.pi.fire("ui_prompt_end");
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false }); // idle: the latest
  await waitFor("two more calls", () => f.argvs().filter((a) => a[1] === "state").length === 3, 3000);
  await f.quiet(); // the last call (in flight) ended and nothing is wanted after it

  assert.deepEqual(stateCalls(f), ["idle", "working", "idle"], "blocked never went out");
  const calls = f.calls().filter((c) => c.argv[1] === "state");
  const ends = calls.filter((c) => c.phase === "end");
  const starts = calls.filter((c) => c.phase === "start");
  for (let i = 1; i < starts.length; i++) assert.ok(starts[i]!.t >= ends[i - 1]!.t, `call ${i} started before call ${i - 1} ended`);
});

test("a refused call is reported once and the next change is tried normally", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: false, error: { code: "illegal_transition", message: "stopped to working.", details: {} } }, { exit: 3 });

  await f.pi.fire("agent_start");
  await waitFor("the error", () => adapterErrors(f).length === 1);
  assert.match(adapterErrors(f)[0]!.message.content, /^φ adapter: session state: illegal_transition: stopped to working\.$/);
  await f.quiet();
  assert.equal(adapterErrors(f).length, 1, "once");

  f.respond("session-state", { ok: true });
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false });
  await waitFor("the next change", () => stateCalls(f).length === 3);
  assert.deepEqual(stateCalls(f), ["idle", "working", "idle"]);
  assert.equal(adapterErrors(f).length, 1);
});

test("a refused state wanted again is sent again (pi-ci-W2)", async () => {
  const sent: string[] = [];
  const errs: string[] = [];
  let fail = false;
  const r = new Reporter(
    async (w) => {
      sent.push(w.state);
      if (fail) throw new Error("db busy");
    },
    (e) => errs.push((e as Error).message),
  );
  r.want({ state: "working" });
  await r.settled();
  fail = true;
  r.want({ state: "idle" });
  await r.settled();
  fail = false;
  r.want({ state: "idle" });
  await r.settled();
  assert.deepEqual(sent, ["working", "idle", "idle"]);
  assert.deepEqual(errs, ["db busy"]);
});

test("session_shutdown reports stopped and stops the follower", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await waitFor("the follower", () => f.pids().length === 1);
  await f.pi.fire("session_shutdown");
  await waitFor("the stopped call", () => stateCalls(f).at(-1) === "stopped");
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "stopped", "--session", ID]);
  await waitFor("the follower killed", () => f.killed(f.pids()[0]!));
  await f.pi.fire("agent_start"); // after shutdown nothing is reported
  await f.quiet();
  assert.equal(stateCalls(f).at(-1), "stopped");
});

test("session_shutdown waits at most shutdownWaitMs for `stopped`", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: true }, { delay: 3000 });
  await f.pi.fire("session_shutdown");
  // It returned while the `stopped` call (which takes 3 s) was still running: it did not wait for it.
  assert.deepEqual([...f.pi.execsRunning].filter((a) => a[1] === "state"), [["session", "state", "stopped", "--session", ID]]);
  await waitFor("the stopped call", () => stateCalls(f).at(-1) === "stopped");
});

test("a /name typed in pi reaches cpd; a name cpd set does not echo back", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ key: null })); // a name is fixed only for a Session with a key
  await started(f);
  assert.equal(f.pi.sessionName, "the lead");

  await f.pi.fire("session_info_changed", { name: "the lead" });
  await f.quiet();
  assert.equal(f.argvs().some((a) => a[1] === "rename"), false);

  f.pi.sessionName = "typed by the User";
  await f.pi.fire("session_info_changed", { name: "typed by the User" });
  await waitFor("the rename", () => f.argvs().some((a) => a[1] === "rename"));
  assert.deepEqual(f.argvs().find((a) => a[1] === "rename"), ["session", "rename", "typed by the User", "--session", ID]);

  await f.pi.fire("session_info_changed", { name: "typed by the User" });
  await f.quiet();
  assert.equal(f.argvs().filter((a) => a[1] === "rename").length, 1);

  // the Event it causes comes back and changes nothing
  f.emit(ev(101, "session-renamed", { session: ID, data: { name: "typed by the User" } }));
  await waitFor("applied", () => f.pi.notices.length === 1);
  assert.equal(f.argvs().filter((a) => a[1] === "rename").length, 1);
});

test("a pi dialog (ui_prompt_start..end) reports blocked `dialog`; nested prompts are one blocked period; a stray end changes nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);

  await f.pi.fire("ui_prompt_end"); // nothing open
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle"]);

  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await waitFor("blocked", () => stateCalls(f).length === 2);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state")[1], ["session", "state", "blocked", "--label", "dialog", "--session", ID]);
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "select" });
  await f.pi.fire("ui_prompt_end");
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog"], "still blocked after the inner end");

  await f.pi.fire("ui_prompt_end");
  await waitFor("idle", () => stateCalls(f).length === 3);
  await f.pi.fire("ui_prompt_end"); // stray, after a full cycle
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog", "idle"]);
});

test("a Worker's pi dialog opens no reason: its state does not change (Workers ask their Lead, hs-d303)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "worker", plan: "p1", plan_lead: false }));
  await started(f);

  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle"], "no blocked:dialog");
  await f.pi.fire("ui_prompt_end");
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle"]);
});

test("a dialog during a turn returns to working when it ends", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await f.pi.fire("agent_start");
  await waitFor("working", () => stateCalls(f).length === 2);
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "confirm" });
  await waitFor("blocked", () => stateCalls(f).length === 3);
  await f.pi.fire("ui_prompt_end");
  await waitFor("working", () => stateCalls(f).length === 4);
  assert.deepEqual(stateCalls(f), ["idle", "working", "blocked:dialog", "working"]);
});

test("a herdr:blocked event no longer changes the state; the dialog reason still blocks", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.pi.events.emit("herdr:blocked", { active: true, label: "ask" });
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle"]);
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await waitFor("dialog", () => stateCalls(f).length === 2);
  f.pi.events.emit("herdr:blocked", { active: false });
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog"]);
});

test("a reply run's idle carries --reply through the serialising reporter: the latest wanted state, once", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  f.respond("session-state", { ok: true }, { delay: 120 });

  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "hello" });
  await f.pi.fire("agent_start"); // working: in flight
  await f.pi.fire("agent_settled", { aborted: false }); // pi.idle is true: idle with --reply, pending
  await f.pi.fire("agent_settled", { aborted: false }); // the same again
  await waitFor("two more calls", () => stateCalls(f).length === 3, 3000);
  await f.quiet();
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").slice(1), [
    ["session", "state", "working", "--session", ID],
    ["session", "state", "idle", "--reply", "--session", ID],
  ]);
});

test("a blocked -> idle that closes the last reason of a reply run carries --reply; a later dialog at rest does not", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f);
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "hello" });
  await f.pi.fire("agent_start");
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "confirm" });
  await waitFor("blocked", () => stateCalls(f).length === 3);
  f.pi.idle = true;
  await f.pi.fire("agent_settled", { aborted: false }); // still blocked: nothing new
  await f.pi.fire("ui_prompt_end");
  await waitFor("idle", () => stateCalls(f).length === 4);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "idle", "--reply", "--session", ID]);

  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "confirm" });
  await waitFor("blocked", () => stateCalls(f).length === 5);
  await f.pi.fire("ui_prompt_end");
  await waitFor("idle", () => stateCalls(f).length === 6);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state").at(-1), ["session", "state", "idle", "--session", ID], "the mark was spent");
});

// hs-d303 (audit J2): `blocked` only while a human reason (dialog) is open; else `waiting` while an agent reason (Worker question) is open.
const QUESTION = "φ: Worker question";
const question = (id: number, esc: string) => ev(id, "escalation", { data: { escalation: esc, question: "q?" } });
const answered = (id: number, esc: string) => ev(id, "escalation-answered", { data: { escalation: esc } });

async function startedLead(f: Fixture) {
  await started(f);
  await waitFor("the follower", () => f.follows().length > 0);
}

test("a Worker question alone reports waiting with its label", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await startedLead(f);

  f.emit(question(101, "j-a1-q1"));
  await waitFor("waiting", () => stateCalls(f).length === 2);
  assert.deepEqual(f.argvs().filter((a) => a[1] === "state")[1], ["session", "state", "waiting", "--label", QUESTION, "--session", ID]);
  f.emit(answered(102, "j-a1-q1"));
  await waitFor("idle", () => stateCalls(f).length === 3);
  assert.deepEqual(stateCalls(f), ["idle", `waiting:${QUESTION}`, "idle"]);
});

test("a Worker question opened during a dialog does not hide it; answered while the dialog stays: still blocked `dialog`", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await startedLead(f);

  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await waitFor("blocked", () => stateCalls(f).length === 2);
  f.emit(question(101, "j-a1-q1"));
  await waitFor("the question is known", () => f.pi.delivered().length === 1);
  await f.drain();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog"], "the question did not replace the dialog label");

  f.emit(answered(102, "j-a1-q1"));
  await f.drain();
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog"], "still blocked: dialog");
});

test("the dialog closed with a Worker question still open reports waiting", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await startedLead(f);

  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await waitFor("blocked", () => stateCalls(f).length === 2);
  f.emit(question(101, "j-a1-q1"));
  await waitFor("the question is known", () => f.pi.delivered().length === 1);
  await f.drain();
  await f.pi.fire("ui_prompt_end");
  await waitFor("waiting", () => stateCalls(f).length === 3);
  assert.deepEqual(stateCalls(f), ["idle", "blocked:dialog", `waiting:${QUESTION}`]);
});
