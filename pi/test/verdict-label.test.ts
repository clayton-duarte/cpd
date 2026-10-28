import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture, planShow, shown, stateCalls, waitFor } from "./fake.ts";

// commands.md §15.4 (pro-d150): an `ask_user_question` with a `Verdict` question names the next dialog's blocked reason `verdict: <option>`.

type Question = { question: string; header: string; options: unknown[] };
const opt = (label: unknown) => ({ label, description: "d" });
const verdict = (label: unknown, header = "Verdict"): Question => ({ question: "What is your verdict on the change?", header, options: [opt(label), opt("Other")] });
const PROMPT = { reason: "ui_prompt", kind: "select" };
const VISIBLE = [{ type: "message", message: { role: "assistant", content: [{ type: "text", text: "Here is what I think." }, { type: "toolCall", id: "t", name: "ask_user_question", arguments: {} }] } }];

async function started(role: "lead" | "worker" = "lead"): Promise<Fixture> {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", planShow());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("idle", () => stateCalls(f).length >= 1);
  return f;
}

async function ask(f: Fixture, questions: Question[], toolCallId = "t") {
  f.pi.entries = VISIBLE;
  return (await f.pi.fireWith("tool_call", { toolName: "ask_user_question", toolCallId, input: { questions } }))[0] as { block?: boolean } | undefined;
}

const next = async (f: Fixture) => {
  const n = stateCalls(f).length;
  await f.pi.fire("ui_prompt_start", PROMPT);
  await waitFor("the state call", () => stateCalls(f).length > n);
  await f.quiet();
  return stateCalls(f).at(-1);
};
const end = async (f: Fixture) => {
  const n = stateCalls(f).length;
  await f.pi.fire("ui_prompt_end");
  await waitFor("idle", () => stateCalls(f).length > n);
  await f.quiet();
  return stateCalls(f).at(-1);
};
/** One full dialog of a Verdict ask: its label as reported, then the end. */
async function cycle(f: Fixture, label: string): Promise<void> {
  await ask(f, [verdict(label)]);
  assert.match((await next(f))!, /^blocked:verdict: /);
  assert.equal(await end(f), "idle");
}

test("a Verdict ask names the blocked reason verdict: with its first option, then idle", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await ask(f, [verdict("Approve with comments (Recommended)")]);
  assert.equal(await next(f), "blocked:verdict: Approve with comments");
  assert.equal(await end(f), "idle");
});

test("the Verdict header matches in any case and a trailing Recommended tag is stripped in any case", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await ask(f, [verdict("Reject (recommended)", "VERDICT")]);
  assert.equal(await next(f), "blocked:verdict: Reject");
  await end(f);
  await ask(f, [verdict("Approve (Recommended) now")]);
  assert.equal(await next(f), "blocked:verdict: Approve (Recommended) now");
});

test("the first Verdict question with a usable first option decides", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await ask(f, [verdict("Wide", "Scope"), verdict(" (Recommended)"), verdict("Approve", "verdict")]);
  assert.equal(await next(f), "blocked:verdict: Approve");
});

test("the option is whitespace-normalised and the whole label is cut at 80 characters", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await ask(f, [verdict("  Approve\n\twith   comments  ")]);
  assert.equal(await next(f), "blocked:verdict: Approve with comments");
  await end(f);
  await ask(f, [verdict("x".repeat(300))]);
  const label = (await next(f))!.slice("blocked:".length);
  assert.equal(label.length, 80);
  assert.ok(label.startsWith("verdict: x"), label);
});

test("the label ends with the last prompt: a later dialog is dialog again", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await cycle(f, "Approve");
  assert.equal(await next(f), "blocked:dialog");
});

test("a tool_result clears a label that no dialog used", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await cycle(f, "Approve");
  await ask(f, [verdict("Reject")]);
  await f.pi.fire("tool_result", { toolName: "ask_user_question", toolCallId: "t", input: {}, content: [], isError: false });
  assert.equal(await next(f), "blocked:dialog");
});

test("nested prompts keep the Verdict label until the last one ends", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await ask(f, [verdict("Approve")]);
  assert.equal(await next(f), "blocked:verdict: Approve");
  await f.pi.fire("ui_prompt_start", PROMPT);
  await f.pi.fire("ui_prompt_end");
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle", "blocked:verdict: Approve"], "no new state call");
  assert.equal(await end(f), "idle");
});

// ---- not named: these pass today and must keep passing ----

test("a non-Verdict header gives dialog", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await ask(f, [verdict("Approve", "Scope")]);
  assert.equal(await next(f), "blocked:dialog");
});

test("a Verdict question with no options, a non-string label or an empty label gives dialog", async (t) => {
  for (const q of [{ question: "What is your verdict?", header: "Verdict", options: [] }, verdict(42), verdict("  (Recommended)")]) {
    const f = await started();
    t.after(() => f.close());
    await ask(f, [q]);
    assert.equal(await next(f), "blocked:dialog");
  }
});

test("a Verdict ask refused by G2 leaves the next dialog dialog", async (t) => {
  const f = await started();
  t.after(() => f.close());
  const verdictOf = await ask(f, [{ ...verdict("Approve"), question: "q".repeat(1600) }]);
  assert.equal(verdictOf?.block, true);
  assert.equal(await next(f), "blocked:dialog");
});

test("a Worker's Verdict ask opens no reason", async (t) => {
  const f = await started("worker");
  t.after(() => f.close());
  await ask(f, [verdict("Approve")]);
  await f.pi.fire("ui_prompt_start", PROMPT);
  await f.quiet();
  assert.deepEqual(stateCalls(f), ["idle"]);
});
