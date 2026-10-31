import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture, ev, planShow, waitFor as wait } from "./fake.ts";

// commands.md §15.5 "The adapter serializes its own dialogs" (hs-n607, hs-n618; Plan `ask-dialog-slot`):
// a tool dialog can hold pi's one editor slot (the inline `ask_user_question` questionnaire), and a dialog the
// adapter mounts over it displaces the questionnaire. So the adapter waits, event-driven, while a tool dialog
// holds the slot and mounts its own dialog when the tool dialog settles.

/** Everything here is local and fast: a red test should fail in seconds. */
const waitFor = <T>(what: string, fn: () => T | undefined | null | false, ms = 3000) => wait(what, fn, ms);

const TITLE = "Review title";
const SUMMARY = `# ${TITLE}\nPlan p1: Plan · review r1\n\nGoal: Show a review in chat\n\nApprove | Change\n`;
const OPTIONS = ["Approve", "Change"];

const pending = (id = "r1", over: Record<string, unknown> = {}) => planShow({ review: { id, status: "pending", fingerprint: "ff", current: true, title: TITLE, ...over } });
const reviewEvent = (id: number, review = "r1", status = "pending") => ev(id, "plan-review", { data: { review, fingerprint: "ff", status, title: TITLE } });

const summaries = (f: Fixture) => f.pi.sent.filter((s) => s.message.customType === "cpd-plan-summary");
const decisions = (f: Fixture) => f.pi.sent.filter((s) => s.message.customType === "cpd-plan-decision");
const decides = (f: Fixture) => f.argvs().filter((a) => a[1] === "decide");
const selects = (f: Fixture) => f.pi.dialogs.filter((d) => d.kind === "select");
const select = (f: Fixture) => waitFor("the select", () => f.pi.openDialog("select"));

/** A Lead (role `lead`, Plan p1) whose review r1 is pending, with the follower started. */
async function lead(f: Fixture) {
  f.respond("plan-show", pending());
  f.summaryFile("p1", "r1", SUMMARY);
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}

/** A tool dialog holds pi's editor slot: pi's `ui_prompt_start`, until its `ui_prompt_end`. */
const holdToolDialog = (f: Fixture) => f.pi.fire("ui_prompt_start", { reason: "ui_prompt", kind: "custom" });
const releaseToolDialog = (f: Fixture) => f.pi.fire("ui_prompt_end");
/** The loud safety release of §15.5: an `ask_user_question` `tool_result` with no `ui_prompt_end`. */
const loudAskResult = (f: Fixture) => f.pi.fire("tool_result", { toolName: "ask_user_question", toolCallId: "t1" });

const SECRET_ARGS = { action: "get", vault: "other", item: "db", field: "password", reason: "migrate the db" };
const secretCalls = (f: Fixture) => f.argvs().filter((a) => a[0] === "secret");
const secretTool = (f: Fixture) => {
  const tool = f.pi.tools.find((x) => x.name === "cpd_secret_approve");
  assert.ok(tool, "the tool cpd_secret_approve is registered");
  return tool;
};

test("a held tool dialog defers the plan-review select; it opens when the tool dialog settles and the review is decided", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.respond("plan-decide", { ok: true, review: { id: "r1", status: "approved" }, current: true, next: "cpd plan lock" });

  await holdToolDialog(f);
  f.emit(reviewEvent(101));
  await waitFor("the summary", () => summaries(f).length === 1);
  await f.quiet(); // the detached dialog attempt has run

  assert.equal(f.pi.openDialog("select"), undefined, "the adapter's select must not open while the tool dialog holds the slot");
  assert.deepEqual(selects(f), [], "no select is in pi's dialog list");

  await releaseToolDialog(f);
  const d = await select(f);
  d.answer("Approve");
  await waitFor("the decision", () => decisions(f).length === 1);
  assert.deepEqual(decides(f), [["plan", "decide", "r1", "--approve", "--plan", "p1"]], "both dialogs ran and the review was decided");
});

test("a held tool dialog defers the cpd_secret_approve select; it opens when the tool dialog settles", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  const tool = secretTool(f);

  await holdToolDialog(f);
  const running = f.pi.runTool(tool, SECRET_ARGS);
  await f.quiet(); // the tool's own dialog attempt has run

  assert.equal(f.pi.openDialog("select"), undefined, "cpd_secret_approve's select must not open while the tool dialog holds the slot");
  assert.deepEqual(selects(f), [], "no select is in pi's dialog list");

  await releaseToolDialog(f);
  const d = await select(f);
  d.answer("Allow once");
  const out = await running;
  assert.deepEqual(secretCalls(f), [["secret", "approve", "--action", "get", "--vault", "other", "--item", "db", "--field", "password"]], "the approval ran after the select opened");
  assert.equal(out.text, "Approved for one command in the next 10 minutes: run it now.");
});

test("a held ask that ends only as a loud result still releases the deferred adapter dialog", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);

  await holdToolDialog(f);
  f.emit(reviewEvent(101));
  await waitFor("the summary", () => summaries(f).length === 1);
  await f.quiet();
  assert.equal(f.pi.openDialog("select"), undefined, "the adapter's select must not open while the ask holds the slot");

  // The ask settles without a `ui_prompt_end`: its `tool_result` is the safety release (§15.5).
  await loudAskResult(f);
  const d = await select(f);
  assert.equal(d.title, `Plan review r1: ${TITLE}`);
  assert.deepEqual(d.options, OPTIONS);
});

// Unnamed regression pin: with no tool dialog held the select opens at once. Passes on origin/main and must keep passing.
test("the plan-review select opens at once when no tool dialog is held", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await lead(f);
  f.emit(reviewEvent(101));
  const d = await select(f);
  assert.equal(d.title, `Plan review r1: ${TITLE}`);
  assert.deepEqual(d.options, OPTIONS);
});
