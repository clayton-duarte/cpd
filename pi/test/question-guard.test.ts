import assert from "node:assert/strict";
import { test } from "node:test";
import { boldPhi } from "../lib/phi.ts";
import { classified, classifyError, classifyThrows, classifyTimedOut, classifyUntilAborted, Fixture, planShow, shown, stateCalls, type ClassifyBehavior } from "./fake.ts";

/** commands.md §15.26 (pro-d366): the turn-end question guard nudges a Lead or Manager that ended its turn on a plain-text question. */

const NUDGE =
  "You ended your turn with a question in plain text. If you need a decision or input from the User, call `ask_user_question` now so the session enters `blocked` state; otherwise state your assumption in one line and proceed.";
const NOTICE =
  "φ: classifier_unreachable: http://ollama.lab/v1 is unreachable, so the turn-end question guard is skipped (connect Tailscale or turn it off: cpd config experimental turn_question_guard off).";
const INSTRUCTIONS = "Does this assistant message ask the human user a question or ask for a decision/confirmation before proceeding?";
const TRUE_CRITERION = "The assistant is asking the user a question, offering choices, or waiting for user confirmation/input.";
const FALSE_CRITERION = "The assistant is only reporting status, summarizing completed work, or rhetorical/quoted question marks.";
const QUESTION = "Which of the two options do you want me to take?";
const LOOP_LABEL = "blocked:φ: loop guard";

const counts = (over: Record<string, number> = {}) => ({ draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0, ...over });
const CONFIG_OK = { ok: true, path: "/c/config.toml", config: { version: 1, experimental: { turn_question_guard: true } }, sources: {} };

type Role = "lead" | "manager" | "worker";

async function started(role: Role = "lead"): Promise<Fixture> {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead", attempt: role === "worker" ? "a1" : null }));
  f.respond("plan-show", planShow({ counts: counts() }));
  f.respond("config", CONFIG_OK);
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  return f;
}

type Tool = string | { name: string; parent: string };
type Turn = { text?: string; stopReason?: string; tools?: Tool[]; before?: Record<string, unknown>; settle?: boolean };
let callId = 0;

/** One run as pi fires it: `before_agent_start`, `agent_start`, the tool calls, `agent_end`, then `agent_settled`. By default the run of a sent user message (`messageType: "user"`, no source). */
async function turn(f: Fixture, o: Turn = {}): Promise<void> {
  await f.pi.fire("before_agent_start", o.before ?? { messageType: "user", prompt: "go" });
  await f.pi.fire("agent_start");
  for (const t of o.tools ?? []) {
    const call = typeof t === "string" ? { toolName: t } : { toolName: t.name, parentToolCallId: t.parent };
    await f.pi.fire("tool_call", { toolCallId: `c${++callId}`, input: { questions: [] }, ...call });
  }
  const messages = [
    { role: "user", content: "go" },
    { role: "assistant", stopReason: o.stopReason ?? "stop", content: [{ type: "text", text: o.text ?? QUESTION }] },
  ];
  await f.pi.fire("agent_end", { messages });
  if (o.settle !== false) await f.pi.fire("agent_settled");
}

const typed = (f: Fixture) => f.pi.fire("input", { source: "interactive", text: "hello" });
const nudged = (f: Fixture, n: number) => f.userMessagesSent(n);

/** The positive control: the same Session does nudge on a question turn; then the User types (the count resets) and the records are cleared. */
async function control(f: Fixture): Promise<void> {
  await turn(f);
  await nudged(f, 1);
  assert.equal(f.pi.classifyCalls.length, 1, "the control classified once");
  await typed(f);
  f.pi.userMessages.length = 0;
  f.pi.classifyCalls.length = 0;
}

async function nothingSent(f: Fixture): Promise<void> {
  await f.quiet();
  assert.deepEqual(f.pi.userMessages, []);
  assert.deepEqual(f.pi.errorNotices, []);
  assert.deepEqual(f.pi.errors(), []);
}

const warnings = (f: Fixture) => f.pi.noticeLevels.filter((n) => n.message.includes("classifier_unreachable"));
const planReads = (f: Fixture) => f.argvs().filter((a) => a[0] === "plan" && a[1] === "show").length;
const calls = (f: Fixture, n: number) => f.classifyCalled(n);

test("question guard: a Lead turn that ends on a question is classified and nudged once", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await turn(f);
  await nudged(f, 1);
  await f.quiet();
  assert.equal(f.pi.classifyCalls.length, 1);
  assert.equal(f.pi.userMessages.length, 1);
});

test("question guard: the nudge is the agreed text sent as a steer", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await turn(f);
  await nudged(f, 1);
  const [m] = f.pi.userMessages;
  assert.equal(m!.content, NUDGE);
  assert.equal(m!.options.deliverAs, "steer");
});

test("question guard: a probability below 0.85 sends nothing, 0.85 sends", async (t) => {
  const f = await started();
  t.after(() => f.close());
  f.pi.classifyBehavior = classified(0.84);
  await turn(f);
  await calls(f, 1);
  await nothingSent(f);
  f.pi.classifyBehavior = classified(0.85);
  await turn(f);
  await nudged(f, 1);
  assert.equal(f.pi.classifyCalls.length, 2);
  assert.equal(f.pi.userMessages[0]!.content, NUDGE);
});

test("question guard: a final message without a question mark makes no classifier call", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  await turn(f, { text: "All done. Nothing left to decide." });
  await nothingSent(f);
  assert.deepEqual(f.pi.classifyCalls, []);
});

test("question guard: a run that called ask_user_question, cpd_plan_review, cpd_secret_approve or cpd_host_approve makes no classifier call", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  for (const tool of ["ask_user_question", "cpd_plan_review", "cpd_secret_approve", "cpd_host_approve"]) {
    await turn(f, { tools: ["bash", tool] });
    await nothingSent(f);
    assert.deepEqual(f.pi.classifyCalls, [], tool);
  }
  await turn(f, { tools: [{ name: "ask_user_question", parent: "c-codemode" }] }); // a codemode-nested call counts
  await nothingSent(f);
  assert.deepEqual(f.pi.classifyCalls, [], "nested");
  await turn(f, { tools: ["bash"] }); // the set was cleared: the next run without the tools is classified
  await nudged(f, 1);
});

test("question guard: a stopReason other than stop makes no classifier call", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  for (const stopReason of ["length", "toolUse", "error", "aborted"]) {
    await turn(f, { stopReason });
    await nothingSent(f);
    assert.deepEqual(f.pi.classifyCalls, [], stopReason);
  }
});

test("question guard: a Lead with a running or queued Job makes no classifier call", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  for (const busy of [{ running: 1 }, { queued: 1 }] as Record<string, number>[]) {
    f.respond("plan-show", planShow({ counts: counts(busy) }));
    await turn(f);
    await nothingSent(f);
    assert.deepEqual(f.pi.classifyCalls, [], JSON.stringify(busy));
  }
  assert.ok(planReads(f) > 0, "the Lead reads its Plan");
});

test("question guard: a Worker never calls the classifier", async (t) => {
  const lead = await started();
  t.after(() => lead.close());
  await control(lead); // the same turn nudges a Lead
  const w = await started("worker");
  t.after(() => w.close());
  await turn(w);
  await nothingSent(w);
  assert.deepEqual(w.pi.classifyCalls, []);
});

test("question guard: a Manager is guarded and reads no Plan", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  await turn(f);
  await nudged(f, 1);
  assert.equal(f.pi.userMessages[0]!.content, NUDGE);
  assert.equal(f.pi.classifyCalls.length, 1);
  assert.equal(planReads(f), 0);
});

test("question guard: experimental turn_question_guard false does nothing", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  f.respond("config", { ...CONFIG_OK, config: { version: 1, experimental: { turn_question_guard: false } } });
  await turn(f);
  await nothingSent(f);
  assert.deepEqual(f.pi.classifyCalls, []);
});

test("question guard: an absent experimental table counts as enabled", async (t) => {
  const f = await started();
  t.after(() => f.close());
  f.respond("config", { ...CONFIG_OK, config: { version: 1 } });
  await turn(f);
  await nudged(f, 1);
  f.pi.userMessages.length = 0;
  await typed(f);
  f.respond("config", { ...CONFIG_OK, config: { version: 1, experimental: {} } }); // an absent key too
  await turn(f);
  await nudged(f, 1);
});

test("question guard: a failed cpd config read does nothing and notifies no one", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  const before = f.pi.notices.length;
  f.respond("config", { ok: false, error: { code: "config_invalid", message: "bad", details: {} } }, { exit: 1 });
  await turn(f);
  await nothingSent(f);
  assert.deepEqual(f.pi.classifyCalls, []);
  assert.equal(f.pi.notices.length, before);
  f.respond("config", "not json at all");
  await turn(f);
  await nothingSent(f);
  assert.deepEqual(f.pi.classifyCalls, []);
  assert.equal(f.pi.notices.length, before);
  assert.deepEqual(f.pi.delivered(), []);
});

test("question guard: at most one nudge per User turn, reset by typed input", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await turn(f);
  await nudged(f, 1);
  await turn(f); // a second question turn of the same User turn
  await f.quiet();
  assert.equal(f.pi.userMessages.length, 1);
  assert.equal(f.pi.classifyCalls.length, 1, "no classifier call while the counter is 1");
  await typed(f);
  await turn(f);
  await nudged(f, 2);
  await f.quiet();
  assert.equal(f.pi.userMessages.length, 2);
});

test("question guard: the run the nudge starts does not reset the counter", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await turn(f);
  await nudged(f, 1);
  // pi starts the run of sendUserMessage with messageType "user" and no source
  await turn(f, { before: { messageType: "user", prompt: NUDGE } });
  await turn(f, { before: { messageType: "user", prompt: NUDGE } });
  await f.quiet();
  assert.equal(f.pi.userMessages.length, 1);
});

test("question guard: a missing model, an error, an abort or a throw from the classifier sends nothing", async (t) => {
  const cases: [string, (f: Fixture) => void, boolean][] = [
    ["missing model", (f) => f.pi.classifierModels.clear(), false],
    ["error", (f) => (f.pi.classifyBehavior = classifyError), true],
    ["timeout abort", (f) => (f.pi.classifyBehavior = classifyTimedOut), true],
    ["throw", (f) => (f.pi.classifyBehavior = classifyThrows), true],
    ["session abort", (f) => (f.pi.classifyBehavior = classifyUntilAborted), true],
  ];
  for (const [name, set, calls1] of cases) {
    const f = await started();
    try {
      await control(f);
      set(f);
      await turn(f);
      if (calls1) await calls(f, 1);
      if (name === "session abort") await f.pi.fire("session_shutdown");
      await nothingSent(f);
      assert.equal(f.pi.classifyCalls.length, calls1 ? 1 : 0, name);
    } finally {
      await f.close();
    }
  }
});

test("question guard: a run that starts before the classifier answers is not nudged", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  let release: (v: unknown) => void = () => {};
  const pending: ClassifyBehavior = () => new Promise((resolve) => (release = resolve));
  f.pi.classifyBehavior = pending;
  await turn(f);
  await calls(f, 1);
  await f.pi.fire("before_agent_start", { messageType: "user", prompt: "next" });
  await f.pi.fire("agent_start");
  release({ stopReason: "stop", answers: { asking_user: { type: "bool", probability: 0.99 } } });
  await nothingSent(f);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(f.pi.userMessages, []);
});

test("question guard: a tripped loop guard holds the nudge", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await control(f);
  for (let i = 0; i < 21; i++) {
    await f.pi.fire("before_agent_start", { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } });
    await f.pi.fire("agent_start");
  }
  await f.stateSet(LOOP_LABEL);
  await turn(f, { before: { messageType: "custom", prompt: "x", message: { customType: "cpd", content: "x", details: {} } } });
  await nothingSent(f);
});

test("question guard: the classifier gets kev-4b, the asking_user bool question and a signal", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await turn(f);
  await calls(f, 1);
  const [c] = f.pi.classifyCalls;
  assert.equal(c!.model.provider, "llama.cpp");
  assert.equal(c!.model.id, "kev-4b");
  assert.equal(c!.context.state.message, QUESTION);
  assert.deepEqual(c!.context.questions, {
    asking_user: { type: "bool", instructions: INSTRUCTIONS, criteria: { true: TRUE_CRITERION, false: FALSE_CRITERION } },
  });
  assert.equal(c!.options.maxRetries, 0);
  assert.ok(c!.options.signal instanceof AbortSignal);
  assert.equal(c!.options.signal.aborted, false);
  await nudged(f, 1);
  await typed(f);
  const long = "a".repeat(1500) + "b".repeat(2000) + "?";
  await turn(f, { text: long });
  await calls(f, 2);
  const message = f.pi.classifyCalls[1]!.context.state.message;
  assert.equal(message.length, 2000);
  assert.equal(message, long.slice(-2000));
});

test("question guard: an unreachable classifier warns the User once and the next success re-arms the warning", async (t) => {
  const f = await started();
  t.after(() => f.close());
  const run = async (b: ClassifyBehavior, n: number) => {
    f.pi.classifyBehavior = b;
    await turn(f);
    await calls(f, n);
    await f.quiet();
  };
  await run(classifyError, 1);
  await f.noticesContaining("classifier_unreachable", 1);
  await run(classifyThrows, 2);
  await run(classifyTimedOut, 3);
  assert.equal(warnings(f).length, 1, "one notice for three failures");
  assert.equal(warnings(f)[0]!.message, boldPhi(NOTICE));
  assert.equal(warnings(f)[0]!.level, "warning");
  await run(classified(0.1), 4); // reached: re-arms
  assert.equal(warnings(f).length, 1);
  await run(classifyError, 5);
  await f.noticesContaining("classifier_unreachable", 2);
  await f.quiet();
  assert.equal(warnings(f).length, 2);
  assert.deepEqual(f.pi.userMessages, []);
  assert.deepEqual(f.pi.delivered(), []);
  assert.ok(!f.pi.sent.some((s) => String(s.message.content).includes("classifier_unreachable")));
  assert.ok(!stateCalls(f).some((s) => s.includes("classifier")));
  assert.ok(!f.argvs().some((a) => a.some((x) => x.includes("classifier_unreachable"))));
});

test("question guard: no unreachable warning for a missing model, a session abort, a low probability or a failed config read", async (t) => {
  const cases: [string, (f: Fixture) => void][] = [
    ["missing model", (f) => f.pi.classifierModels.clear()],
    ["session abort", (f) => (f.pi.classifyBehavior = classifyUntilAborted)],
    ["low probability", (f) => (f.pi.classifyBehavior = classified(0.1))],
    ["failed config", (f) => f.respond("config", { ok: false, error: { code: "config_missing", message: "none", details: {} } }, { exit: 1 })],
  ];
  for (const [name, set] of cases) {
    const f = await started();
    try {
      await control(f);
      set(f);
      await turn(f);
      if (name === "session abort") {
        await calls(f, 1);
        await f.pi.fire("session_shutdown");
      }
      await f.quiet();
      assert.deepEqual(warnings(f), [], name);
      assert.deepEqual(f.pi.userMessages, [], name);
    } finally {
      await f.close();
    }
  }
});
