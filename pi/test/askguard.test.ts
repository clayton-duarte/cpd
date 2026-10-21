import assert from "node:assert/strict";
import { test } from "node:test";
import { Fixture, planShow, sessionShow, waitFor } from "./fake.ts";

// commands.md §15.13 (hs-d502, hs-d503): G1 (a Lead with draft Jobs) is deleted; any Session's `ask_user_question` over 1500 characters (G2) is refused.

type Option = { label: string; description: string; preview?: string };
type Question = { question: string; header: string; options: Option[]; multiSelect?: boolean };
type Verdict = { block?: boolean; reason?: string } | undefined;

const LIMIT = 1500;
const G2 = (n: number) => `cpd: ask_user_question is for a few short choices; put anything larger in plannotator; refused: ${n} characters (limit ${LIMIT})`;

const job = (key: string, status: string) => ({ key, pr: "api", title: key, status, tier: "S3", needs: [] });
const withJobs = (...statuses: string[]) => {
  const jobs = statuses.map((s, i) => job(`j${i}`, s));
  return planShow({ jobs, counts: { draft: statuses.filter((s) => s === "draft").length, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 } });
};

/** A question whose text is `question`, `header`, and one option of `label`, `description` and `preview` characters long (a missing preview is none) and an empty one; so the total is their sum. */
const ask = (question: number, header: number, option: [number, number, number?] = [0, 0]): Question => ({
  question: "q".repeat(question),
  header: "h".repeat(header),
  options: [{ label: "l".repeat(option[0]), description: "d".repeat(option[1]), ...(option[2] === undefined ? {} : { preview: "p".repeat(option[2]) }) }, { label: "", description: "" }],
});

async function started(role: "lead" | "manager", plan = withJobs()) {
  const f = new Fixture();
  f.respond("session-show", sessionShow({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", plan);
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return f;
}

const planReads = (f: Fixture) => f.argvs().filter((a) => a[0] === "plan" && a[1] === "show").length;

async function call(f: Fixture, questions: Question[], toolName = "ask_user_question"): Promise<Verdict> {
  const input = toolName === "bash" ? { command: "ls" } : { questions };
  return (await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input }))[0] as Verdict;
}

// ---- G1 is gone (hs-d502): a Lead with draft Jobs may ask ----

test("a Lead whose Plan has draft Jobs may call ask_user_question; the Plan is not read for it", async (t) => {
  const f = await started("lead", withJobs("draft", "queued", "draft", "merged"));
  t.after(() => f.close());
  const before = planReads(f);
  assert.equal(await call(f, [ask(10, 5)]), undefined);
  assert.equal(planReads(f), before, "the adapter no longer reads cpd plan show for the guard");
});

test("a Lead is not refused whatever its Jobs are, nor when plan show fails", async (t) => {
  const f = await started("lead", withJobs("draft"));
  t.after(() => f.close());
  for (const statuses of [["draft"], ["queued", "running", "merged", "failed", "cancelled"], []]) {
    f.respond("plan-show", withJobs(...statuses));
    assert.equal(await call(f, [ask(10, 5)]), undefined);
  }
  f.respond("plan-show", { ok: false, error: { code: "store_busy", message: "busy" } }, { exit: 1 });
  assert.equal(await call(f, [ask(10, 5)]), undefined);
});

test("a Manager is not refused for drafts", async (t) => {
  const f = await started("manager", withJobs("draft", "draft"));
  t.after(() => f.close());
  assert.equal(await call(f, [ask(10, 5)]), undefined);
});

test("other tools are unaffected by a Plan with drafts", async (t) => {
  const f = await started("lead", withJobs("draft", "draft"));
  t.after(() => f.close());
  assert.equal(await call(f, [], "bash"), undefined);
  assert.equal(await call(f, [ask(10, 5)], "cpd_plan_review"), undefined);
});

// ---- G2: a long question ----

for (const role of ["lead", "manager"] as const) {
  test(`G2: a ${role}'s call of 1501 characters is refused, naming the count; exactly 1500 passes`, async (t) => {
    const f = await started(role);
    t.after(() => f.close());
    assert.equal(await call(f, [ask(1000, 500)]), undefined, "1500 characters");
    const r = await call(f, [ask(1000, 501)]);
    assert.deepEqual(r, { block: true, reason: G2(1501) });
    assert.equal(r?.reason?.includes("\n"), false);
  });
}

test("G2: the count sums every question's `question` and `header` and every option's `label`, `description` and `preview`", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  // 100 + 50 per question, two questions; options 10 + 20 + 30 each, two options each: 300 + 2 * 2 * 60 = 540
  const two: Question[] = [0, 1].map(() => ({
    question: "q".repeat(100),
    header: "h".repeat(50),
    options: [0, 1].map(() => ({ label: "l".repeat(10), description: "d".repeat(20), preview: "p".repeat(30) })),
  }));
  assert.equal(await call(f, two), undefined);
  const exact = (n: number): Question[] => [{ question: "q".repeat(n), header: "", options: [] }];
  assert.equal(await call(f, [...two, ...exact(LIMIT - 540)]), undefined, "exactly 1500 over two questions and a third");
  assert.deepEqual(await call(f, [...two, ...exact(LIMIT - 540 + 1)]), { block: true, reason: G2(LIMIT + 1) });
});

test("G2: a long preview alone counts", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  assert.equal(await call(f, [ask(10, 5, [1, 1, LIMIT - 17])]), undefined, "1500 with the preview");
  assert.deepEqual(await call(f, [ask(10, 5, [1, 1, LIMIT - 16])]), { block: true, reason: G2(LIMIT + 1) });
  assert.deepEqual(await call(f, [ask(1, 1, [1, 1, 5000])]), { block: true, reason: G2(5004) });
});

test("G2: a long description alone counts", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  assert.deepEqual(await call(f, [ask(1, 1, [1, 3000])]), { block: true, reason: G2(3003) });
});

test("G2: a long label and a long header count", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  assert.deepEqual(await call(f, [ask(1, 1, [2000, 1])]), { block: true, reason: G2(2003) });
  assert.deepEqual(await call(f, [ask(1, 2000)]), { block: true, reason: G2(2001) });
});

test("G2: a Lead with draft Jobs over the limit is refused with G2's reason only", async (t) => {
  const f = await started("lead", withJobs("draft", "queued"));
  t.after(() => f.close());
  assert.deepEqual(await call(f, [ask(2000, 0)]), { block: true, reason: G2(2000) });
});

test("G2: a Lead's short question and a Manager's short question pass", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  assert.equal(await call(f, [ask(80, 12, [10, 40])]), undefined);
});

test("G2: a long bash command is unaffected", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  const long = "echo " + "x".repeat(5000);
  const r = (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command: long } }))[0];
  assert.equal(r, undefined);
});

test("before session_start no call is refused", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.load();
  assert.equal(await call(f, [ask(2000, 0)]), undefined);
});
