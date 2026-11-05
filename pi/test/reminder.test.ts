import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../lib/cpd.ts";
import { failedJobReminderText, lastAssistantText, namesKey, needsReminder, PHRASES, REMINDER_TEXT, unaddressedFailedJobs } from "../lib/reminder.ts";
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
  await f.quiet();
  assert.equal(plans(), atStart + 1, "one plan show ran, for the failed-Job check");
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

// ---- the failed-Job reminder (§15.8) ----

const job = (key: string, status: string, attempts: unknown[] = [], over: Record<string, unknown> = {}) => ({ key, status, attempts, ...over });
const failedShow = (...jobs: unknown[]) => planShow({ jobs });
const FAILED = "cpd reminder: failed Job(s) a, b still unaddressed. A failed Job is never final: dispose of it now, in this turn: cpd job retry <k> --tier <T>, a fix Job (cpd job add), cpd job cancel <k> --reason \"...\" when superseded, or ask_user_question when the choice is the User's. Never leave it failed while idle or defer the retry to a background sleep or timer.";
const failedSent = (f: Fixture) => f.pi.sent.filter((s) => s.message.customType === "cpd-reminder");

test("unaddressedFailedJobs lists every failed Job with its attempts count, in order", () => {
  const show = failedShow(job("b", "failed", [{}, {}]), job("x", "merged", [{}]), job("a", "failed", [{}]), job("c", "failed", []));
  assert.deepEqual(unaddressedFailedJobs(show), [{ key: "b", attempts: 2 }, { key: "a", attempts: 1 }, { key: "c", attempts: 0 }]);
});

test("unaddressedFailedJobs ignores merged, queued, running, draft and cancelled Jobs", () => {
  const show = failedShow(...["merged", "queued", "running", "draft", "cancelled"].map((s) => job(`j-${s}`, s, [{}])), job("f", "failed", [{}]));
  assert.deepEqual(unaddressedFailedJobs(show), [{ key: "f", attempts: 1 }]);
});

test("unaddressedFailedJobs ignores a worked_on field: a failed Job always counts", () => {
  const show = failedShow(job("a", "failed", [{}], { worked_on: true }), job("b", "failed", [{}], { worked_on: false }));
  assert.deepEqual(unaddressedFailedJobs(show), [{ key: "a", attempts: 1 }, { key: "b", attempts: 1 }]);
});

test("unaddressedFailedJobs throws bad_output on a bad shape", () => {
  const bad = (show: Record<string, unknown>) => assert.throws(() => unaddressedFailedJobs(show), (e) => e instanceof CpdError && e.code === "bad_output");
  bad({});
  bad({ jobs: "x" });
  bad({ jobs: [{ status: "failed", attempts: [] }] });
  bad({ jobs: [{ key: 1, status: "failed", attempts: [] }] });
  bad({ jobs: [{ key: "a", status: "failed" }] });
  bad({ jobs: [{ key: "a", status: "failed", attempts: 2 }] });
});

test("failedJobReminderText names the keys and the four ways to dispose", () => {
  assert.equal(failedJobReminderText(["a", "b"]), FAILED);
});

test("a Lead run ending with a failed Job sends one failed-Job reminder that starts a turn", async (t) => {
  const f = await started({}, failedShow(job("a", "failed", [{}]), job("b", "failed", [{}])));
  t.after(() => f.close());
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  const sent = failedSent(f);
  assert.equal(sent.length, 1);
  assert.equal(sent[0]!.message.display, false);
  assert.equal(sent[0]!.message.content, FAILED);
  assert.deepEqual(sent[0]!.message.details, { rule: "failed-job-disposition", jobs: ["a", "b"] });
  assert.deepEqual(sent[0]!.options, { triggerTurn: true, deliverAs: "followUp" });
});

test("a Lead run ending with no assistant text still sends the failed-Job reminder", async (t) => {
  const f = await started({}, failedShow(job("a", "failed", [{}]), job("b", "failed", [{}])));
  t.after(() => f.close());
  await f.pi.fire("agent_end", { messages: [{ role: "assistant", content: [{ type: "toolCall" }] }] });
  await f.quiet();
  assert.equal(failedSent(f).length, 1);
  assert.equal(failedSent(f)[0]!.message.content, FAILED);
});

test("no failed-Job reminder when no Job failed or the failed Job was retried or cancelled", async (t) => {
  const f = await started({}, failedShow(job("a", "failed", [{}])));
  t.after(() => f.close());
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  assert.equal(failedSent(f).length, 1, "the failed Job is reminded");
  for (const status of ["merged", "queued", "cancelled"]) {
    f.respond("plan-show", failedShow(job("a", status, [{}, {}])));
    await f.pi.fire("agent_end", ended("Done."));
    await f.quiet();
    assert.equal(failedSent(f).length, 1, status);
  }
});

test("no failed-Job reminder while the Lead is blocked on a dialog", async (t) => {
  const f = await started({}, failedShow(job("a", "failed", [{}])));
  t.after(() => f.close());
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  assert.deepEqual(failedSent(f), [], "blocked: nothing sent");
  await f.pi.fire("ui_prompt_end");
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  assert.equal(failedSent(f).length, 1, "the same failure state was not marked reminded while blocked");
});

test("the failed-Job reminder fires at most once for the same failure state", async (t) => {
  const f = await started({}, failedShow(job("a", "failed", [{}])));
  t.after(() => f.close());
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  assert.equal(failedSent(f).length, 1);
});

test("the failed-Job reminder fires again when attempts grow, another Job fails, or the set emptied before", async (t) => {
  const f = await started({}, failedShow(job("a", "failed", [{}])));
  t.after(() => f.close());
  const end = async () => {
    await f.pi.fire("agent_end", ended("Done."));
    await f.quiet();
  };
  await end();
  assert.equal(failedSent(f).length, 1);
  f.respond("plan-show", failedShow(job("a", "failed", [{}, {}])));
  await end();
  assert.equal(failedSent(f).length, 2, "attempts grew");
  f.respond("plan-show", failedShow(job("a", "failed", [{}, {}]), job("b", "failed", [{}])));
  await end();
  assert.equal(failedSent(f).length, 3, "another Job failed");
  f.respond("plan-show", failedShow(job("a", "queued", [{}, {}]), job("b", "merged", [{}])));
  await end();
  assert.equal(failedSent(f).length, 3, "nothing failed");
  f.respond("plan-show", failedShow(job("a", "failed", [{}, {}]), job("b", "merged", [{}])));
  await end();
  assert.equal(failedSent(f).length, 4, "failed again after the set emptied");
});

test("a failing plan show sends no failed-Job reminder", async (t) => {
  const f = await started();
  t.after(() => f.close());
  f.respond("plan-show", { ok: false, error: { code: "plan_not_found", message: "No.", details: {} } }, { exit: 4 });
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  assert.deepEqual(failedSent(f), []);
  f.respond("plan-show", failedShow(job("a", "failed", [{}])));
  await f.pi.fire("agent_end", ended("Done."));
  await f.quiet();
  assert.equal(failedSent(f).length, 1, "a later good plan show reminds");
});

test("a Manager and a Worker get no failed-Job reminder", async (t) => {
  const lead = await started({}, failedShow(job("a", "failed", [{}])));
  t.after(() => lead.close());
  await lead.pi.fire("agent_end", ended("Done."));
  await lead.quiet();
  assert.equal(failedSent(lead).length, 1, "a Lead gets one");
  for (const over of [{ role: "manager", plan: null, plan_lead: false }, { role: "worker", plan: "p1", plan_lead: false }]) {
    const f = await started(over, failedShow(job("a", "failed", [{}])));
    t.after(() => f.close());
    await f.pi.fire("agent_end", ended("Done."));
    await f.quiet();
    assert.deepEqual(failedSent(f), [], String(over.role));
  }
});
