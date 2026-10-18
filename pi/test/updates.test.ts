import assert from "node:assert/strict";
import { test } from "node:test";
import { contextCursorFrom, contextUpdate, countsOf, parsePending, parsePlanEvents, parseRecords, pendingSection, summaryLine, type ContextRecord, type PendingJob, type PendingPlan, type PlanEvent, type RecordEvent } from "../lib/updates.ts";
import { Fixture, ID, OTHER, sessionShow, settle, waitFor } from "./fake.ts";

const contextMessage = (event: number | undefined, deliveredTo: string | undefined, customType = "cpd-context") => ({
  type: "custom_message",
  customType,
  content: "x",
  display: true,
  details: { ...(event !== undefined ? { event } : {}), ...(deliveredTo !== undefined ? { delivered_to: deliveredTo } : {}), records: [] },
});

test("cursor: registered_event, then the greatest event of this Session's cpd-context messages", () => {
  assert.equal(contextCursorFrom(100, [], ID), 100);
  assert.equal(contextCursorFrom(100, [contextMessage(90, ID)], ID), 100, "never below registered_event");
  assert.equal(contextCursorFrom(100, [contextMessage(130, ID), contextMessage(120, ID)], ID), 130, "the greatest");
  assert.equal(contextCursorFrom(100, [contextMessage(500, OTHER)], ID), 100, "a fork's file starts with its source's messages");
  assert.equal(contextCursorFrom(100, [contextMessage(500, undefined)], ID), 100, "no delivered_to, no count");
  assert.equal(contextCursorFrom(100, [contextMessage(undefined, ID)], ID), 100);
  assert.equal(contextCursorFrom(100, [contextMessage(500, ID, "cpd")], ID), 100, "only customType cpd-context");
  assert.equal(contextCursorFrom(100, [{ type: "custom", customType: "cpd-context", data: { event: 900, delivered_to: ID } }, "junk", null], ID), 100);
});

const SEQ0 = 40;
const record = (n: number, over: Partial<ContextRecord> = {}): ContextRecord => ({
  seq: SEQ0 + n, id: `hs-n${n}`, kind: "note", at: "2026-10-17T09:00:00.000Z", by: OTHER, text: `note ${n}`, ...over,
});
const recordEvent = (n: number): RecordEvent => ({ event: 200 + n, record: `hs-n${n}`, seq: SEQ0 + n });
const range = (n: number) => Array.from({ length: n }, (_, i) => i + 1);

test("message: nothing for 0 records", () => {
  assert.equal(contextUpdate("ctx", ID, [], []), undefined);
});

test("message: 1 record, in full, with its date; answers and supersedes name their target", () => {
  const m = contextUpdate("ctx", ID, [recordEvent(1)], [record(1)])!;
  assert.equal(m.customType, "cpd-context");
  assert.equal(m.display, true);
  assert.equal(m.content, "Context ctx: 1 new records since your last turn.\n- hs-n1 note (2026-10-17): note 1");
  assert.deepEqual(m.details, { event: 201, delivered_to: ID, records: ["hs-n1"], plan_events: 0, plans_pending: 0 });

  const both = contextUpdate("ctx", ID, [recordEvent(1), recordEvent(2)], [
    record(1, { id: "hs-n1", kind: "answer", text: "Yes.", question: "hs-q8" }),
    record(2, { kind: "decision", text: "One table.", supersedes: "hs-d12" }),
  ])!;
  assert.deepEqual(both.content.split("\n").slice(1), ["- hs-n1 answer (2026-10-17): Yes. Answers hs-q8.", "- hs-n2 decision (2026-10-17): One table. Supersedes hs-d12."]);
});

test("message: a brief is shown short: its first line and its Plan, never the rest", () => {
  const brief = record(1, { id: "hs-b1", kind: "brief", text: "Retry payouts.\nSecond line, with detail.", plan: "pay" });
  const m = contextUpdate("ctx", ID, [recordEvent(1)], [{ ...brief, id: "hs-n1" }])!;
  assert.equal(m.content, "Context ctx: 1 new records since your last turn.\n- hs-n1 brief (2026-10-17): Retry payouts. Plan pay.");
  assert.ok(!m.content.includes("detail"));
  const noPlan = contextUpdate("ctx", ID, [recordEvent(1)], [{ ...brief, id: "hs-n1", plan: undefined }])!;
  assert.equal(noPlan.content.split("\n")[1], "- hs-n1 brief (2026-10-17): Retry payouts.");
});

test("parseRecords: a record's plan is its Plan id", () => {
  const raw = { seq: 1, id: "hs-b1", kind: "brief", at: "2026-10-17T09:00:00.000Z", by: { session: null, plan: null }, text: "Retry.", short: true, plan: { id: "pay", machine: "hs" } };
  assert.equal(parseRecords({ records: [raw] })[0]!.plan, "pay");
  assert.equal("plan" in parseRecords({ records: [{ ...raw, plan: undefined }] })[0]!, false);
});

test("message: 20 records are all listed", () => {
  const m = contextUpdate("ctx", ID, range(20).map(recordEvent), range(20).map((n) => record(n)))!;
  assert.equal(m.content.split("\n").length, 21);
  assert.ok(!m.content.includes("more:"));
});

test("message: 25 records list 20 and name the 5 more with the seq before the first unlisted", () => {
  const m = contextUpdate("ctx", ID, range(25).map(recordEvent), range(25).map((n) => record(n)))!;
  const lines = m.content.split("\n");
  assert.equal(lines[0], "Context ctx: 25 new records since your last turn.");
  assert.equal(lines.length, 1 + 20 + 1);
  assert.equal(lines[1], "- hs-n1 note (2026-10-17): note 1");
  assert.equal(lines[20], "- hs-n20 note (2026-10-17): note 20");
  assert.equal(lines[21], `and 5 more: cpd context show --context ctx --since ${SEQ0 + 20}`);
  assert.equal((m.details as any).event, 225);
  assert.equal((m.details as any).records.length, 25);
});

test("message: own records are skipped; when all are own there is no message", () => {
  const own = record(2, { by: ID });
  const m = contextUpdate("ctx", ID, [recordEvent(1), recordEvent(2), recordEvent(3)], [record(1), own, record(3)])!;
  assert.deepEqual((m.details as any).records, ["hs-n1", "hs-n3"]);
  assert.ok(m.content.startsWith("Context ctx: 2 new records"));
  assert.equal((m.details as any).event, 203);

  const lastOwn = contextUpdate("ctx", ID, [recordEvent(1), recordEvent(2)], [record(1), record(2, { by: ID })])!;
  assert.equal((lastOwn.details as any).event, 202, "own records count for the cursor");
  assert.equal(contextUpdate("ctx", ID, [recordEvent(2)], [own]), undefined);
});

// ---- through the adapter: before_agent_start ----

const eventsList = (ns: number[]) => ({ ok: true, events: ns.map((n) => ({ id: 200 + n, kind: "context-record", data: { record: `hs-n${n}`, kind: "note", machine: "hs", seq: SEQ0 + n } })), next: 200 + Math.max(...ns, 0) });
const contextShow = (rs: ContextRecord[]) => ({ ok: true, context: { id: "ctx" }, records: rs.map((r) => ({ ...r, machine: "hs", by: { session: r.by, plan: "p1" } })), markdown: "" });

async function lead(f: Fixture, over: Record<string, unknown> = {}) {
  f.respond("session-show", sessionShow(over));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

const updatesCalls = (f: Fixture) => f.calls().filter((c) => c.phase === "start").map((c) => c.argv).filter((a) => (a[0] === "events" && a[1] === "list") || (a[0] === "context" && a[1] === "show"));

/** A Manager whose Plan Events and pending work are empty, so a test can focus on the records. */
async function quietManager(f: Fixture) {
  await lead(f, { role: "manager", plan: null, plan_lead: false });
  f.respond("events-list@plan-created", { ok: true, events: [], next: 0 });
  f.respond("context-pending", { ok: true, plans: [] });
}
const recordCalls = (f: Fixture) => updatesCalls(f).filter((a) => a[1] === "show" || a.includes("context-record"));

test("a Manager's run with 3 new records by others gets one cpd-context message, in full", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  f.respond("events-list@context-record", eventsList([1, 2, 3, 4]));
  f.respond("context-show", contextShow([record(1), record(2), record(3), record(4, { by: ID })]));

  const [result] = await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" });
  const m = (result as any).message;
  assert.equal(m.customType, "cpd-context");
  assert.equal(m.display, true);
  assert.equal(m.content, ["Context ctx: 3 new records since your last turn.", ...range(3).map((n) => `- hs-n${n} note (2026-10-17): note ${n}`)].join("\n"));
  assert.deepEqual(m.details, { event: 204, delivered_to: ID, records: ["hs-n1", "hs-n2", "hs-n3"], plan_events: 0, plans_pending: 0 });
  assert.deepEqual(recordCalls(f), [
    ["events", "list", "--context", "ctx", "--kind", "context-record", "--since", "100"],
    ["context", "show", "--context", "ctx", "--since", String(SEQ0)],
  ]);
  assert.deepEqual(f.pi.sent, [], "it only returns the message: nothing is sent, so no turn");
});

test("a run with no new records gets nothing, and reads no records", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  f.respond("events-list@context-record", eventsList([]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.deepEqual(recordCalls(f).map((a) => a[0] + " " + a[1]), ["events list"]);
  assert.deepEqual(f.pi.sent, []);
});

test("25 new records show 20 and the command for the rest", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  f.respond("events-list@context-record", eventsList(range(25)));
  f.respond("context-show", contextShow(range(25).map((n) => record(n))));
  const [result] = await f.pi.fireWith("before_agent_start");
  const lines = (result as any).message.content.split("\n");
  assert.equal(lines.length, 22);
  assert.equal(lines[21], `and 5 more: cpd context show --context ctx --since ${SEQ0 + 20}`);
});

test("only records by this Session: no message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([record(1, { by: ID })]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
});

test("after a restart the cursor is read back from this session's own cpd-context messages, not a fork's foreign ones", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries = [contextMessage(900, OTHER), contextMessage(150, ID), contextMessage(170, ID)];
  await quietManager(f);
  f.respond("events-list@context-record", eventsList([]));
  await f.pi.fireWith("before_agent_start");
  assert.deepEqual(recordCalls(f)[0], ["events", "list", "--context", "ctx", "--kind", "context-record", "--since", "170"]);
});

test("the cursor moves with the message the run keeps in the session", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([record(1)]));
  const [result] = await f.pi.fireWith("before_agent_start");
  f.pi.entries.push({ type: "custom_message", ...(result as any).message }); // pi stores what the hook returned
  f.respond("events-list@context-record", eventsList([]));
  await f.pi.fireWith("before_agent_start");
  assert.deepEqual(recordCalls(f).filter((a) => a[1] === "list").map((a) => a.at(-1)), ["100", "201"]);
});

test("a failed read adds no message, is reported once without a turn, and the next success re-arms the report", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  const fail = { ok: false, error: { code: "store_busy", message: "database is locked" } };
  f.respond("events-list@context-record", fail, { exit: 1 });
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.equal(f.pi.errors().length, 1, "once");
  const e = f.pi.errors()[0]!;
  assert.deepEqual(e.options, { triggerTurn: false, deliverAs: "followUp" });
  assert.ok((e.message.content as string).includes("store_busy"));
  assert.deepEqual(recordCalls(f).filter((a) => a[1] === "list").map((a) => a.at(-1)), ["100", "100"], "the cursor stays");

  f.respond("events-list@context-record", eventsList([]));
  await f.pi.fireWith("before_agent_start");
  f.respond("events-list@context-record", fail, { exit: 1 });
  await f.pi.fireWith("before_agent_start");
  assert.equal(f.pi.errors().length, 2, "reported again after a successful read");
});

test("a failed context show is a failed read too", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await quietManager(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", { ok: false, error: { code: "context_not_found", message: "no such context" } }, { exit: 4 });
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.equal(f.pi.errors().length, 1);
});

test("a Lead (the Plan's, or a side fork) gets no cpd-context message and reads nothing, even with new records (hs-d209, D1)", async (t) => {
  for (const over of [{}, { plan_lead: false }]) {
    const f = new Fixture();
    t.after(() => f.close());
    await lead(f, over);
    f.respond("events-list", eventsList([1, 2, 3]));
    f.respond("events-list@plan-created", planEventsList([1]));
    f.respond("context-show", contextShow([record(1), record(2), record(3)]));
    f.respond("context-pending", pendingJson(twoPlans));
    assert.deepEqual(await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" }), []);
    await settle();
    assert.deepEqual(updatesCalls(f), []);
    assert.deepEqual(f.calls().filter((c) => c.phase === "start").map((c) => c.argv).filter((a) => a[1] === "pending"), []);
    assert.deepEqual(f.pi.sent, []);
  }
});

test("a Worker never gets updates", async (t) => {
  const worker = new Fixture();
  t.after(() => worker.close());
  await lead(worker, { role: "worker", plan_lead: false });
  worker.respond("events-list", eventsList([1]));
  worker.respond("context-show", contextShow([record(1)]));
  assert.deepEqual(await worker.pi.fireWith("before_agent_start"), []);
  await settle();
  assert.deepEqual(updatesCalls(worker), []);
});

// ---- the Manager's digest (§15.7, B3-A5) ----

const planEvent = (n: number, over: Partial<PlanEvent> = {}): PlanEvent => ({ event: 300 + n, at: `2026-10-18T09:${String(n).padStart(2, "0")}:00Z`, plan: "pay-4521", text: `Plan phase ${n}.`, ...over });
const planEventsList = (ns: number[]) => ({ ok: true, events: ns.map((n) => ({ id: 300 + n, at: planEvent(n).at, kind: "plan-phase", context: "ctx", plan: "pay-4521", text: `Plan phase ${n}.`, data: {} })), next: 300 + Math.max(...ns, 0) });

test("digest: records, then Plans:, one line per Event; details.event is the highest of either read", () => {
  const m = contextUpdate("ctx", ID, [recordEvent(1), recordEvent(2)], [record(1), record(2)], [planEvent(1), planEvent(2, { plan: "pay-4522", text: "Job api-1 failed." })])!;
  assert.equal(
    m.content,
    [
      "Context ctx: 2 new records since your last turn.",
      "- hs-n1 note (2026-10-17): note 1",
      "- hs-n2 note (2026-10-17): note 2",
      "Plans:",
      "- 2026-10-18T09:01:00Z pay-4521: Plan phase 1.",
      "- 2026-10-18T09:02:00Z pay-4522: Job api-1 failed.",
    ].join("\n"),
  );
  assert.deepEqual(m.details, { event: 302, delivered_to: ID, records: ["hs-n1", "hs-n2"], plan_events: 2, plans_pending: 0 });
});

test("digest: Plan Events alone, or records alone, or none", () => {
  const plansOnly = contextUpdate("ctx", ID, [], [], [planEvent(1)])!;
  assert.equal(plansOnly.content, "Plans:\n- 2026-10-18T09:01:00Z pay-4521: Plan phase 1.");
  assert.deepEqual(plansOnly.details, { event: 301, delivered_to: ID, records: [], plan_events: 1, plans_pending: 0 });
  assert.ok(!contextUpdate("ctx", ID, [recordEvent(1)], [record(1)], [])!.content.includes("Plans:"));
  assert.equal(contextUpdate("ctx", ID, [], [], []), undefined);
  assert.equal(contextUpdate("ctx", ID, [recordEvent(1)], [record(1, { by: ID })], []), undefined, "only own records, no Plan Events");
  const ownAndPlans = contextUpdate("ctx", ID, [recordEvent(1)], [record(1, { by: ID })], [planEvent(1)])!;
  assert.equal((ownAndPlans.details as any).event, 301);
});

test("digest: 25 Plan Events list 20 and name the 5 more with the id of the last one listed", () => {
  const m = contextUpdate("ctx", ID, [], [], range(25).map((n) => planEvent(n)))!;
  const lines = m.content.split("\n");
  assert.equal(lines.length, 1 + 20 + 1);
  assert.equal(lines[20], "- 2026-10-18T09:20:00Z pay-4521: Plan phase 20.");
  assert.equal(lines[21], "and 5 more: cpd events list --context ctx --since 320");
  assert.equal((m.details as any).event, 325, "the highest id read, listed or not");
});

test("parsePlanEvents: id, at, plan and text; anything else is bad_output", () => {
  assert.deepEqual(parsePlanEvents(planEventsList([1])), [planEvent(1)]);
  assert.throws(() => parsePlanEvents({ events: [{ id: 1, at: "x", plan: null, text: "t" }] }), { code: "bad_output" });
  assert.throws(() => parsePlanEvents({}), { code: "bad_output" });
});

async function manager(f: Fixture, over: Record<string, unknown> = {}) {
  await lead(f, { role: "manager", plan: null, plan_lead: false, ...over });
}
const planCalls = (f: Fixture) => updatesCalls(f).filter((a) => a[1] === "list" && !a.includes("context-record"));

test("a Manager's run with 2 new records and 3 Plan Events gets one cpd-context message with both sections", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([1, 2]));
  f.respond("context-show", contextShow([record(1), record(2)]));
  f.respond("events-list@plan-created", planEventsList([1, 2, 3]));

  const [result] = await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" });
  const m = (result as any).message;
  assert.equal(m.customType, "cpd-context");
  assert.equal(m.content.split("\n").filter((l: string) => l === "Plans:").length, 1);
  assert.deepEqual(m.content.split("\n").slice(0, 3), ["Context ctx: 2 new records since your last turn.", "- hs-n1 note (2026-10-17): note 1", "- hs-n2 note (2026-10-17): note 2"]);
  assert.deepEqual(m.content.split("\n").slice(3), ["Plans:", ...range(3).map((n) => `- ${planEvent(n).at} pay-4521: Plan phase ${n}.`)]);
  assert.deepEqual(m.details, { event: 303, delivered_to: ID, records: ["hs-n1", "hs-n2"], plan_events: 3, plans_pending: 0 });
  assert.deepEqual(planCalls(f), [["events", "list", "--context", "ctx", ...["plan-created", "plan-phase", "pr-opened", "pr-merged", "job-failed", "escalation", "plan-done"].flatMap((k) => ["--kind", k]), "--since", "100"]]);
  assert.deepEqual(f.pi.sent, [], "it never starts a turn");
});

test("a Manager's digest alone: no records, Plan Events only", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList([1]));
  const [result] = await f.pi.fireWith("before_agent_start");
  assert.equal((result as any).message.content, `Plans:\n- ${planEvent(1).at} pay-4521: Plan phase 1.`);
  assert.deepEqual(updatesCalls(f).map((a) => a[0] + " " + a[1]), ["events list", "events list"], "no records to read");
});

test("a Manager gets nothing when nothing is new", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList([]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
});

test("a Manager's 25 Plan Events show 20 and the command for the rest", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList(range(25)));
  const [result] = await f.pi.fireWith("before_agent_start");
  const lines = (result as any).message.content.split("\n");
  assert.equal(lines.length, 22);
  assert.equal(lines[21], "and 5 more: cpd events list --context ctx --since 320");
  assert.equal((result as any).message.details.event, 325);
});

test("the Manager's cursor is the one for both sections: the next read starts after the highest id read", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList([1]));
  const [result] = await f.pi.fireWith("before_agent_start");
  f.pi.entries.push({ type: "custom_message", ...(result as any).message });
  await f.pi.fireWith("before_agent_start");
  assert.deepEqual(updatesCalls(f).filter((a) => a[1] === "list").map((a) => a.at(-1)), ["100", "100", "301", "301"]);
});

test("a failed Plan Events read fails the whole message: nothing added, the cursor stays, one error without a turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([record(1)]));
  f.respond("events-list@plan-created", { ok: false, error: { code: "store_busy", message: "database is locked" } }, { exit: 1 });
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.equal(f.pi.errors().length, 1);
  assert.deepEqual(f.pi.errors()[0]!.options, { triggerTurn: false, deliverAs: "followUp" });
});

// ---- the Manager's Pending work (§15.7, hs-d159 P2) ----

const pjob = (n: number, status: string): PendingJob => ({ key: `job-${n}`, status, title: `Title ${n}` });
const ppr = (jobs: PendingJob[], over: Record<string, unknown> = {}) => ({ key: "api", status: "dispatch", number: 212 as number | null, jobs, ...over });
const pplan = (id: string, prs: ReturnType<typeof ppr>[], phase = "dispatch"): PendingPlan => ({ id, phase, prs });
/** What `cpd context pending` prints for the same Plans (the fields the digest does not use are there too). */
const pendingJson = (plans: PendingPlan[]) => ({
  ok: true,
  context: "ctx",
  plans: plans.map((p) => ({
    id: p.id, title: `T ${p.id}`, phase: p.phase, lead_session: null,
    prs: p.prs.map((r) => ({
      key: r.key, repo: "/r", base: "main", status: r.status, lead_branch: `cpd/${p.id}/${r.key}`,
      pull_request: r.number === null ? null : { number: r.number, url: "u", state: "OPEN" },
      jobs: r.jobs.map((j) => ({ ...j, touches: ["a.rs"] })),
    })),
  })),
});

test("pendingSection: one line per PR with the counts, then its Jobs in order", () => {
  const lines = pendingSection([pplan("pay-4521", [ppr([pjob(1, "merged"), pjob(2, "queued"), pjob(3, "running"), pjob(4, "failed"), pjob(5, "draft"), pjob(6, "queued")])])]);
  assert.deepEqual(lines, [
    "Pending work:",
    "- pay-4521 (dispatch), PR api dispatch #212: 1 merged on its Lead Branch, 2 queued, 1 running, 1 failed, 1 draft",
    "  - job-1 merged: Title 1",
    "  - job-2 queued: Title 2",
    "  - job-3 running: Title 3",
    "  - job-4 failed: Title 4",
    "  - job-5 draft: Title 5",
    "  - job-6 queued: Title 6",
  ]);
});

test("pendingSection: no ` #<n>` before the pull request exists; a PR with no Jobs is one line", () => {
  assert.deepEqual(pendingSection([pplan("p", [ppr([], { number: null, status: "plan" })], "plan")]), ["Pending work:", "- p (plan), PR api plan: 0 merged on its Lead Branch, 0 queued, 0 running, 0 failed, 0 draft"]);
});

test("pendingSection: 10 Jobs are all listed, 13 list 10 and say 3 more; the counts cover all 13", () => {
  const ten = pendingSection([pplan("p", [ppr(range(10).map((n) => pjob(n, "queued")))])]);
  assert.equal(ten.length, 1 + 1 + 10);
  assert.ok(!ten.some((l) => l.includes("more")));
  const jobs = range(13).map((n) => pjob(n, "queued"));
  const lines = pendingSection([pplan("p", [ppr(jobs)])]);
  assert.equal(lines.length, 1 + 1 + 10 + 1);
  assert.ok(lines[1]!.includes("13 queued"));
  assert.equal(lines[10 + 1]!, "  - job-10 queued: Title 10");
  assert.equal(lines.at(-1), "  and 3 more");
});

test("pendingSection: nothing when no Plan is active or no Plan has a PR", () => {
  assert.deepEqual(pendingSection([]), []);
  assert.deepEqual(pendingSection([pplan("p", [])]), []);
});

test("parsePending: Plans, PRs and Jobs; anything else is bad_output", () => {
  const plans = [pplan("p", [ppr([pjob(1, "queued")]), ppr([], { key: "ui", number: null })])];
  assert.deepEqual(parsePending(pendingJson(plans)), plans);
  assert.throws(() => parsePending({}), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "dispatch" }] }), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "x", prs: [{ key: "a", status: "s", pull_request: null, jobs: [{ key: "j", status: "queued" }] }] }] }), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "x", prs: [{ key: "a", status: "s", jobs: [] }] }] }), { code: "bad_output" }, "pull_request is required");
});

test("message: Pending work alone is a message; it does not move details.event; it follows Plans:", () => {
  const pending = [pplan("p", [ppr([pjob(1, "queued")])])];
  const alone = contextUpdate("ctx", ID, [], [], [], pending)!;
  assert.equal(alone.content.split("\n")[0], "Pending work:");
  assert.deepEqual(alone.details, { event: 0, delivered_to: ID, records: [], plan_events: 0, plans_pending: 1 });
  const ownOnly = contextUpdate("ctx", ID, [recordEvent(1)], [record(1, { by: ID })], [], pending)!;
  assert.deepEqual(ownOnly.details, { event: 201, delivered_to: ID, records: [], plan_events: 0, plans_pending: 1 });
  const all = contextUpdate("ctx", ID, [recordEvent(1)], [record(1)], [planEvent(1)], pending)!;
  assert.deepEqual(all.content.split("\n").filter((l) => !l.startsWith(" ") && !l.startsWith("- ")), ["Context ctx: 1 new records since your last turn.", "Plans:", "Pending work:"]);
  assert.ok(all.content.indexOf("Plans:") < all.content.indexOf("Pending work:"));
  assert.equal((all.details as any).event, 301);
  assert.equal(contextUpdate("ctx", ID, [], [], [], []), undefined);
});

const twoPlans = [
  pplan("pay-4521", [ppr([pjob(1, "merged"), pjob(2, "queued"), pjob(3, "running")])]),
  pplan("pay-4522", [ppr(range(12).map((n) => pjob(n, "draft")), { key: "web", status: "plan", number: null })], "plan"),
];

test("a Manager's turn start with two active Plans gets a Pending work section, one line per Plan, capped at 10 Jobs", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList([]));
  f.respond("context-pending", pendingJson(twoPlans));
  const [result] = await f.pi.fireWith("before_agent_start");
  const m = (result as any).message;
  assert.equal(m.customType, "cpd-context");
  const lines = m.content.split("\n");
  assert.deepEqual(lines.filter((l: string) => l.startsWith("- ")), [
    "- pay-4521 (dispatch), PR api dispatch #212: 1 merged on its Lead Branch, 1 queued, 1 running, 0 failed, 0 draft",
    "- pay-4522 (plan), PR web plan: 0 merged on its Lead Branch, 0 queued, 0 running, 0 failed, 12 draft",
  ]); // Job lines are indented, so they do not match
  assert.equal(lines[0], "Pending work:");
  assert.equal(lines.length, 1 + 1 + 3 + 1 + 10 + 1);
  assert.equal(lines.at(-1), "  and 2 more");
  assert.deepEqual(m.details, { event: 0, delivered_to: ID, records: [], plan_events: 0, plans_pending: 2 });
  assert.deepEqual(f.calls().filter((c) => c.phase === "start").map((c) => c.argv).filter((a) => a[1] === "pending"), [["context", "pending", "--context", "ctx"]]);
  assert.deepEqual(f.pi.sent, []);
});

test("Pending work follows Plans: and the records in one message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([record(1)]));
  f.respond("events-list@plan-created", planEventsList([1]));
  f.respond("context-pending", pendingJson(twoPlans));
  const [result] = await f.pi.fireWith("before_agent_start");
  const lines = (result as any).message.content.split("\n");
  assert.deepEqual(lines.filter((l: string) => /^[A-Z]/.test(l) && !l.startsWith("Context")), ["Plans:", "Pending work:"]);
  assert.ok(lines.indexOf("Plans:") < lines.indexOf("Pending work:"));
  assert.equal((result as any).message.details.event, 301);
});

test("a Manager with no active Plan gets no Pending work section", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([record(1)]));
  f.respond("events-list@plan-created", planEventsList([]));
  f.respond("context-pending", pendingJson([]));
  const [result] = await f.pi.fireWith("before_agent_start");
  assert.ok(!(result as any).message.content.includes("Pending work:"));
  f.respond("events-list@context-record", eventsList([]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
});

test("a failed Pending work read omits the message and is reported once without a turn", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList([1]));
  f.respond("context-pending", { ok: false, error: { code: "store_busy", message: "database is locked" } }, { exit: 1 });
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.equal(f.pi.errors().length, 1);
  assert.deepEqual(f.pi.errors()[0]!.options, { triggerTurn: false, deliverAs: "followUp" });
});


test("summaryLine: 0, 1 and many of each part; a zero part is left out; nothing else", () => {
  const line = (records: number, plan_events: number, plans_pending: number) => summaryLine({ records: range(records), plan_events, plans_pending });
  assert.equal(line(6, 17, 10), "cpd-context: 6 records · 17 Plan Events · 10 Plans pending");
  assert.equal(line(1, 1, 1), "cpd-context: 1 record · 1 Plan Event · 1 Plan pending");
  assert.equal(line(0, 17, 10), "cpd-context: 17 Plan Events · 10 Plans pending");
  assert.equal(line(6, 0, 10), "cpd-context: 6 records · 10 Plans pending");
  assert.equal(line(6, 17, 0), "cpd-context: 6 records · 17 Plan Events");
  assert.equal(line(0, 0, 2), "cpd-context: 2 Plans pending");
  assert.equal(line(0, 1, 0), "cpd-context: 1 Plan Event");
  assert.equal(line(1, 0, 0), "cpd-context: 1 record");
});

test("message: details carry the counts: records read, every Plan Event (listed or not), Plans with a PR", () => {
  const plans = [pplan("a", [ppr([pjob(1, "queued")]), ppr([pjob(2, "queued")], { key: "web" })]), pplan("b", []), pplan("c", [ppr([])])];
  const m = contextUpdate("ctx", ID, [recordEvent(1), recordEvent(2)], [record(1), record(2, { by: ID })], range(25).map((n) => planEvent(n)), plans)!;
  assert.deepEqual(m.details, { event: 325, delivered_to: ID, records: ["hs-n1"], plan_events: 25, plans_pending: 2 });
  assert.equal(summaryLine(countsOf(m.details)!), "cpd-context: 1 record · 25 Plan Events · 2 Plans pending");
});
