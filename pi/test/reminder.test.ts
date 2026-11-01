import assert from "node:assert/strict";
import { test } from "node:test";
import { lastAssistantText, namesKey, needsReminder, PHRASES, REMINDER_TEXT } from "../lib/reminder.ts";
import { Fixture, ID, planShow, shown, waitFor } from "./fake.ts";

test("every phrase of the list makes a message talk of future work, in any case", () => {
  for (const p of PHRASES) {
    assert.equal(needsReminder(`Done. ${p}: x`, []), true, p);
    assert.equal(needsReminder(`xx ${p.toUpperCase()} yy`, []), true, `upper: ${p}`);
  }
});

test("`after … merges` is a pattern: 1 to 60 characters, no newline or full stop", () => {
  assert.equal(needsReminder("After the first PR merges we go on", []), true);
  assert.equal(needsReminder("after it merged", []), false);
  assert.equal(needsReminder("after.\nmerges", []), false);
  assert.equal(needsReminder("after a. merges", []), false);
  assert.equal(needsReminder(`after ${"x".repeat(61)} merges`, []), false);
  assert.equal(needsReminder(`after ${"x".repeat(58)} merges`, []), true);
});

test("a message with no phrase needs no reminder", () => {
  assert.equal(needsReminder("Everything is done and merged.", []), false);
  assert.equal(needsReminder("", []), false);
});

test("a known Job or PR key as a whole word, case-sensitive, silences it", () => {
  assert.equal(needsReminder("Next batch: reg-docs", ["reg-docs"]), false);
  assert.equal(needsReminder("Next batch is the `register` PR.", ["register"]), false);
  assert.equal(needsReminder("Next batch: (reg-docs).", ["reg-docs"]), false);
  assert.equal(needsReminder("Next batch: reg-docs-2", ["reg-docs"]), true, "not a whole word");
  assert.equal(needsReminder("Next batch: reg", ["reg-docs"]), true);
  assert.equal(needsReminder("Next batch: REG-DOCS", ["reg-docs"]), true, "case-sensitive");
  assert.equal(needsReminder("Next batch: x", ["a", "b"]), true);
  assert.equal(namesKey("a.b c", "a.b"), true, "regex characters of a key are literal");
  assert.equal(namesKey("axb", "a.b"), false);
});

test("the last assistant message's text", () => {
  const a = (text: string) => ({ role: "assistant", content: [{ type: "thinking", thinking: "t" }, { type: "text", text }] });
  assert.equal(lastAssistantText([a("one"), { role: "toolResult", content: [] }, a("two")]), "two");
  assert.equal(lastAssistantText([{ role: "assistant", content: "plain" }]), "plain");
  assert.equal(lastAssistantText([{ role: "assistant", content: [{ type: "toolCall" }] }]), undefined);
  assert.equal(lastAssistantText([{ role: "user", content: "x" }]), undefined);
  assert.equal(lastAssistantText(undefined), undefined);
});

// ---- through the adapter, with the fake cpd ----

const ended = (text: string) => ({ messages: [{ role: "user", content: "go" }, { role: "assistant", content: [{ type: "text", text }] }] });
const plan = () => planShow({ jobs: [{ key: "reg-docs" }, { key: "reg-reminder" }], workflows: [{ key: "register", jobs: ["reg-docs", "reg-reminder"] }] });

async function started(over: Record<string, unknown> = {}, show: unknown = plan()) {
  const f = new Fixture();
  f.respond("session-show", shown(over));
  f.respond("plan-show", show);
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return f;
}

const hidden = (out: unknown[]) => out.filter((o) => o !== undefined) as { message: { customType: string; content: string; display: boolean } }[];

test("a Lead's run ending with future work and no key: the next run carries one hidden reminder", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await f.pi.fire("agent_end", ended("Merged. Next batch: the docs."));
  const first = hidden(await f.pi.fireWith("before_agent_start"));
  assert.equal(first.length, 1);
  assert.equal(first[0]!.message.customType, "cpd-reminder");
  assert.equal(first[0]!.message.display, false);
  assert.equal(first[0]!.message.content, REMINDER_TEXT);
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), [], "at most once");
  assert.deepEqual(f.pi.sent, [], "never a message of its own: no turn, nothing shown");
});

test("naming a key of the Plan, a Job or a Workflow, makes none", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await f.pi.fire("agent_end", ended("Next batch: reg-reminder."));
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), []);
  await f.pi.fire("agent_end", ended("Follow-up PR `register` waits."));
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), []);
  await f.pi.fire("agent_end", ended("All done."));
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), []);
});

test("a later run that ends cleanly takes the reminder back", async (t) => {
  const f = await started();
  t.after(() => f.close());
  await f.pi.fire("agent_end", ended("Next batch: the docs."));
  await f.pi.fire("agent_end", ended("Registered reg-docs."));
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), []);
});

test("a run that ends with no assistant text checks nothing", async (t) => {
  const f = await started();
  t.after(() => f.close());
  const plans = () => f.argvs().filter((a) => a[0] === "plan").length;
  const atStart = plans();
  await f.pi.fire("agent_end", { messages: [{ role: "assistant", content: [{ type: "toolCall" }] }] });
  assert.equal(plans(), atStart, "the reminder ran no plan show for that run");
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), []);
});

test("a failing plan show: the reminder adds nothing and says nothing", async (t) => {
  const f = await started();
  t.after(() => f.close());
  f.respond("plan-show", { ok: false, error: { code: "plan_not_found", message: "No.", details: {} } }, { exit: 4 });
  await f.pi.fire("agent_end", ended("Next batch: the docs."));
  const out = hidden(await f.pi.fireWith("before_agent_start"));
  assert.deepEqual(out, [
    {
      message: {
        customType: "cpd-context",
        display: true,
        content: "could not read the Plan's Workflows: No.",
        details: { event: 0, delivered_to: ID, records: [], summary: "cpd-context" },
      },
    },
  ], "only the turn line's warning");
  assert.ok(!JSON.stringify(out).includes("cpd-reminder"), "no reminder message");
  await f.quiet(); // the failed plan show is done and its result handled
  assert.deepEqual(f.pi.sent, [], "no error message in the chat");
  assert.deepEqual(f.pi.errorNotices, []);
});

test("a plan show of an unexpected shape adds nothing", async (t) => {
  const f = await started();
  t.after(() => f.close());
  f.respond("plan-show", { ok: true, plan: { id: "p1" } });
  await f.pi.fire("agent_end", ended("Next batch: the docs."));
  assert.deepEqual(hidden(await f.pi.fireWith("before_agent_start")), []);
});

test("a Manager and a Worker get none, and read no Plan for it", async (t) => {
  for (const over of [{ role: "manager", plan: null, plan_lead: false }, { role: "worker", plan: "p1", plan_lead: false }]) {
    const f = await started(over);
    t.after(() => f.close());
    const plans = () => f.argvs().filter((a) => a[0] === "plan").length;
    const before = plans();
    await f.pi.fire("agent_end", ended("Next batch: the docs."));
    const out = hidden(await f.pi.fireWith("before_agent_start")).filter((o) => o.message.customType === "cpd-reminder");
    assert.deepEqual(out, [], String(over.role));
    assert.equal(plans(), before, `${over.role}: no plan show`);
    assert.deepEqual(f.pi.sent.filter((s) => s.message.customType === "cpd-reminder"), []);
  }
});
