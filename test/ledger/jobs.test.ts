import { before, test } from "node:test";
import assert from "node:assert/strict";
import { CpdError } from "../../src/lib/errors.ts";
import {
  addJob, cancelJob, editJob, findCycle, getJobByKey, listJobs, parseSpec, retryJob, showJob,
} from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, allIssues, approvePlan, bdJson, CATALOG, jobId, makeAttempt, newPlan, sandbox, setJobStatus, spec } from "./helpers.ts";

const { repo } = sandbox();
const wf = "jobs";

const refusal = (code: string, exit: number) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;
const input = (over: Record<string, unknown> = {}) => ({ key: "x", title: "X", tier: "S3", reviewTier: "S4", spec: spec(), needs: [] as string[], ...over });

before(async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await addDraft(wf, "b", ["a"]);
});

test("a draft Job stores its Spec and needs; edit works on a draft", async () => {
  const b = await getJobByKey(wf, "b");
  assert.deepEqual(
    { status: b.status, tier: b.tier, reviewTier: b.reviewTier, goal: b.goal, needs: b.needs, touches: b.touches, acceptance: b.acceptance, outOfScope: b.outOfScope, context: b.context },
    { status: "draft", tier: "S3", reviewTier: "S4", goal: "goal of b", needs: ["a"], touches: ["src/a.ts"], acceptance: ["npm test passes"], outOfScope: ["everything else"], context: "because" },
  );
  const edited = await editJob(wf, "b", { title: "B renamed", tier: "H2", spec: spec({ goal: "new goal", touches: ["x", "y"] }), needs: [] }, CATALOG);
  assert.equal(edited.title, "B renamed");
  assert.equal(edited.tier, "H2");
  assert.equal(edited.goal, "new goal");
  assert.deepEqual(edited.touches, ["x", "y"]);
  assert.deepEqual(edited.needs, []);
  assert.equal(edited.metadata.key, "b");
  await editJob(wf, "b", { needs: ["a"], title: "-starts with a dash" }, CATALOG);
  const again = await getJobByKey(wf, "b");
  assert.deepEqual(again.needs, ["a"]);
  assert.equal(again.title, "-starts with a dash");
});

test("invalid inputs are each refused with a specific code and write nothing", async () => {
  const before = await allIssues(wf);
  const cases: [string, () => Promise<unknown>, string, number][] = [
    ["bad key", () => addJob(wf, input({ key: "Bad_Key" }), CATALOG), "invalid_job_key", 2],
    ["bad tier syntax", () => addJob(wf, input({ tier: "X9" }), CATALOG), "invalid_tier", 2],
    ["tier not supported by the model", () => addJob(wf, input({ tier: "O0" }), CATALOG), "tier_invalid", 2],
    ["bad review tier", () => addJob(wf, input({ reviewTier: "H5" }), CATALOG), "tier_invalid", 2],
    ["unknown needs", () => addJob(wf, input({ needs: ["ghost"] }), CATALOG), "needs_unknown", 2],
    ["self need", () => addJob(wf, input({ needs: ["x"] }), CATALOG), "dependency_cycle", 3],
    ["empty touches", () => addJob(wf, input({ spec: { ...spec(), touches: [] } }), CATALOG), "spec_invalid", 2],
    ["empty acceptance", () => addJob(wf, input({ spec: { ...spec(), acceptance: [] } }), CATALOG), "spec_invalid", 2],
    ["unknown spec field", () => addJob(wf, input({ spec: { ...spec(), extra: 1 } }), CATALOG), "spec_invalid", 2],
    ["duplicate key", () => addJob(wf, input({ key: "a" }), CATALOG), "job_key_exists", 3],
    ["duplicate needs", () => addJob(wf, input({ needs: ["a", "a"] }), CATALOG), "needs_duplicate", 2],
    ["empty title", () => addJob(wf, input({ title: "" }), CATALOG), "invalid_title", 2],
    ["edit: cycle", () => editJob(wf, "a", { needs: ["b"] }, CATALOG), "dependency_cycle", 3],
    ["edit: unknown needs", () => editJob(wf, "a", { needs: ["ghost"] }, CATALOG), "needs_unknown", 2],
    ["edit: bad tier", () => editJob(wf, "a", { tier: "Z1" }, CATALOG), "invalid_tier", 2],
    ["edit: empty touches", () => editJob(wf, "a", { spec: { ...spec(), touches: [] } }, CATALOG), "spec_invalid", 2],
    ["edit: nothing", () => editJob(wf, "a", {}, CATALOG), "nothing_to_edit", 2],
    ["edit: unknown job", () => editJob(wf, "ghost", { title: "t" }, CATALOG), "job_not_found", 4],
  ];
  for (const [name, p, code, exit] of cases) await assert.rejects(p(), refusal(code, exit), name);
  assert.deepEqual(await allIssues(wf), before, "bd list is identical after every refusal");
});

test("findCycle finds longer cycles and accepts DAGs", () => {
  assert.deepEqual(findCycle(new Map([["a", ["b"]], ["b", ["c"]], ["c", ["a"]]])), ["a", "b", "c", "a"]);
  assert.equal(findCycle(new Map([["a", ["b", "c"]], ["b", ["c"]], ["c", []]])), null);
});

test("parseSpec rejects non-objects and missing fields", () => {
  assert.throws(() => parseSpec(null), refusal("spec_invalid", 2));
  assert.throws(() => parseSpec({ goal: "g" }), refusal("spec_invalid", 2));
  assert.throws(() => parseSpec({ ...spec(), goal: "" }), refusal("spec_invalid", 2));
});

test("job edit on a queued Job is job_locked (exit 3) and writes nothing", async () => {
  await addDraft(wf, "q");
  await approvePlan(wf);
  await lockPlan(wf);
  assert.equal((await getJobByKey(wf, "q")).status, "queued");
  const before = await allIssues(wf);
  await assert.rejects(editJob(wf, "q", { title: "nope" }, CATALOG), refusal("job_locked", 3));
  await assert.rejects(editJob(wf, "a", { needs: [] }, CATALOG), refusal("job_locked", 3));
  assert.deepEqual(await allIssues(wf), before);
});

test("listJobs filters by status and validates it", async () => {
  assert.equal((await listJobs(wf, { status: "queued" })).length, 3);
  assert.equal((await listJobs(wf, { status: "draft" })).length, 0);
  await assert.rejects(listJobs(wf, { status: "bogus" }), refusal("invalid_status", 2));
});

test("job cancel without a running Attempt cancels at once, with the reason", async () => {
  const r = await cancelJob(wf, "q", "no longer needed");
  assert.equal(r.cancelled, true);
  assert.equal(r.job.status, "cancelled");
  assert.equal(r.job.metadata.cancel_reason, "no longer needed");
  assert.equal(typeof r.job.metadata.cancelled_at, "string");
  await assert.rejects(cancelJob(wf, "q", "again"), refusal("job_not_cancellable", 3));
  await assert.rejects(cancelJob(wf, "ghost", "r"), refusal("job_not_found", 4));
  await assert.rejects(cancelJob(wf, "a", " "), refusal("invalid_reason", 2));
});

test("a numeric-looking cancel reason stays a string", async () => {
  await addJob(wf, input({ key: "num" }), CATALOG);
  const r = await cancelJob(wf, "num", "123");
  assert.equal(r.job.metadata.cancel_reason, "123");
});

test("job cancel with a running Attempt records write-once cancel_requested and keeps the status", async () => {
  await setJobStatus(wf, "a", "running");
  await makeAttempt(wf, "a", 1, "in_progress");
  const first = await cancelJob(wf, "a", "stop it");
  assert.equal(first.cancelled, false);
  assert.equal(first.alreadyRequested, false);
  assert.equal(first.job.status, "running");
  const stamp = first.job.metadata.cancel_requested;
  assert.equal(typeof stamp, "string");
  const second = await cancelJob(wf, "a", "other reason");
  assert.equal(second.alreadyRequested, true);
  assert.equal(second.job.metadata.cancel_requested, stamp);
  assert.equal(second.job.metadata.cancel_reason, "stop it");
});

test("job retry is refused unless the Job is failed", async () => {
  const before = await allIssues(wf);
  for (const key of ["b", "a"]) await assert.rejects(retryJob(wf, key, "S4", CATALOG), refusal("job_not_failed", 3), key);
  await assert.rejects(retryJob(wf, "num", "S4", CATALOG), refusal("job_not_failed", 3));
  assert.deepEqual(await allIssues(wf), before);
});

test("job retry on a failed Job records retry_<n>_tier for the next Attempt and queues it", async () => {
  await setJobStatus(wf, "b", "failed");
  await makeAttempt(wf, "b", 1, "closed");
  await makeAttempt(wf, "b", 2, "closed");
  await assert.rejects(retryJob(wf, "b", "O0", CATALOG), refusal("tier_invalid", 2));
  assert.equal((await getJobByKey(wf, "b")).status, "failed");
  const r = await retryJob(wf, "b", "O3", CATALOG);
  assert.equal(r.attempt, 3);
  assert.equal(r.job.status, "queued");
  assert.equal(r.job.metadata.retry_3_tier, "O3");
  // a second failure and retry gets its own write-once key
  await setJobStatus(wf, "b", "failed");
  await makeAttempt(wf, "b", 3, "closed");
  const r2 = await retryJob(wf, "b", "O4", CATALOG);
  assert.equal(r2.job.metadata.retry_3_tier, "O3");
  assert.equal(r2.job.metadata.retry_4_tier, "O4");
});

test("showJob returns the Job, its Attempts, comments and Escalations", async () => {
  const bid = await jobId(wf, "b");
  const attempts: any[] = await bdJson(wf, ["list", "--all", "-n", "0", "-t", "attempt", "--parent", bid]);
  const a1 = attempts.find((a) => a.metadata.number === 1);
  await bdJson(wf, ["comments", "add", a1.id, "REJECTED: tests fail"]);
  await bdJson(wf, ["create", "--title=Escalation: why", "-t", "escalation", "--parent", a1.id, "--description=which db?"]);
  const s = await showJob(wf, "b");
  assert.equal(s.job.key, "b");
  assert.deepEqual(s.attempts.map((a) => [a.id, a.tier]), [["b-a1", "S3"], ["b-a2", "S3"], ["b-a3", "S3"]]);
  assert.deepEqual(s.attempts[0]!.comments.map((c) => [c.author, c.text]), [["cpd", "REJECTED: tests fail"]]);
  assert.deepEqual(s.attempts[0]!.escalations.map((e) => [e.status, e.question]), [["open", "which db?"]]);
  await assert.rejects(showJob(wf, "ghost"), refusal("job_not_found", 4));
});
