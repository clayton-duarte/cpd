import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { Fixture, ev, planShow, stateCalls, waitFor as wait } from "./fake.ts";
import { adapterErrors } from "./lead-errors.ts";

/** Everything here is local and fast: a red test should fail in seconds. */
const waitFor = <T>(what: string, fn: () => T | undefined | null | false, ms = 3000) => wait(what, fn, ms);

// commands.md §15.5 "The Plan review in chat" (hs-d502): the summary in the Lead's chat and one dialog (Approve, Change).

const REVIEW_LABEL = "blocked:φ: plan review";
const TITLE = "Review title";
const SUMMARY = `# ${TITLE}\nPlan p1: Plan · review r1\n\nGoal: Show a review in chat\n\nApprove | Change\n`;
const OPTIONS = ["Approve", "Change"];
const STILL_WAITING = (r: string) => `Plan review ${r} is still waiting: /plan-review reopens it`;
const NONE_WAITING = "\x1b[1mφ\x1b[22m: no plan review is waiting in chat";

const pending = (id = "r1", over: Record<string, unknown> = {}) => planShow({ review: { id, status: "pending", fingerprint: "ff", current: true, title: TITLE, ...over } });
const reviewEvent = (id: number, review = "r1", status = "pending") => ev(id, "plan-review", { data: { review, fingerprint: "ff", status, title: TITLE } });
const decidedEvent = (id: number, status: string, feedback: string | null, review = "r1") => ev(id, "plan-review", { data: { review, fingerprint: "ff", status, feedback } });
const entries = (f: Fixture) => f.pi.entries.filter((e: any) => e.type === "custom" && e.customType === "cpd-plan-review").map((e: any) => e.data);
const decisions = (f: Fixture) => f.pi.sent.filter((s) => s.message.customType === "cpd-plan-decision");
const summaries = (f: Fixture) => f.pi.sent.filter((s) => s.message.customType === "cpd-plan-summary");
const decides = (f: Fixture) => f.argvs().filter((a) => a[1] === "decide");
const chatEntry = (over: Record<string, unknown> = {}) => ({ plan: "p1", review: "r1", surface: "chat", status: "pending", summary: SUMMARY, title: TITLE, ...over });
const select = (f: Fixture) => waitFor("the select", () => f.pi.openDialog("select"));
const nSelects = (f: Fixture) => f.pi.dialogs.filter((d) => d.kind === "select").length;
const feedbackFile = (f: Fixture, review = "r1") => join(f.cpdHome, "run", "p1", `feedback-${review}.md`);

async function lead(f: Fixture, show: unknown = pending(), files: string[] = ["r1"]) {
  f.respond("plan-show", show);
  for (const r of files) {
    f.summaryFile("p1", r, SUMMARY.replaceAll("r1", r));
  }
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

/** A Lead whose review r1 is open in chat: the dialog's select is waiting. */
async function opens(f: Fixture) {
  await lead(f);
  f.emit(reviewEvent(101));
  return select(f);
}

const seeded = (f: Fixture, over: Record<string, unknown> = {}) =>
  f.pi.entries.push({ type: "custom", customType: "cpd-plan-review", data: chatEntry(over) });

test("a pending plan-review Event shows the summary in chat without a turn, records a chat entry, blocks the Lead and opens the select", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.emit(reviewEvent(101));
  const d = await select(f);

  assert.equal(summaries(f).length, 1);
  const s = summaries(f)[0]!;
  assert.equal(s.message.content, SUMMARY, "the text of summary-r1.md");
  assert.equal(s.message.display, true);
  assert.deepEqual(s.message.details, { plan: "p1", review: "r1" });
  assert.deepEqual(s.options, { triggerTurn: false });
  assert.equal(d.title, `Plan review r1: ${TITLE}`);
  assert.deepEqual(d.options, OPTIONS);
  assert.ok(d.signal, "a signal the adapter can abort");
  assert.deepEqual(entries(f), [chatEntry()]);
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  assert.deepEqual(f.pi.sent.filter((x) => x !== s), [], "no other message, no turn");

  f.emit(reviewEvent(102)); // replay: the session has an entry for r1
  await f.drain();
  assert.equal(summaries(f).length, 1);
  assert.equal(nSelects(f), 1);
});

test("the dialog is detached: Events keep being handled while it is open", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  f.emit(ev(110, "job-merged", { data: { turn: true } }));
  await waitFor("the Event delivered", () => f.pi.delivered().some((s) => (s.message.details as any)?.id === 110));
});

test("a superseded or decided request is skipped", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, pending("r3"), ["r1", "r2", "r3"]);
  f.emit(reviewEvent(101, "r2")); // not the latest
  await f.drain();
  assert.equal(nSelects(f), 0);
  assert.deepEqual(summaries(f), []);
  f.respond("plan-show", pending("r3", { status: "approved" }));
  f.emit(reviewEvent(110, "r3"));
  await f.drain();
  assert.equal(nSelects(f), 0);
});

test("a failed read of the summary is an error without a turn; nothing is written", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-show", pending("r2"));
  // summary-r2.md exists as a directory: reading it fails
  const { mkdirSync } = await import("node:fs");
  mkdirSync(join(f.cpdHome, "run", "p1", "summary-r2.md"));
  f.emit(reviewEvent(101, "r2"));
  await waitFor("the error", () => adapterErrors(f).length === 1);
  assert.match(adapterErrors(f)[0]!.message.content, /^φ adapter: plan review r2: file_unreadable: /);
  assert.deepEqual(entries(f), []);
  assert.equal(nSelects(f), 0);
  assert.equal(stateCalls(f).includes(REVIEW_LABEL), false);
});

test("Approve: plan decide --approve, the entry, the reason closed, the Lead told with today's text", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: true, next: "cpd plan lock" });
  d.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);

  assert.deepEqual(decides(f), [["plan", "decide", "r1", "--approve", "--plan", "p1"]]);
  assert.deepEqual(entries(f).at(-1), chatEntry({ status: "approved" }));
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
  assert.equal(decisions(f)[0]!.message.content, "Plan review r1 was APPROVED.\nNext: cpd plan lock");
  assert.deepEqual(decisions(f)[0]!.options, { triggerTurn: true, deliverAs: "followUp" });
  assert.equal(f.pi.dialogs.length, 1, "no further dialog");
});

test("Approve of a review that is no longer current asks for a new review", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: false, next: "x" });
  d.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.equal(decisions(f)[0]!.message.content, "Plan review r1 was APPROVED.\nThe Jobs changed since that review. Call cpd_plan_review again for a new review.");
});

test("Change with a note: the editor, the note in feedback-<review>.md, plan decide --reject --feedback-file, the Lead told", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  d.answer("Change");
  const e = await waitFor("the editor", () => f.pi.openDialog("editor"));
  assert.equal(e.title, "What should change?");
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "rejected" }, current: true });
  e.answer("Split the first PR.\nDrop A2.");
  await waitFor("the decision message", () => decisions(f).length === 1);

  assert.deepEqual(decides(f), [["plan", "decide", "r1", "--reject", "--feedback-file", feedbackFile(f), "--plan", "p1"]]);
  assert.equal(readFileSync(feedbackFile(f), "utf8"), "Split the first PR.\nDrop A2.");
  assert.equal(f.calls().find((c) => c.phase === "end" && c.argv[1] === "decide")!.feedback, "Split the first PR.\nDrop A2.");
  assert.deepEqual(entries(f).at(-1), chatEntry({ status: "rejected" }));
  assert.equal(
    decisions(f)[0]!.message.content,
    "Plan review r1 was REJECTED.\nFeedback from the reviewer:\nSplit the first PR.\nDrop A2.\nRevise the plan (job edit, job add, job cancel), then call cpd_plan_review again.",
  );
  assert.deepEqual(decisions(f)[0]!.options, { triggerTurn: true, deliverAs: "followUp" });
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
});

for (const [name, note] of [["an empty note", ""], ["a note of spaces and newlines", "  \n\t "], ["a cancelled editor", undefined]] as const) {
  test(`Change with ${name} returns to the select; nothing is recorded`, async (t) => {
    const f = new Fixture();
    t.after(() => f.close());
    const d = await opens(f);
    d.answer("Change");
    const e = await waitFor("the editor", () => f.pi.openDialog("editor"));
    e.answer(note);
    await waitFor("the select again", () => nSelects(f) === 2 && f.pi.openDialog("select"));
    const again = f.pi.openDialog("select")!;
    assert.equal(again.title, `Plan review r1: ${TITLE}`);
    assert.deepEqual(again.options, OPTIONS);
    assert.deepEqual(decides(f), []);
    assert.equal(existsSync(feedbackFile(f)), false);
    assert.deepEqual(entries(f), [chatEntry()]);
    assert.equal(summaries(f).length, 1, "the summary is not sent again");
    assert.equal(stateCalls(f).at(-1), REVIEW_LABEL);
    assert.deepEqual(decisions(f), []);
  });
}

test("a failed plan decide from the dialog is an error with a turn, the entry stays pending and the select opens again", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  f.respond("plan-decide", { ok: false, error: { code: "store_failed", message: "Disk full.", details: {} } }, { exit: 1 });
  d.answer("Approve");
  await waitFor("the error", () => adapterErrors(f).length === 1);
  assert.match(adapterErrors(f)[0]!.message.content, /^φ adapter: plan decide r1: store_failed: Disk full\.$/);
  await waitFor("the select again", () => nSelects(f) === 2 && f.pi.openDialog("select"));
  assert.equal(entries(f).at(-1).status, "pending");
  assert.deepEqual(decisions(f), []);
  assert.equal(stateCalls(f).at(-1), REVIEW_LABEL);

  f.respond("plan-decide", { ok: true, current: true });
  f.pi.openDialog("select")!.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);
});

test("review_decided from the dialog: the adapter continues with the decision cpd holds", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  f.respond("plan-decide", { ok: false, error: { code: "review_decided", message: "Already decided.", details: {} } }, { exit: 3 });
  f.respond("plan-show", pending("r1", { status: "rejected", feedback: "Not yet." }));
  d.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.match(decisions(f)[0]!.message.content, /^Plan review r1 was REJECTED\.\nFeedback from the reviewer:\nNot yet\./);
  assert.equal(entries(f).at(-1).status, "rejected");
  assert.equal(adapterErrors(f).length, 0);
});

test("a review without summary-<review>.md is an error without a turn; nothing is written", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", pending());
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  f.emit(reviewEvent(101));
  await waitFor("the error", () => adapterErrors(f).length === 1);
  assert.match(adapterErrors(f)[0]!.message.content, /^φ adapter: plan review r1: file_unreadable: /);
  assert.deepEqual(entries(f), []);
  assert.equal(nSelects(f), 0);
});

// ---- Esc and /plan-review ----

test("Esc: nothing is recorded, the review stays pending and blocking, one notice says how to reopen", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  d.answer(undefined);
  await waitFor("the notice", () => f.pi.notices.includes(STILL_WAITING("r1")));
  await f.quiet();
  assert.equal(f.pi.notices.filter((n) => n === STILL_WAITING("r1")).length, 1);
  assert.deepEqual(entries(f), [chatEntry()]);
  assert.deepEqual(decides(f), []);
  assert.equal(stateCalls(f).at(-1), REVIEW_LABEL, "the reason stays open");
  assert.equal(nSelects(f), 1, "no select opens by itself");
  assert.deepEqual(decisions(f), []);
  assert.equal(summaries(f).length, 1);
});

test("/plan-review reopens the dialog of the pending chat review, without a new summary", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  d.answer(undefined);
  await waitFor("the notice", () => f.pi.notices.includes(STILL_WAITING("r1")));

  const run = f.pi.commands.get("plan-review");
  assert.ok(run, "registered");
  await run("", f.pi.commandCtx());
  const again = await waitFor("the select again", () => f.pi.openDialog("select"));
  assert.equal(again.title, `Plan review r1: ${TITLE}`);
  assert.deepEqual(again.options, OPTIONS);
  assert.equal(summaries(f).length, 1);

  await run("", f.pi.commandCtx()); // a dialog is already open: nothing
  assert.equal(nSelects(f), 2);

  f.respond("plan-decide", { ok: true, current: true, next: "cpd plan lock" });
  again.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.deepEqual(decides(f), [["plan", "decide", "r1", "--approve", "--plan", "p1"]]);
});

test("/plan-review with no pending chat review says so and opens nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  const run = f.pi.commands.get("plan-review")!;
  assert.ok(run, "registered");
  await run("", f.pi.commandCtx());
  assert.deepEqual(f.pi.notices, [NONE_WAITING]);
  assert.equal(nSelects(f), 0);
});

test("/plan-review is not registered for a Manager", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", { ok: true, session: { id: "x", role: "manager", context: "ctx", plan: null, attempt: null, name: "m", state: "idle", plan_lead: false, grounded_lead: false, registered_event: 1, loop_guard_turns: 20 } });
  f.load();
  assert.equal(f.pi.commands.has("plan-review"), false);
});

// ---- opening a newer review, deciding elsewhere ----

test("a newer pending review aborts the older review's dialog, which records nothing, and opens its own", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f, pending("r1"), ["r1", "r2"]);
  f.emit(reviewEvent(101, "r1"));
  const first = await select(f);
  f.respond("plan-show", pending("r2"));
  f.emit(reviewEvent(102, "r2"));
  await waitFor("the second select", () => nSelects(f) === 2);
  assert.equal(first.signal!.aborted, true);
  assert.equal(first.settled, true);
  const second = f.pi.openDialog("select")!;
  assert.equal(second.title, `Plan review r2: ${TITLE}`);
  assert.deepEqual(decides(f), []);
  assert.equal(f.pi.notices.includes(STILL_WAITING("r1")), false, "an abort is not Esc");
  assert.equal(entries(f).find((e: any) => e.review === "r1").status, "pending");
  assert.deepEqual(stateCalls(f), ["idle", REVIEW_LABEL]);
});

for (const [status, feedback, text] of [
  ["approved", "Ship it.", "Plan review r1 was APPROVED.\nFeedback from the reviewer:\nShip it.\nNext: cpd plan lock"],
  ["rejected", null, "Plan review r1 was REJECTED.\nRevise the plan (job edit, job add, job cancel), then call cpd_plan_review again."],
] as const) {
  test(`deciding elsewhere (${status}): the open select is aborted, the Lead is told once, no plan decide and no note of a page`, async (t) => {
    const f = new Fixture();
    t.after(() => f.close());
    const d = await opens(f);
    f.respond("plan-show", pending("r1", { status }));
    f.emit(decidedEvent(102, status, feedback));
    await waitFor("the decision message", () => decisions(f).length === 1);

    assert.equal(d.signal!.aborted, true);
    assert.equal(d.settled, true);
    const content = decisions(f)[0]!.message.content;
    assert.ok(content.startsWith(`${text}\n`), content);
    assert.match(content, /decided outside/);
    assert.deepEqual(decisions(f)[0]!.options, { triggerTurn: true, deliverAs: "followUp" });
    assert.deepEqual(entries(f).at(-1), chatEntry({ status }));
    await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
    await f.drain();
    assert.equal(decisions(f).length, 1);
    assert.deepEqual(decides(f), []);
    assert.equal(f.pi.notices.includes(STILL_WAITING("r1")), false, "the abort is not Esc");
    assert.equal(nSelects(f), 1);
  });
}

test("deciding elsewhere while the editor of Change is open: its late note is ignored (no plan decide, no new select)", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  d.answer("Change");
  const e = await waitFor("the editor", () => f.pi.openDialog("editor"));
  f.respond("plan-show", pending("r1", { status: "approved" }));
  f.emit(decidedEvent(102, "approved", null));
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.equal(e.settled, false, "an editor cannot be aborted");

  e.answer("Too late.");
  await f.drain();
  assert.deepEqual(decides(f), []);
  assert.equal(nSelects(f), 1, "no new select");
  assert.equal(decisions(f).length, 1);
  assert.equal(entries(f).filter((x: any) => x.status === "approved").length, 1);
  assert.equal(adapterErrors(f).length, 0);
});

test("deciding elsewhere while the User chose Approve at the same moment: the late answer changes nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  f.respond("plan-show", pending("r1", { status: "rejected" }));
  f.emit(decidedEvent(102, "rejected", null));
  await waitFor("the decision message", () => decisions(f).length === 1);
  d.answer("Approve"); // already settled by the abort: pi would not deliver it; the adapter must not act on it either
  await f.drain();
  assert.deepEqual(decides(f), []);
  assert.equal(decisions(f).length, 1);
});

// ---- Recovery ----

test("recovery: a pending chat entry of the latest pending review reopens its dialog, blocks the Lead, and sends no summary", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  seeded(f);
  await lead(f);
  const d = await select(f);
  assert.equal(d.title, `Plan review r1: ${TITLE}`);
  assert.deepEqual(d.options, OPTIONS);
  assert.deepEqual(summaries(f), []);
  await waitFor("blocked at start-up", () => stateCalls(f).includes(REVIEW_LABEL));

  f.respond("plan-decide", { ok: true, current: true, next: "cpd plan lock" });
  d.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);
  assert.deepEqual(decides(f), [["plan", "decide", "r1", "--approve", "--plan", "p1"]]);
});

test("recovery: a chat entry whose review plan show reports decided is not reopened", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  seeded(f);
  await lead(f, pending("r1", { status: "approved" }));
  await f.quiet();
  assert.equal(nSelects(f), 0);
  assert.deepEqual(stateCalls(f), ["idle"]);
});

test("recovery: a chat entry of an older review than the latest is not reopened", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  seeded(f);
  await lead(f, pending("r2"), ["r1", "r2"]);
  await f.quiet();
  assert.equal(nSelects(f), 0);
  assert.deepEqual(stateCalls(f), ["idle"]);
});

test("recovery: a restart after a chat review was decided in the dialog opens nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  seeded(f);
  seeded(f, { status: "approved" });
  await lead(f, pending("r1", { status: "approved" }));
  await f.quiet();
  assert.equal(nSelects(f), 0);
});

test("an entry of an older plannotator page is a pending chat review: it blocks and reopens the dialog", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.pi.entries.push({ type: "custom", customType: "cpd-plan-review", data: { plan: "p1", review: "r1", plannotator_id: "pn-1", status: "pending" } });
  await lead(f);
  await select(f);
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  assert.deepEqual(decides(f), []);
  assert.deepEqual(summaries(f), []);
});

// ---- the tool ----

test("the tool: `already` with no pending entry opens the review in chat; with one, nothing", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  const tool = f.pi.tools.find((x) => x.name === "cpd_plan_review")!;
  f.respond("plan-review", { ok: true, already: true, review: { id: "r1", status: "pending" }, summary: join(f.cpdHome, "run", "p1", "summary-r1.md"), next: "wait" });
  const out = await f.pi.runTool(tool, { title: "T" });
  assert.equal(out.text, "wait", "returns at once, the dialog is detached");
  const d = await select(f);
  assert.deepEqual(d.options, OPTIONS);
  assert.equal(summaries(f).length, 1);
  assert.deepEqual(entries(f), [chatEntry()]);

  await f.pi.runTool(tool, { title: "T" });
  await f.quiet();
  assert.equal(summaries(f).length, 1);
  assert.equal(nSelects(f), 1);
});

// hs-d524: one ask, one Inbox item. The review's own dialogs open no `dialog` reason.
const PROMPT = { reason: "ui_prompt", kind: "select" };
const dialogCalls = (f: Fixture) => f.argvs().filter((a) => a[1] === "state" && a.includes("dialog"));

test("hs-d524: the review's select opens no dialog reason; the state stays blocked on φ: plan review", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await opens(f);
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  await f.pi.fire("ui_prompt_start", PROMPT);
  await f.drain();
  assert.equal(stateCalls(f).at(-1), REVIEW_LABEL);
  assert.deepEqual(dialogCalls(f), []);
  assert.equal(stateCalls(f).includes("blocked:dialog"), false);
});

test("hs-d524: the review's Change editor opens no dialog reason", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  d.answer("Change");
  await waitFor("the editor", () => f.pi.openDialog("editor"));
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "editor" });
  await f.drain();
  assert.equal(stateCalls(f).at(-1), REVIEW_LABEL);
  assert.deepEqual(dialogCalls(f), []);
});

test("hs-d524: the prompt end after an answer changes no reason", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  await waitFor("blocked", () => stateCalls(f).includes(REVIEW_LABEL));
  await f.pi.fire("ui_prompt_start", PROMPT);
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: true, next: "cpd plan lock" });
  d.answer("Approve");
  await waitFor("the decision message", () => decisions(f).length === 1);
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
  const before = stateCalls(f);
  await f.pi.fire("ui_prompt_end");
  await f.drain();
  assert.deepEqual(stateCalls(f), before);
  assert.deepEqual(dialogCalls(f), []);
});

test("hs-d524: a prompt with no review dialog open still opens the dialog reason", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  await f.pi.fire("ui_prompt_start", PROMPT); // the review's own select: no reason
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: true, next: "cpd plan lock" });
  d.answer("Approve");
  await waitFor("unblocked", () => stateCalls(f).at(-1) === "idle");
  await f.pi.fire("ui_prompt_end");
  await f.drain();
  assert.deepEqual(dialogCalls(f), [], "the review's prompt opened no dialog reason");
  // no review dialog is open any more (the counter is back to 0): another prompt opens `dialog`
  await f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
  await waitFor("blocked dialog", () => stateCalls(f).at(-1) === "blocked:dialog");
  assert.equal(dialogCalls(f).length, 1);
});
