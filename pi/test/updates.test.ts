import assert from "node:assert/strict";
import { test } from "node:test";
import { contextCursorFrom, contextUpdate, parsePending, parsePrOpened, parseRecords, type ContextRecord, type PendingJob, type PendingPlan, type PendingPrJob, type PlanEvent, type RecordEvent } from "../lib/updates.ts";
import { CpdError } from "../lib/cpd.ts";
import { Fixture, ID, OTHER, shown, waitFor } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

// Pin the process timezone so the expected local times are deterministic and prove the rendering is local, not UTC.
process.env.TZ = "Asia/Kolkata";

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
  assert.equal(m.content, "Context ctx: 1 new records since your last turn.\n- hs-n1 note (2026-10-17 UTC): note 1");
  assert.deepEqual(m.details, { event: 201, delivered_to: ID, records: ["hs-n1"], plan_events: 0, plans_pending: 0 });

  const both = contextUpdate("ctx", ID, [recordEvent(1), recordEvent(2)], [
    record(1, { id: "hs-n1", kind: "answer", text: "Yes.", question: "hs-q8" }),
    record(2, { kind: "decision", text: "One table.", supersedes: ["hs-d12"] }),
  ])!;
  assert.deepEqual(both.content.split("\n").slice(1), ["- hs-n1 answer (2026-10-17 UTC): Yes. Answers hs-q8.", "- hs-n2 decision (2026-10-17 UTC): One table. Supersedes hs-d12."]);
});

test("message: a record superseding several records names every target", () => {
  const m = contextUpdate("ctx", ID, [recordEvent(1)], [record(1, { kind: "decision", text: "One table.", supersedes: ["hs-d12", "hs-d13"] })])!;
  assert.equal(m.content.split("\n")[1], "- hs-n1 decision (2026-10-17 UTC): One table. Supersedes hs-d12, hs-d13.");
});

test("message: a brief is shown short: its first line and its Plan, never the rest", () => {
  const brief = record(1, { id: "hs-b1", kind: "brief", text: "Retry payouts.\nSecond line, with detail.", plan: "pay" });
  const m = contextUpdate("ctx", ID, [recordEvent(1)], [{ ...brief, id: "hs-n1" }])!;
  assert.equal(m.content, "Context ctx: 1 new records since your last turn.\n- hs-n1 brief (2026-10-17 UTC): Retry payouts. Plan pay.");
  assert.ok(!m.content.includes("detail"));
  const noPlan = contextUpdate("ctx", ID, [recordEvent(1)], [{ ...brief, id: "hs-n1", plan: undefined }])!;
  assert.equal(noPlan.content.split("\n")[1], "- hs-n1 brief (2026-10-17 UTC): Retry payouts.");
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
  assert.equal(lines[1], "- hs-n1 note (2026-10-17 UTC): note 1");
  assert.equal(lines[20], "- hs-n20 note (2026-10-17 UTC): note 20");
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
  f.respond("session-show", shown(over));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

// the start-up MCP replay read (pro-d226) is not a Context-updates read
const updatesCalls = (f: Fixture) => f.calls().filter((c) => c.phase === "start").map((c) => c.argv).filter((a) => !(a[0] === "events" && a[1] === "list" && a[a.indexOf("--kind") + 1] === "mcp-attached")).filter((a) => (a[0] === "events" && a[1] === "list") || (a[0] === "context" && a[1] === "show"));

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
  assert.equal(m.content, ["Context ctx: 3 new records since your last turn.", ...range(3).map((n) => `- hs-n${n} note (2026-10-17 UTC): note ${n}`)].join("\n"));
  assert.equal(m.details.event, 204);
  assert.equal(m.details.delivered_to, ID);
  assert.deepEqual(m.details.records, ["hs-n1", "hs-n2", "hs-n3"]);
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

test("a failed read adds no message, is reported once without a turn, and a success does not re-arm an identical report (§15.1)", async (t) => {
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
  assert.equal(f.pi.errors().length, 1, "the same what and code is not shown again after a successful read");
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

test("a Worker never gets updates", async (t) => {
  const worker = new Fixture();
  t.after(() => worker.close());
  await lead(worker, { role: "worker", plan_lead: false });
  worker.respond("events-list", eventsList([1]));
  worker.respond("context-show", contextShow([record(1)]));
  assert.deepEqual(await worker.pi.fireWith("before_agent_start"), []);
  await worker.quiet();
  assert.deepEqual(updatesCalls(worker), []);
});

// ---- the Manager's digest (§15.7, B3-A5) ----

const planEvent = (n: number, over: Partial<PlanEvent> = {}): PlanEvent => ({ event: 300 + n, at: `2026-10-18T09:${String(n).padStart(2, "0")}:00Z`, plan: "pay-4521", text: `Plan phase ${n}.`, ...over });
const planEventsList = (ns: number[]) => ({ ok: true, events: ns.map((n) => ({ id: 300 + n, at: planEvent(n).at, kind: "plan-phase", context: "ctx", plan: "pay-4521", text: `Plan phase ${n}.`, data: {} })), next: 300 + Math.max(...ns, 0) });

async function manager(f: Fixture, over: Record<string, unknown> = {}) {
  await lead(f, { role: "manager", plan: null, plan_lead: false, ...over });
}
const planCalls = (f: Fixture) => updatesCalls(f).filter((a) => a[1] === "list" && !a.includes("context-record"));

test("a Manager gets nothing when nothing is new", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@plan-created", planEventsList([]));
  assert.deepEqual(await f.pi.fireWith("before_agent_start"), [undefined]);
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
  assert.deepEqual(updatesCalls(f).filter((a) => a[1] === "list").map((a) => a.at(-1)), ["100", "100", "100", "100", "301", "301", "301", "301"]);
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

const pjob = (n: number, status: string, tier = "H2"): PendingJob => ({ key: `job-${n}`, kind: "scout", status, tier, title: `Title ${n}` });
const prjob = (n: number, status: string): PendingPrJob => ({ key: `job-${n}`, status, title: `Title ${n}` });
const ppr = (jobs: PendingPrJob[], over: Record<string, unknown> = {}) => ({ key: "api", status: "dispatch", number: 212 as number | null, jobs, ...over });
const pplan = (id: string, workflows: ReturnType<typeof ppr>[], phase = "dispatch", jobs: PendingJob[] = []): PendingPlan => ({ id, phase, workflows, jobs });
/** What `cpd context pending` prints for the same Plans (the fields the digest does not use are there too). */
const pendingJson = (plans: PendingPlan[]) => ({
  ok: true,
  context: "ctx",
  plans: plans.map((p) => ({
    id: p.id, title: `T ${p.id}`, phase: p.phase, lead_session: null,
    jobs: p.jobs.map((j) => ({ ...j, touches: ["a.rs"] })),
    workflows: p.workflows.map((r) => ({
      key: r.key, repo: "/r", base: "main", status: r.status, lead_branch: `cpd/${p.id}/${r.key}`,
      pull_request: r.number === null ? null : { number: r.number, url: "u", state: "OPEN" },
      jobs: r.jobs.map((j) => ({ ...j, touches: ["a.rs"] })),
    })),
  })),
});

test("parsePending: Plans, Workflows and Jobs; anything else is bad_output", () => {
  const plans = [pplan("p", [ppr([prjob(1, "queued")]), ppr([], { key: "ui", number: null })], "dispatch", [pjob(1, "queued")])];
  assert.deepEqual(parsePending(pendingJson(plans)), plans);
  assert.throws(() => parsePending({}), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "dispatch" }] }), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "x", jobs: [], workflows: [{ key: "a", status: "s", pull_request: null, jobs: [{ key: "j", status: "queued" }] }] }] }), { code: "bad_output" });
  assert.throws(() => parsePending({ plans: [{ id: "p", phase: "x", jobs: [], workflows: [{ key: "a", status: "s", jobs: [] }] }] }), { code: "bad_output" }, "pull_request is required");
});

const twoPlans = [
  pplan("pay-4521", [ppr([prjob(1, "merged"), prjob(2, "queued"), prjob(3, "running")])]),
  pplan("pay-4522", [ppr(range(12).map((n) => prjob(n, "draft")), { key: "web", status: "plan", number: null })], "plan"),
];

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



// ---- Leads: the short update (§15.7, hs-d514) ----

const decision = (n: number, text: string, over: Partial<ContextRecord> = {}) => record(n, { kind: "decision", text, ...over });
const eventsOf = (kind: string, evs: Record<string, unknown>[]) => ({ ok: true, events: evs.map((e) => ({ kind, context: "ctx", at: "2026-10-21T15:00:00Z", text: "t", ...e })), next: Math.max(0, ...evs.map((e) => e.id as number)) });
const prOpened = (id: number, workflow: string, number: number) => ({ id, plan: "p1", data: { workflow, number, url: `https://github.com/o/r/pull/${number}` } });
const take = (...posts: { id: number; from_session: string | null; text: string; at?: string }[]) => ({ ok: true, session: ID, taken: posts.map((p) => ({ at: "2026-10-21T15:30:00Z", ...p })) });
const nothingNew = (f: Fixture) => {
  f.respond("events-list@context-record", eventsList([]));
  f.respond("events-list@pr-opened", eventsOf("pr-opened", []));
  f.respond("board-take", take());
};
const beforeTurn = async (f: Fixture) => ((await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" })) as any[]).find((r) => r !== undefined)?.message;
const takeCalls = (f: Fixture) => f.calls().filter((c) => c.phase === "start" && c.argv[0] === "board" && c.argv[1] === "take").map((c) => c.argv);

test("a Lead gets one cpd-context message with new decisions, board posts and opened pull requests", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list@context-record", eventsList([1, 2, 3, 4]));
  f.respond("context-show", contextShow([decision(1, "Use one table. The rest is in the record."), record(2, { text: "Just a note." }), decision(3, "Mine.", { by: ID }), decision(4, "Cut the wake.")]));
  f.respond("events-list@pr-opened", eventsOf("pr-opened", [prOpened(350, "api", 211)]));
  f.respond("board-take", take({ id: 1, from_session: null, text: "PR #207 opened" }));
  const m = await beforeTurn(f);
  assert.equal(m.customType, "cpd-context");
  assert.equal(m.display, true);
  assert.equal(
    m.content,
    [
      "Context ctx: 2 decisions · 1 board post · 1 pull request opened",
      "- hs-n1: Use one table.",
      "- hs-n4: Cut the wake.",
      "",
      "Board:",
      "- Board post from the User (2026-10-21 21:00 +05:30): PR #207 opened",
      "",
      "Workflows:",
      "- Workflow api: pull request opened https://github.com/o/r/pull/211",
    ].join("\n"),
  );
  assert.equal(m.details.delivered_to, ID);
  assert.equal(m.details.event, 350);
  assert.deepEqual(f.pi.sent, [], "it only returns the message: no turn");
  assert.deepEqual(takeCalls(f), [["board", "take", "--session", ID]]);
});

const legacyUrl = "https://github.com/o/r/pull/211";

test("parsePrOpened reads the Workflow key of a legacy pr-opened Event from data.pr", () => {
  assert.deepEqual(parsePrOpened({ events: [{ id: 350, plan: "p1", data: { pr: "api", number: 211, url: legacyUrl } }] }), [{ event: 350, plan: "p1", workflow: "api", number: 211, url: legacyUrl }]);
});

test("parsePrOpened prefers data.workflow when an Event carries both data.workflow and data.pr", () => {
  assert.deepEqual(parsePrOpened({ events: [{ id: 350, plan: "p1", data: { workflow: "api", pr: "old", number: 211, url: legacyUrl } }] }), [{ event: 350, plan: "p1", workflow: "api", number: 211, url: legacyUrl }]);
});

test("parsePrOpened refuses a pr-opened Event with neither data.workflow nor data.pr as bad_output", () => {
  assert.throws(() => parsePrOpened({ events: [{ id: 350, plan: "p1", data: { number: 211, url: legacyUrl } }] }), (e: any) => e instanceof CpdError && e.code === "bad_output");
});

test("a Lead cpd-context message lists the pull request of a legacy pr-opened Event that carries data.pr", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("events-list@context-record", eventsList([1, 2, 3, 4]));
  f.respond("context-show", contextShow([decision(1, "Use one table. The rest is in the record."), record(2, { text: "Just a note." }), decision(3, "Mine.", { by: ID }), decision(4, "Cut the wake.")]));
  f.respond("events-list@pr-opened", eventsOf("pr-opened", [{ id: 350, plan: "p1", data: { pr: "api", number: 211, url: legacyUrl } }]));
  f.respond("board-take", take({ id: 1, from_session: null, text: "PR #207 opened" }));
  const m = await beforeTurn(f);
  assert.equal(
    m.content,
    [
      "Context ctx: 2 decisions · 1 board post · 1 pull request opened",
      "- hs-n1: Use one table.",
      "- hs-n4: Cut the wake.",
      "",
      "Board:",
      "- Board post from the User (2026-10-21 21:00 +05:30): PR #207 opened",
      "",
      "Workflows:",
      "- Workflow api: pull request opened https://github.com/o/r/pull/211",
    ].join("\n"),
  );
});

test("a Lead gets no message when nothing is new", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  assert.equal(await beforeTurn(f), undefined);
  assert.equal(takeCalls(f).length, 1, "it looked: the board is taken at every turn start");
  assert.deepEqual(f.pi.sent, []);
});

test("a Lead's decision line is its id and first sentence", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([decision(1, "Quiet agents, the design in full (User, 2026-10-20). (1) Direct messages only to unblock. (2) The board.")]));
  const m = await beforeTurn(f);
  assert.deepEqual(m.content.split("\n").slice(1), ["- hs-n1: Quiet agents, the design in full (User, 2026-10-20)."]);
  assert.ok(!m.content.includes("board."));
});

test("board posts are taken at turn start and shown in full", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  const long = "Rebase api on main before you start, because quiet-core changed the table and the test files moved, then say so on the board when the test Job is merged and wait for the Manager's answer before the open Job.";
  f.respond("board-take", take({ id: 1, from_session: null, text: long }, { id: 2, from_session: null, text: "Second post" }));
  const m = await beforeTurn(f);
  assert.equal(m.content.split("\n")[0], "Context ctx: 2 board posts");
  assert.deepEqual(m.content.split("\n").slice(1), ["", "Board:", `- Board post from the User (2026-10-21 21:00 +05:30): ${long}`, "- Board post from the User (2026-10-21 21:00 +05:30): Second post"]);
  assert.deepEqual(takeCalls(f), [["board", "take", "--session", ID]]);
  assert.deepEqual(f.pi.sent, [], "a post never starts a turn");
});

// ---- the Manager's compressed digest (§15.7, hs-d514) ----

const planKind = (id: number, plan: string, kind: string, data: Record<string, unknown> = {}) => ({ id, plan, data, kind });

test("the Manager's Plans section has one line per Plan with counts by kind", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  nothingNew(f);
  const events = [
    { id: 301, plan: "cross-repo", kind: "pr-opened", data: { workflow: "a", number: 210, url: "u" } },
    { id: 302, plan: "cross-repo", kind: "pr-opened", data: { workflow: "b", number: 211, url: "u" } },
    { id: 303, plan: "cross-repo", kind: "job-failed", data: { cause: "x", detail: "d", tier: "O3", attempts: 1 } },
    { id: 304, plan: "inbox", kind: "job-failed", data: { cause: "x", detail: "d", tier: "O3", attempts: 1 } },
    { id: 306, plan: "cross-repo", kind: "plan-done", data: { workflows: [] } },
  ];
  f.respond("events-list@plan-created", { ok: true, events: events.map((e) => ({ context: "ctx", at: "2026-10-21T15:00:00Z", text: "t", ...e })), next: 306 });
  const m = await beforeTurn(f);
  assert.deepEqual(m.content.split("\n"), ["Plans:", "- cross-repo: 2 pull requests opened (#210, #211), 1 Job failed, done", "- inbox: 1 Job failed"]);
  assert.equal(m.details.event, 306);
});

test("the Manager's digest reads no Escalation Events: its Plan Events call names six kinds", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  nothingNew(f);
  f.respond("events-list@plan-created", planEventsList([]));
  f.respond("context-pending", pendingJson([]));
  await beforeTurn(f);
  const call = updatesCalls(f).find((c) => c.includes("plan-created"))!;
  const kinds = call.flatMap((a, i) => (a === "--kind" ? [call[i + 1]] : []));
  assert.deepEqual(kinds, ["plan-created", "plan-phase", "pr-opened", "pr-merged", "job-failed", "plan-done"]);
});

test("the Manager's Plans section never counts an Escalation Event", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  nothingNew(f);
  const events = [
    { id: 301, plan: "cross-repo", kind: "pr-opened", data: { workflow: "a", number: 210, url: "u" } },
    { id: 303, plan: "cross-repo", kind: "job-failed", data: { cause: "x", detail: "d", tier: "O3", attempts: 1 } },
    { id: 305, plan: "cross-repo", kind: "escalation", data: { escalation: "e1", question: "q" } },
    { id: 306, plan: "cross-repo", kind: "plan-done", data: { workflows: [] } },
  ];
  f.respond("events-list@plan-created", { ok: true, events: events.map((e) => ({ context: "ctx", at: "2026-10-21T15:00:00Z", text: "t", ...e })), next: 306 });
  const m = await beforeTurn(f);
  assert.deepEqual(m.content.split("\n"), ["Plans:", "- cross-repo: 1 pull request opened (#210), 1 Job failed, done"]);
});

test("the Manager's Pending work has one line per Plan and no Job lines", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  nothingNew(f);
  f.respond("events-list@plan-created", planEventsList([]));
  const jobs = (...statuses: string[]) => statuses.map((s, i) => prjob(i + 1, s));
  f.respond("context-pending", pendingJson([
    pplan("inbox", [ppr(jobs("running", "running", "queued"), { key: "mainfix" }), ppr([], { key: "prlink", status: "open", number: 207 })]),
    pplan("agent-comms", [ppr(jobs("running"), { key: "quiet-core" })], "plan"),
  ]));
  const m = await beforeTurn(f);
  assert.deepEqual(m.content.split("\n"), [
    "Pending work:",
    "- inbox (dispatch): Workflow mainfix 2 running, 1 queued; Workflow prlink: pull request open #207",
    "- agent-comms (plan): Workflow quiet-core 1 running",
  ]);
});

test("the Manager's digest counts engine errors by code", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await manager(f);
  nothingNew(f);
  f.respond("events-list@plan-created", planEventsList([]));
  const err = (id: number, code: string) => ({ id, kind: "error", context: "ctx", at: "2026-10-21T15:00:00Z", text: "t", data: { source: "watch", code, message: "m", details: {} } });
  f.respond("events-list@error", { ok: true, events: [err(401, "gh_failed"), err(402, "pr_start_failed")], next: 402 });
  f.respond("events-list@error@machine", { ok: true, events: [err(403, "gh_failed")], next: 403 });
  const m = await beforeTurn(f);
  assert.equal(m.content, "Errors: 3 (gh_failed x2, pr_start_failed x1): cpd events list --kind error --since 100");
  assert.equal(m.details.event, 403);
});

test("record lines are labelled UTC", () => {
  const at = "2026-10-26T03:03:34.352Z";
  const m = contextUpdate("ctx", ID, [{ event: 201, record: "pro-d41", seq: SEQ0 + 1 }, { event: 202, record: "pro-b1", seq: SEQ0 + 2 }], [
    record(1, { id: "pro-d41", kind: "decision", at, text: "One table." }),
    record(2, { id: "pro-b1", kind: "brief", at, text: "Retry payouts." }),
  ])!;
  assert.deepEqual(m.content.split("\n").slice(1), ["- pro-d41 decision (2026-10-26 UTC): One table.", "- pro-b1 brief (2026-10-26 UTC): Retry payouts."]);
});

test("board posts show the local date, time and zone", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  f.respond("board-take", take({ id: 1, from_session: null, text: "hello", at: "2026-10-25T22:29:00.000Z" }));
  const m = await beforeTurn(f);
  assert.deepEqual(m.content.split("\n").slice(1), ["", "Board:", "- Board post from the User (2026-10-26 03:59 +05:30): hello"]);
});

// ---- the Lead's standing line for a PR whose pull request closed without a merge ----

const planPr = (key: string, number: number, closed: boolean) => ({
  key,
  status: "open",
  pull_request: { number, url: `https://github.com/o/r/pull/${number}`, state: closed ? "closed" : "open" },
  closed_unmerged: closed,
});
const planWith = (...workflows: ReturnType<typeof planPr>[]) => ({ ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null, workflows });
const closedLine = (key: string, n: number) => `Workflow ${key}: its pull request #${n} (https://github.com/o/r/pull/${n}) is closed without a merge and cpd still lists the Workflow as open. Ask the User, or cpd workflow cancel --workflow ${key}.`;

test("a Lead turn start carries a standing line for a PR whose pull request closed without a merge", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  f.respond("plan-show", planWith(planPr("api", 623, true), planPr("web", 624, false), planPr("db", 625, true)));
  const m = await beforeTurn(f);
  assert.ok(m, "a message, though nothing else is new");
  assert.equal(m.customType, "cpd-context");
  assert.equal(m.display, true);
  const lines: string[] = m.content.split("\n");
  assert.ok(lines.includes(closedLine("api", 623)), m.content);
  assert.ok(lines.includes(closedLine("db", 625)), m.content);
  assert.equal(lines.filter((l) => l.includes("closed without a merge")).length, 2, "one line per closed PR, none for web");
  assert.deepEqual(f.pi.sent, [], "it only returns the message: no turn");
});

test("the closed PR line repeats on the next turn while nothing else is new", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  f.respond("plan-show", planWith(planPr("api", 623, true)));
  const first = await beforeTurn(f);
  const second = await beforeTurn(f);
  assert.ok(first?.content.split("\n").includes(closedLine("api", 623)), "first turn");
  assert.ok(second?.content.split("\n").includes(closedLine("api", 623)), "second turn: a standing line, not an event");
});

test("no closed PR line when no PR is closed unmerged", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  f.respond("plan-show", planWith(planPr("api", 623, true)));
  assert.ok((await beforeTurn(f))?.content.includes("closed without a merge"), "while closed: the line is there");
  f.respond("plan-show", planWith(planPr("api", 623, false)));
  assert.equal(await beforeTurn(f), undefined, "reopened: a Lead with nothing new gets no message");
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([decision(1, "Cut the wake.")]));
  const m = await beforeTurn(f);
  assert.ok(!m.content.includes("closed without a merge"));
});

test("a Manager and a Worker get no closed PR line", async (t) => {
  const control = new Fixture();
  t.after(() => control.close());
  await lead(control);
  nothingNew(control);
  control.respond("plan-show", planWith(planPr("api", 623, true)));
  assert.ok((await beforeTurn(control))?.content.includes("closed without a merge"), "a Lead gets it");

  const manager = new Fixture();
  t.after(() => manager.close());
  await quietManager(manager);
  manager.respond("plan-show", planWith(planPr("api", 623, true)));
  assert.equal(await beforeTurn(manager), undefined);

  const worker = new Fixture();
  t.after(() => worker.close());
  await lead(worker, { role: "worker", plan_lead: false });
  worker.respond("plan-show", planWith(planPr("api", 623, true)));
  assert.deepEqual(await worker.pi.fireWith("before_agent_start"), []);
});

test("a failing plan show gives one visible warning line", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  nothingNew(f);
  f.respond("plan-show", { ok: false, error: { code: "store_busy", message: "database is locked\nretry later" } }, { exit: 1 });
  const m = await beforeTurn(f);
  assert.ok(m, "never silence");
  const lines: string[] = m.content.split("\n");
  assert.equal(lines.filter((l) => l.startsWith("could not read the Plan's Workflows:")).length, 1);
  assert.ok(lines.includes("could not read the Plan's Workflows: database is locked"), m.content);
  assert.ok(!m.content.includes("retry later"), "the error's first line only");
});

// ---- the closed PR read: its argv, and a usage refusal of it ----

const planShows = (f: Fixture) => f.argvs().filter((a) => a[0] === "plan" && a[1] === "show");
const usageRefusal = { ok: false, error: { code: "usage", message: "unexpected argument 'p1' found", details: {} } };

test("the closed PR line is read with plan show and the Plan id", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-show", planWith(planPr("api", 623, true)));
  nothingNew(f);
  const before = planShows(f).length;
  const m = await beforeTurn(f);
  const fresh = planShows(f).slice(before);
  assert.equal(fresh.length, 1);
  assert.deepEqual(fresh[0], ["plan", "show", "--plan", "p1"]);
  assert.ok(m?.content.split("\n").includes(closedLine("api", 623)), m?.content);
});

test("a usage refusal of the closed PR read is reported, not rendered", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-show", usageRefusal, { exit: 2 });
  nothingNew(f);
  assert.equal(await beforeTurn(f), undefined);
  await f.quiet();
  const errors = adapterErrors(f);
  assert.equal(errors.length, 1);
  assert.equal(errors[0]!.what, "closed-unmerged Workflow line");
  assert.equal(errors[0]!.code, "usage");
});

test("a usage refusal of the closed PR read keeps the rest of the update", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-show", usageRefusal, { exit: 2 });
  nothingNew(f);
  f.respond("events-list@context-record", eventsList([1]));
  f.respond("context-show", contextShow([decision(1, "Cut the wake.")]));
  const m = await beforeTurn(f);
  assert.ok(m, "the update is delivered");
  assert.ok(m.content.includes("Cut the wake."), m.content);
  const lines: string[] = m.content.split("\n");
  assert.ok(!lines.some((l) => l.startsWith("could not read")), m.content);
  assert.ok(!lines.some((l) => l.includes("closed without a merge")), m.content);
  await f.quiet();
  const errors = adapterErrors(f);
  assert.equal(errors.length, 1);
  assert.equal(errors[0]!.code, "usage");
});

test("a usage refusal of the closed PR read is reported once", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-show", usageRefusal, { exit: 2 });
  nothingNew(f);
  await beforeTurn(f);
  await beforeTurn(f);
  await f.quiet();
  const reports = f.reports();
  assert.equal(reports.length, 1);
  assert.equal(reports[0]!.code, "usage");
});
