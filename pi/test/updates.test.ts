import assert from "node:assert/strict";
import { test } from "node:test";
import { contextCursorFrom, contextUpdate, parsePlanEvents, parseRecords, type ContextRecord, type PlanEvent, type RecordEvent } from "../lib/updates.ts";
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
  assert.deepEqual(m.details, { event: 201, delivered_to: ID, records: ["hs-n1"] });

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

test("a Lead's run with 3 new records by others gets one cpd-context message, in full", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList([1, 2, 3, 4]));
  f.respond("context-show", contextShow([record(1), record(2), record(3), record(4, { by: ID })]));

  const [result] = await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" });
  const m = (result as any).message;
  assert.equal(m.customType, "cpd-context");
  assert.equal(m.display, true);
  assert.equal(m.content, ["Context ctx: 3 new records since your last turn.", ...range(3).map((n) => `- hs-n${n} note (2026-10-17): note ${n}`)].join("\n"));
  assert.deepEqual(m.details, { event: 204, delivered_to: ID, records: ["hs-n1", "hs-n2", "hs-n3"] });
  assert.deepEqual(updatesCalls(f), [
    ["events", "list", "--context", "ctx", "--kind", "context-record", "--since", "100"],
    ["context", "show", "--context", "ctx", "--since", String(SEQ0)],
  ]);
  assert.deepEqual(f.pi.sent, [], "it only returns the message: nothing is sent, so no turn");
});

test("a run with no new records gets nothing, and reads no records", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList([]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.deepEqual(updatesCalls(f).map((a) => a[0] + " " + a[1]), ["events list"]);
  assert.deepEqual(f.pi.sent, []);
});

test("25 new records show 20 and the command for the rest", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList(range(25)));
  f.respond("context-show", contextShow(range(25).map((n) => record(n))));
  const [result] = await f.pi.fireWith("before_agent_start");
  const lines = (result as any).message.content.split("\n");
  assert.equal(lines.length, 22);
  assert.equal(lines[21], `and 5 more: cpd context show --context ctx --since ${SEQ0 + 20}`);
});

test("only records by this Session: no message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList([1]));
  f.respond("context-show", contextShow([record(1, { by: ID })]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
});

test("after a restart the cursor is read back from this session's own cpd-context messages, not a fork's foreign ones", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries = [contextMessage(900, OTHER), contextMessage(150, ID), contextMessage(170, ID)];
  await lead(f);
  f.respond("events-list", eventsList([]));
  await f.pi.fireWith("before_agent_start");
  assert.deepEqual(updatesCalls(f)[0], ["events", "list", "--context", "ctx", "--kind", "context-record", "--since", "170"]);
});

test("the cursor moves with the message the run keeps in the session", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList([1]));
  f.respond("context-show", contextShow([record(1)]));
  const [result] = await f.pi.fireWith("before_agent_start");
  f.pi.entries.push({ type: "custom_message", ...(result as any).message }); // pi stores what the hook returned
  f.respond("events-list", eventsList([]));
  await f.pi.fireWith("before_agent_start");
  assert.deepEqual(updatesCalls(f).filter((a) => a[1] === "list").map((a) => a.at(-1)), ["100", "201"]);
});

test("a failed read adds no message, is reported once without a turn, and the next success re-arms the report", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  const fail = { ok: false, error: { code: "store_busy", message: "database is locked" } };
  f.respond("events-list", fail, { exit: 1 });
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.equal(f.pi.errors().length, 1, "once");
  const e = f.pi.errors()[0]!;
  assert.deepEqual(e.options, { triggerTurn: false, deliverAs: "followUp" });
  assert.ok((e.message.content as string).includes("store_busy"));
  assert.deepEqual(updatesCalls(f).filter((a) => a[1] === "list").map((a) => a.at(-1)), ["100", "100"], "the cursor stays");

  f.respond("events-list", eventsList([]));
  await f.pi.fireWith("before_agent_start");
  f.respond("events-list", fail, { exit: 1 });
  await f.pi.fireWith("before_agent_start");
  assert.equal(f.pi.errors().length, 2, "reported again after a successful read");
});

test("a failed context show is a failed read too", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList([1]));
  f.respond("context-show", { ok: false, error: { code: "context_not_found", message: "no such context" } }, { exit: 4 });
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
  assert.equal(f.pi.errors().length, 1);
});

test("a Worker never gets updates, and a side fork (role lead, not the Plan's Lead) does", async (t) => {
  const worker = new Fixture();
  t.after(() => worker.close());
  await lead(worker, { role: "worker", plan_lead: false });
  worker.respond("events-list", eventsList([1]));
  worker.respond("context-show", contextShow([record(1)]));
  assert.deepEqual(await worker.pi.fireWith("before_agent_start"), []);
  await settle();
  assert.deepEqual(updatesCalls(worker), []);
});

test("a side fork gets updates", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, { plan_lead: false });
  f.respond("events-list", eventsList([1]));
  f.respond("context-show", contextShow([record(1)]));
  const [result] = await f.pi.fireWith("before_agent_start");
  assert.ok((result as any).message.content.includes("- hs-n1 note"));
});

// ---- the Manager's digest (§15.6, B3-A5) ----

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
  assert.deepEqual(m.details, { event: 302, delivered_to: ID, records: ["hs-n1", "hs-n2"] });
});

test("digest: Plan Events alone, or records alone, or none", () => {
  const plansOnly = contextUpdate("ctx", ID, [], [], [planEvent(1)])!;
  assert.equal(plansOnly.content, "Plans:\n- 2026-10-18T09:01:00Z pay-4521: Plan phase 1.");
  assert.deepEqual(plansOnly.details, { event: 301, delivered_to: ID, records: [] });
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
  assert.deepEqual(m.details, { event: 303, delivered_to: ID, records: ["hs-n1", "hs-n2"] });
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

test("a Lead's message has no Plans: section and never reads Plan Events", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list", eventsList([1]));
  f.respond("events-list@plan-created", planEventsList([1, 2]));
  f.respond("context-show", contextShow([record(1)]));
  const [result] = await f.pi.fireWith("before_agent_start");
  assert.ok(!(result as any).message.content.includes("Plans:"));
  assert.deepEqual(planCalls(f), []);
});
