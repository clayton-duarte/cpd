import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Fixture, ID, planShow, stateCalls, ev, waitFor } from "./fake.ts";

const REVIEW_LABEL = "blocked:cpd: plan review";
const pending = (id = "r1", over: Record<string, unknown> = {}) => planShow({ review: { id, status: "pending", fingerprint: "ff", current: true, ...over } });
const reviewEvent = (id: number, review = "r1", status = "pending") => ev(id, "plan-review", { data: { review, fingerprint: "ff", status } });
const entries = (f: Fixture) => f.pi.entries.filter((e: any) => e.type === "custom" && e.customType === "cpd-plan-review").map((e: any) => e.data);
const requests = (f: Fixture, action?: string) => f.pi.emitted.filter((e) => e.channel === "plannotator:request" && (action === undefined || e.data.action === action));
const decisions = (f: Fixture) => f.pi.sent.filter((s) => s.message.customType === "cpd-plan-decision");

/** plannotator answers every request with `answer(request)`. */
function plannotator(f: Fixture, answer: (req: any) => unknown) {
  f.pi.events.on("plannotator:request", (req: any) => {
    const a = answer(req);
    if (a !== undefined) req.respond(a);
  });
}
const opened = (reviewId = "pn-1") => ({ status: "handled", result: { status: "pending", reviewId } });

async function lead(f: Fixture, show: unknown = pending()) {
  f.respond("plan-show", show);
  f.planFile("p1", "r1", "# the plan\n");
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

test("a pending plan-review Event opens plannotator, records the entry and blocks the Lead; no message", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, (req) => (req.action === "plan-review" ? opened() : undefined));
  await lead(f);
  f.emit(reviewEvent(101));
  await waitFor("the entry", () => entries(f).length === 1);

  const [req] = requests(f, "plan-review");
  assert.equal(typeof req!.data.requestId, "string");
  assert.deepEqual(req!.data.payload, { planContent: "# the plan\n", planFilePath: `${f.cpdHome}/run/p1/plan-r1.md` });
  assert.deepEqual(entries(f), [{ plan: "p1", review: "r1", plannotator_id: "pn-1", status: "pending" }]);
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  assert.deepEqual(f.pi.sent, []);
  assert.deepEqual(f.argvs().filter((a) => a[0] === "plan"), [["plan", "show", "--plan", "p1"], ["plan", "show", "--plan", "p1"]], "start-up, then the latest-review check");
});

test("a replayed or superseded request is skipped", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => opened());
  await lead(f);
  f.emit(reviewEvent(101));
  await waitFor("the entry", () => entries(f).length === 1);

  f.emit(reviewEvent(102)); // replay: the session already has an entry for r1
  await f.drain(); // marker 103
  assert.equal(requests(f, "plan-review").length, 1);

  f.respond("plan-show", pending("r3")); // r2 is not the latest review
  f.emit(reviewEvent(110, "r2"));
  await f.drain(); // 111
  assert.equal(requests(f, "plan-review").length, 1);

  f.respond("plan-show", pending("r3", { status: "approved" })); // decided meanwhile
  f.emit(reviewEvent(120, "r3"));
  await f.drain(); // 121
  assert.equal(requests(f, "plan-review").length, 1);

  // a restart keeps the entry, so the replayed Event is still skipped, and the Lead is blocked at start-up
  await f.pi.fire("session_shutdown");
  f.respond("plan-show", pending());
  await f.pi.fire("session_start");
  await waitFor("blocked at start-up", () => stateCalls(f).filter((s) => s === REVIEW_LABEL).length === 2);
  f.emit(reviewEvent(130));
  await f.drain();
  assert.equal(requests(f, "plan-review").length, 1);
});

test("a decided (non-pending) plan-review Event is not opened", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => opened());
  await lead(f);
  f.emit(reviewEvent(101, "r1", "approved"));
  await f.drain();
  assert.equal(requests(f).length, 0);
  assert.deepEqual(f.pi.sent, []);
});

for (const [name, answer, code, text] of [
  ["unavailable", { status: "unavailable" }, "plannotator_unavailable", /unavailable/],
  ["an error", { status: "error", error: "Port 19432 in use after 5 retries" }, "plannotator_error", /Port 19432 in use after 5 retries/],
  ["handled but not pending", { status: "handled", result: { status: "completed" } }, "plannotator_invalid", /did not open/],
  ["no answer", undefined, "plannotator_timeout", /did not answer plan-review/],
] as const) {
  test(`plannotator answers ${name}: an error message with a turn, nothing written, the review stays pending`, async (t) => {
    const f = new Fixture();
    t.after(() => f.close());
    plannotator(f, () => answer);
    await lead(f);
    f.emit(reviewEvent(101));
    await waitFor("the error", () => f.pi.errors().length === 1, 4000);
    const e = f.pi.errors()[0]!;
    assert.match(e.message.content, new RegExp(`^cpd adapter: plan review r1: ${code}: `));
    assert.match(e.message.content, text);
    assert.deepEqual(e.options, { triggerTurn: true, deliverAs: "followUp" });
    assert.deepEqual(entries(f), []);
    assert.equal(stateCalls(f).includes(REVIEW_LABEL), false);
    assert.equal(f.argvs().some((a) => a[1] === "decide"), false);
  });
}

test("an unreadable plan file is an error", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => opened());
  f.respond("plan-show", pending());
  f.load();
  await f.pi.fire("session_start");
  f.emit(reviewEvent(101));
  await waitFor("the error", () => f.pi.errors().length === 1);
  assert.match(f.pi.errors()[0]!.message.content, /^cpd adapter: plan review r1: file_unreadable: /);
  assert.equal(requests(f).length, 0);
});

async function opens(f: Fixture) {
  plannotator(f, () => opened());
  await lead(f);
  f.emit(reviewEvent(101));
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
}
const result = (f: Fixture, data: unknown) => f.pi.events.emit("plannotator:review-result", data);

test("approval: plan decide --approve with the feedback in a file, the entry, the reason closed, the Lead told", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: true, next: "cpd plan lock" });
  result(f, { reviewId: "pn-1", approved: true, feedback: "Looks good.\nShip it." });
  await waitFor("the decision message", () => decisions(f).length === 1);

  const call = f.calls().find((c) => c.phase === "end" && c.argv[1] === "decide")!;
  assert.deepEqual(call.argv.slice(0, 4), ["plan", "decide", "r1", "--approve"]);
  assert.equal(call.argv[4], "--feedback-file");
  const file = join(f.cpdHome, "run", "p1", "feedback-r1.md");
  assert.equal(call.argv[5], file);
  assert.equal(readFileSync(file, "utf8"), "Looks good.\nShip it.", "kept in the Plan's run directory");
  assert.deepEqual(call.argv.slice(6), ["--plan", "p1"]);
  assert.equal(call.feedback, "Looks good.\nShip it.");

  assert.deepEqual(entries(f).at(-1), { plan: "p1", review: "r1", plannotator_id: "pn-1", status: "approved" });
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
  const d = decisions(f)[0]!;
  assert.equal(d.message.content, "Plan review r1 was APPROVED.\nFeedback from the reviewer:\nLooks good.\nShip it.\nNext: cpd plan lock");
  assert.deepEqual(d.options, { triggerTurn: true, deliverAs: "followUp" });
  assert.equal(f.pi.delivered().length, 0, "the custom type differs from cpd, so it never moves the cursor");

  result(f, { reviewId: "pn-1", approved: true }); // a duplicate result settles nothing
  await f.quiet();
  assert.equal(f.argvs().filter((a) => a[1] === "decide").length, 1);
});

test("rejection without feedback: no --feedback-file, and the Lead is told to revise", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  result(f, { reviewId: "pn-1", approved: false });
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.deepEqual(f.argvs().find((a) => a[1] === "decide"), ["plan", "decide", "r1", "--reject", "--plan", "p1"]);
  assert.equal(decisions(f)[0]!.message.content, "Plan review r1 was REJECTED.\nRevise the plan (job edit, job add, job cancel), then call cpd_plan_review again.");
  assert.equal(entries(f).at(-1).status, "rejected");
});

test("an approval that is no longer current asks for a new review", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: false, next: "x" });
  result(f, { reviewId: "pn-1", approved: true });
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.equal(decisions(f)[0]!.message.content, "Plan review r1 was APPROVED.\nThe Jobs changed since that review. Call cpd_plan_review again for a new review.");
});

test("review_decided: another caller decided it; the adapter continues with that decision", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.respond("plan-decide", { ok: false, error: { code: "review_decided", message: "Already decided.", details: {} } }, { exit: 3 });
  f.respond("plan-show", pending("r1", { status: "rejected", feedback: "Not yet." }));
  result(f, { reviewId: "pn-1", approved: true });
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.equal(decisions(f)[0]!.message.content, "Plan review r1 was REJECTED.\nFeedback from the reviewer:\nNot yet.\nRevise the plan (job edit, job add, job cancel), then call cpd_plan_review again.");
  assert.equal(entries(f).at(-1).status, "rejected");
  assert.equal(f.pi.errors().length, 0);
});

test("any other failure of plan decide is an error and leaves the review pending, so a later result retries", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.respond("plan-decide", { ok: false, error: { code: "store_failed", message: "Disk full.", details: {} } }, { exit: 1 });
  result(f, { reviewId: "pn-1", approved: true });
  await waitFor("the error", () => f.pi.errors().length === 1);
  assert.match(f.pi.errors()[0]!.message.content, /^cpd adapter: plan decide r1: store_failed: Disk full\.$/);
  assert.equal(entries(f).at(-1).status, "pending");
  assert.equal(decisions(f).length, 0);
  assert.equal(stateCalls(f).at(-1), REVIEW_LABEL, "still blocked");

  f.respond("plan-decide", { ok: true, current: true });
  result(f, { reviewId: "pn-1", approved: true });
  await waitFor("the decision message", () => decisions(f).length === 1);
});

test("results for reviews this session did not open are ignored", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  result(f, { reviewId: "someone-else", approved: true });
  result(f, "garbage");
  result(f, { reviewId: "pn-1" });
  await f.quiet();
  assert.equal(f.argvs().some((a) => a[1] === "decide"), false);
});

// ---- decided outside plannotator (§15.5 "Deciding elsewhere") ----

const NOTE = "This review was decided outside plannotator. plannotator offers no way to close its review page from here, so the page stays open: ignore it or close the tab.";
const decidedEvent = (id: number, status: string, feedback: string | null, review = "r1") => ev(id, "plan-review", { data: { review, fingerprint: "ff", status, feedback } });

for (const [status, feedback, text] of [
  ["approved", "Ship it.", `Plan review r1 was APPROVED.\nFeedback from the reviewer:\nShip it.\nNext: cpd plan lock\n${NOTE}`],
  ["rejected", null, `Plan review r1 was REJECTED.\nRevise the plan (job edit, job add, job cancel), then call cpd_plan_review again.\n${NOTE}`],
] as const) {
  test(`an Event-only ${status} unblocks the Lead and tells it once, without plan decide; a late plannotator result is ignored`, async (t) => {
    const f = new Fixture();
    t.after(() => f.close());
    await opens(f);
    f.respond("plan-show", pending("r1", { status }));
    f.emit(decidedEvent(102, status, feedback));
    await waitFor("the decision message", () => decisions(f).length === 1);

    assert.equal(decisions(f)[0]!.message.content, text);
    assert.deepEqual(decisions(f)[0]!.options, { triggerTurn: true, deliverAs: "followUp" });
    assert.deepEqual(entries(f).at(-1), { plan: "p1", review: "r1", plannotator_id: "pn-1", status });
    await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");

    f.emit(decidedEvent(102, status, feedback)); // replayed
    result(f, { reviewId: "pn-1", approved: status === "approved", feedback: "late" }); // plannotator's own result, later
    await f.drain();
    assert.equal(decisions(f).length, 1);
    assert.equal(f.argvs().some((a) => a[1] === "decide"), false, "never a second plan decide");
    assert.equal(entries(f).filter((e: any) => e.status === status).length, 1);
    assert.equal(f.pi.errors().length, 0);
  });
}

test("an Event-only decision of a review the Plan has superseded says the Jobs changed", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.respond("plan-show", pending("r1", { status: "approved", current: false }));
  f.emit(decidedEvent(102, "approved", null));
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.match(decisions(f)[0]!.message.content, /^Plan review r1 was APPROVED\.\nThe Jobs changed since that review\. Call cpd_plan_review again for a new review\./);
});

test("an Event-only decision for a review this session has no pending entry for changes nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.emit(decidedEvent(102, "approved", null, "r9"));
  await f.drain();
  assert.deepEqual(decisions(f), []);
  assert.equal(entries(f).at(-1).status, "pending");
});

// ---- latest only (hs-d224) ----

test("r3 then r4 pending: only r4 blocks; r4 approved leaves the Lead not blocked though r3 stays pending", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  let n = 0;
  plannotator(f, (req) => (req.action === "plan-review" ? opened(`pn-${++n}`) : undefined));
  f.planFile("p1", "r3", "# r3\n");
  f.planFile("p1", "r4", "# r4\n");
  await lead(f, pending("r3"));
  f.emit(reviewEvent(101, "r3"));
  await waitFor("r3 blocks", () => stateCalls(f).includes(REVIEW_LABEL));
  f.respond("plan-show", pending("r4"));
  f.emit(reviewEvent(102, "r4"));
  await waitFor("both entries", () => entries(f).length === 2);
  assert.deepEqual(stateCalls(f), ["idle", REVIEW_LABEL], "still blocked by r4, no state change");

  f.respond("plan-show", pending("r4", { status: "approved" }));
  f.emit(decidedEvent(103, "approved", null, "r4"));
  await waitFor("the decision message", () => decisions(f).length === 1);
  await waitFor("not blocked", () => stateCalls(f).at(-1) === "idle");
  assert.equal(entries(f).find((e: any) => e.review === "r3").status, "pending", "r3 is never decided");
  assert.equal(entries(f).at(-1).review, "r4");
});

test("a replayed request for an older review does not close the newer reason", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, (req) => (req.action === "plan-review" ? opened() : undefined));
  f.planFile("p1", "r2", "# r2\n");
  await lead(f, pending("r1"));
  f.emit(reviewEvent(101, "r1"));
  await waitFor("r1 blocks", () => stateCalls(f).includes(REVIEW_LABEL));
  f.respond("plan-show", pending("r2"));
  f.emit(reviewEvent(102, "r2"));
  await waitFor("both entries", () => entries(f).length === 2);
  f.emit(reviewEvent(103, "r1")); // a replayed older request must not close the newer reason
  await f.drain(); // marker 104
  assert.deepEqual(stateCalls(f), ["idle", REVIEW_LABEL]);
});

test("a restart with r3 and r4 entries and plan show reporting r4 approved: not blocked", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "pending" } }));
  seeded(f, { review: "r3", plannotator_id: "pn-3" });
  seeded(f, { review: "r4", plannotator_id: "pn-4", status: "approved" });
  await lead(f, pending("r4", { status: "approved" }));
  await f.quiet(); // recovery starts inside the start-up that `lead` awaited
  assert.deepEqual(stateCalls(f), ["idle"]);
});

test("a restart with r3 and r4 both pending in the session: only r4 blocks, and r4's decision unblocks", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "pending" } }));
  seeded(f, { review: "r3", plannotator_id: "pn-3" });
  seeded(f, { review: "r4", plannotator_id: "pn-4" });
  await lead(f, pending("r4"));
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  f.respond("plan-show", pending("r4", { status: "approved" }));
  f.emit(decidedEvent(103, "approved", null, "r4"));
  await waitFor("not blocked", () => stateCalls(f).at(-1) === "idle");
});

test("a restart with a single pending latest review still blocks", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "pending" } }));
  seeded(f, { review: "r3", plannotator_id: "pn-3" });
  await lead(f, pending("r3"));
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
});

test("a failing plan show at start-up: the session's highest review stands for the latest", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "pending" } }));
  seeded(f, { review: "r2", plannotator_id: "pn-2" });
  seeded(f, { review: "r3", plannotator_id: "pn-3" });
  f.respond("plan-show", { ok: false, error: { code: "store_busy", message: "busy" } }, { exit: 1 });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  f.emit(decidedEvent(103, "approved", null, "r2")); // not the latest: its reason was never open, nothing changes
  await f.drain(); // marker 104
  assert.deepEqual(stateCalls(f), ["idle", REVIEW_LABEL]);
});

// ---- recovery ----

const seeded = (f: Fixture, over: Record<string, unknown> = {}) =>
  f.pi.entries.push({ type: "custom", customType: "cpd-plan-review", data: { plan: "p1", review: "r1", plannotator_id: "pn-1", status: "pending", ...over } });

test("recovery: review-status every pollMs until completed, which is handled as a result", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  let polls = 0;
  let others = 0;
  plannotator(f, (req) => {
    if (req.action !== "review-status") return undefined;
    if (req.payload.reviewId === "pn-2") return others++, { status: "handled", result: { status: "pending" } }; // a second review that keeps polling: its polls are the clock
    assert.deepEqual(req.payload, { reviewId: "pn-1" });
    return { status: "handled", result: ++polls < 3 ? { status: "pending" } : { status: "completed", approved: false, feedback: "no" } };
  });
  seeded(f);
  seeded(f, { review: "r2", plannotator_id: "pn-2" });
  await lead(f);
  await waitFor("blocked at start-up", () => stateCalls(f).includes(REVIEW_LABEL));
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.equal(polls, 3);
  assert.deepEqual(f.argvs().find((a) => a[1] === "decide")!.slice(0, 4), ["plan", "decide", "r1", "--reject"]);
  assert.match(decisions(f)[0]!.message.content, /^Plan review r1 was REJECTED\./);
  assert.equal(entries(f).at(-1).status, "rejected");
  const seen = others;
  await waitFor("three more polls of the other review", () => others >= seen + 3); // two poll intervals pass: r1's loop would have polled again
  assert.equal(polls, 3, "polling stopped");
});

test("recovery: missing (plannotator restarted) records the entry as lost, unblocks, and tells the Lead", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "missing" } }));
  seeded(f);
  await lead(f);
  await waitFor("the message", () => decisions(f).length === 1);
  assert.equal(entries(f).at(-1).status, "lost");
  assert.match(decisions(f)[0]!.message.content, /Call cpd_plan_review again\.$/);
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
});

test("recovery ignores another Plan's entries (a fork's file starts with its source's) and settled ones", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "missing" } }));
  seeded(f, { plan: "other", review: "r9", plannotator_id: "pn-9" });
  seeded(f, { review: "r2", status: "approved" });
  await lead(f);
  await f.quiet(); // recovery starts inside the start-up that `lead` awaited
  assert.equal(requests(f).length, 0);
  assert.deepEqual(stateCalls(f), ["idle"]);
});

test("recovery: plannotator not answering is reported once and polling goes on", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  seeded(f);
  await lead(f);
  await waitFor("the error", () => f.pi.errors().length === 1, 4000);
  // The error is delivered once, however often the polling goes on: wait for polls after it (each one times out, which would report again).
  await waitFor("two more polls", () => requests(f, "review-status").length >= 3);
  assert.equal(f.pi.errors().length, 1);
  assert.match(f.pi.errors()[0]!.message.content, /plannotator_timeout/);
});

// ---- the tool ----

test("cpd_plan_review is registered for Plan Leads: it runs plan review and returns next", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  const tool = f.pi.tools.find((x) => x.name === "cpd_plan_review")!;
  assert.ok(tool);
  assert.deepEqual(tool.parameters, {
    type: "object",
    properties: { title: { type: "string", description: "One line of at most 72 characters naming what the drafts under review do" } },
    required: ["title"],
    additionalProperties: false,
  });
  assert.equal(tool.exposure, "model-only");

  f.respond("plan-review", { ok: true, review: { id: "r1", status: "pending" }, path: `${f.cpdHome}/run/p1/plan-r1.md`, next: "The User reviews the plan in plannotator. Stop and wait for the decision message." });
  const out = await f.pi.runTool(tool, { title: "T" });
  assert.deepEqual(f.argvs().find((a) => a[1] === "review"), ["plan", "review", "--plan", "p1", "--title", "T"]);
  assert.equal(out.text, "The User reviews the plan in plannotator. Stop and wait for the decision message.");
  await f.quiet();
  assert.equal(requests(f).length, 0, "a new request opens through its Event, not through the tool");
});

test("the tool: a call without a title passes an empty --title, so cpd's usage refuses it", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-review", { ok: false, error: { code: "usage", message: "--title is required.", details: {} } }, { exit: 2 });
  await assert.rejects(f.pi.runTool(f.pi.tools[0]!), (e: any) => e.code === "usage");
  assert.deepEqual(f.argvs().find((a) => a[1] === "review"), ["plan", "review", "--plan", "p1", "--title", ""]);
});

test("the tool passes CPD_PLAN to plan review", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  process.env.CPD_PLAN = "p1";
  f.respond("plan-review", { ok: true, review: { id: "r1" }, next: "n" });
  await f.pi.runTool(f.pi.tools[0]!, { title: "T" });
  assert.deepEqual(f.argvs().find((a) => a[1] === "review"), ["plan", "review", "--plan", "p1", "--title", "T"]);
});

test("the tool: `already` with no pending entry opens the review itself, so a review that failed to open can be retried", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  let answer: unknown = { status: "unavailable" };
  plannotator(f, () => answer);
  await lead(f);
  const tool = f.pi.tools[0]!;
  f.respond("plan-review", { ok: true, already: true, review: { id: "r1", status: "pending" }, next: "wait" });

  const first = await f.pi.runTool(tool, { title: "T" });
  assert.equal(first.text, "wait");
  assert.equal(f.pi.errors().length, 1, "the failed open is delivered");
  assert.deepEqual(entries(f), []);

  answer = opened();
  await f.pi.runTool(tool, { title: "T" });
  assert.deepEqual(entries(f), [{ plan: "p1", review: "r1", plannotator_id: "pn-1", status: "pending" }]);

  await f.pi.runTool(tool, { title: "T" }); // pending entry now: nothing to open
  assert.equal(requests(f, "plan-review").length, 2);
});

test("the tool: a refusal of plan review is thrown with its code", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-review", { ok: false, error: { code: "no_draft_jobs", message: "No draft Jobs.", details: {} } }, { exit: 3 });
  await assert.rejects(f.pi.runTool(f.pi.tools[0]!, { title: "T" }), (e: any) => e.code === "no_draft_jobs");
});

test("the tool is not registered for a Worker", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: true, session: { id: ID, role: "worker", context: "ctx", plan: "p1", attempt: "j-a1", name: "w", state: "stopped", plan_lead: false, grounded_lead: false, registered_event: 1 } });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  assert.deepEqual(f.pi.tools, []);
});

const feedbackDirs = () => readdirSync(tmpdir()).filter((n) => n.startsWith("cpd-feedback-"));

test("a decision, failing or not, creates no temp directory", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const before = feedbackDirs();
  await opens(f);
  f.respond("plan-decide", { error: { code: "store_error", message: "boom" } }, { exit: 1 });
  result(f, { reviewId: "pn-1", approved: false, feedback: "Nope." });
  await waitFor("the decide call", () => f.argvs().some((a) => a[1] === "decide"));
  await waitFor("the error", () => f.pi.errors().length === 1);
  assert.deepEqual(feedbackDirs(), before);
  assert.equal(readFileSync(join(f.cpdHome, "run", "p1", "feedback-r1.md"), "utf8"), "Nope.");
});

// ---- superseded (commands.md §15.3, §15.5 "Latest only") ----

const supersededEvent = (id: number, review = "r1") => ev(id, "plan-review", { data: { review, fingerprint: "ff", status: "superseded" } });
const chatSeed = (over: Record<string, unknown> = {}) => ({ plan: "p1", review: "r1", surface: "chat", status: "pending", ...over });

test("an Event-only superseded closes the reason and the dialog, records the entry and tells nobody", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", pending());
  f.planFile("p1", "r1", "# the plan\n");
  f.summaryFile("p1", "r1", "# Review\nsummary\n");
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(reviewEvent(101));
  const d = await waitFor("the select", () => f.pi.openDialog("select"));
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  const sentBefore = f.pi.sent.length;

  f.respond("plan-show", pending("r1", { status: "superseded" }));
  f.emit(supersededEvent(102));
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle", 3000);

  assert.equal(d.signal!.aborted, true);
  assert.deepEqual(entries(f).at(-1), chatSeed({ status: "superseded" }));
  assert.equal(f.argvs().some((a) => a[1] === "decide"), false);
  await f.drain();
  assert.equal(f.pi.sent.length, sentBefore, "no message and no turn");
  assert.equal(f.pi.errors().length, 0);
});

test("a superseded Event for a review this session has no pending entry for changes nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", pending());
  f.planFile("p1", "r1", "# the plan\n");
  f.summaryFile("p1", "r1", "# Review\nsummary\n");
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(reviewEvent(101));
  const d = await waitFor("the select", () => f.pi.openDialog("select"));
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  const sentBefore = f.pi.sent.length;
  const entriesBefore = entries(f).length;

  f.emit(supersededEvent(102, "r9")); // no entry for r9: nothing changes, r1's reason and dialog stay
  await f.drain();
  assert.deepEqual(stateCalls(f), ["idle", REVIEW_LABEL]);
  assert.equal(d.signal!.aborted, false);
  assert.equal(f.pi.sent.length, sentBefore);
  assert.equal(entries(f).length, entriesBefore);

  f.respond("plan-show", pending("r1", { status: "superseded" }));
  f.emit(supersededEvent(103, "r1")); // the same Event for r1 does act: the contrast pins that the r9 one was ignored on purpose
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle", 3000);
  assert.equal(f.pi.errors().length, 0);
});

test("a restart with plan show reporting the latest review superseded does not block or reopen it", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  plannotator(f, () => ({ status: "handled", result: { status: "pending" } }));
  f.pi.entries.push({ type: "custom", customType: "cpd-plan-review", data: chatSeed({ review: "r2" }) });
  seeded(f, { review: "r2", plannotator_id: "pn-2" }); // the same review's page: not polled as pending either
  f.planFile("p1", "r2", "# r2\n");
  f.summaryFile("p1", "r2", "# Review\nsummary\n");
  await lead(f, pending("r2", { status: "superseded" }));
  await f.quiet(); // recovery starts inside the start-up that `lead` awaited
  assert.deepEqual(stateCalls(f), ["idle"]);
  assert.equal(f.pi.dialogs.length, 0, "no dialog reopens");
  assert.equal(requests(f).length, 0, "no plannotator request either");

  const before = { sent: f.pi.sent.length, entries: entries(f).length, dialogs: f.pi.dialogs.length };
  f.emit(supersededEvent(101, "r2")); // replayed
  await f.drain();
  assert.deepEqual(stateCalls(f), ["idle"]);
  assert.deepEqual({ sent: f.pi.sent.length, entries: entries(f).length, dialogs: f.pi.dialogs.length }, before);
  assert.equal(f.pi.errors().length, 0);
});
