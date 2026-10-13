import { test } from "node:test";
import assert from "node:assert/strict";
import { cancelJob, editJob, getJobByKey, listJobs } from "../../src/ledger/jobs.ts";
import { latestApprovedFingerprint, lockPlan, planFingerprint, recordPlanDecision } from "../../src/ledger/plan.ts";
import { getPhase } from "../../src/ledger/workflow.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { addDraft, allIssues, CATALOG, newPlan, sandbox, spec } from "./helpers.ts";

const { repo } = sandbox();
const wf = "plan";

const refusal = (code: string, exit: number) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;

test("lockPlan without an approved decision is plan_not_approved and writes nothing", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await addDraft(wf, "b", ["a"]);
  const before = await allIssues(wf);
  await assert.rejects(lockPlan(wf), refusal("plan_not_approved", 3), "no decision");
  await recordPlanDecision(wf, "r1", "pending");
  await assert.rejects(lockPlan(wf), refusal("plan_not_approved", 3), "only pending");
  await recordPlanDecision(wf, "r1", "rejected", "needs work");
  await assert.rejects(lockPlan(wf), refusal("plan_not_approved", 3), "rejected");
  assert.equal(await latestApprovedFingerprint(wf), null);
  assert.deepEqual((await allIssues(wf)).map((i: any) => [i.id, i.status]), before.map((i: any) => [i.id, i.status]));
  assert.equal(await getPhase(wf), "plan");
});

test("decisions are write-once per state and keep the pending fingerprint", async () => {
  const fp = await planFingerprint(wf);
  const pending = await recordPlanDecision(wf, "r2", "pending");
  assert.equal(pending.fingerprint, fp);
  await assert.rejects(recordPlanDecision(wf, "r2", "pending"), refusal("decision_recorded", 3));
  // the plan changes after the reviewer was shown it: an approval is for what was shown
  await editJob(wf, "a", { title: "changed after pending" }, CATALOG);
  const approved = await recordPlanDecision(wf, "r2", "approved", "lgtm");
  assert.equal(approved.fingerprint, fp);
  assert.equal(approved.approved, true);
  assert.equal(approved.feedback, "lgtm");
  await assert.rejects(recordPlanDecision(wf, "r2", "approved"), refusal("decision_recorded", 3));
  await assert.rejects(recordPlanDecision(wf, "r2", "rejected"), refusal("decision_recorded", 3));
  assert.equal(await latestApprovedFingerprint(wf), fp);
  await assert.rejects(lockPlan(wf), refusal("plan_not_approved", 3), "plan differs from what was approved");
});

test("an approval is invalidated by any later draft change; re-approval for the current plan locks", async () => {
  await recordPlanDecision(wf, "r3", "pending");
  await recordPlanDecision(wf, "r3", "approved");
  const approvedFp = await latestApprovedFingerprint(wf);
  assert.equal(approvedFp, await planFingerprint(wf));

  await editJob(wf, "b", { spec: spec({ goal: "edited after approval" }) }, CATALOG);
  assert.notEqual(await planFingerprint(wf), approvedFp);
  await assert.rejects(lockPlan(wf), refusal("plan_not_approved", 3));
  assert.deepEqual((await listJobs(wf)).map((j) => j.status), ["draft", "draft"]);

  await recordPlanDecision(wf, "r4", "pending");
  await recordPlanDecision(wf, "r4", "approved");
  const r = await lockPlan(wf);
  assert.deepEqual(r.jobs, ["a", "b"]);
  assert.equal(r.fingerprint, await latestApprovedFingerprint(wf));
  assert.deepEqual((await listJobs(wf)).map((j) => j.status), ["queued", "queued"]);
  assert.equal(await getPhase(wf), "dispatch");
  const wfIssue: any = (await allIssues(wf)).find((i: any) => i.issue_type === "workflow");
  assert.equal(typeof wfIssue.metadata.dispatch_started_at, "string");
  assert.equal(wfIssue.metadata.plan_review_r4.status, "approved", "nested decision objects survive later metadata writes");
});

test("after lock: decisions and a second lock are refused, drafts cannot be added", async () => {
  await assert.rejects(lockPlan(wf), refusal("illegal_phase", 3));
  await assert.rejects(recordPlanDecision(wf, "r5", "pending"), refusal("illegal_phase", 3));
});

test("the plan fingerprint covers draft Jobs only", async () => {
  const wf2 = "plan2";
  await newPlan(wf2, repo);
  await addDraft(wf2, "b");
  await addDraft(wf2, "a");
  const fp = await planFingerprint(wf2);
  await addDraft(wf2, "c");
  assert.notEqual(await planFingerprint(wf2), fp);
  await cancelJob(wf2, "c", "no");
  assert.equal(await planFingerprint(wf2), fp, "a cancelled draft is not part of the plan");
  assert.equal((await getJobByKey(wf2, "c")).status, "cancelled");
});

test("lockPlan refuses an empty plan, a bad review id, and a draft that needs a cancelled Job", async () => {
  const wf3 = "plan3";
  await newPlan(wf3, repo);
  await assert.rejects(lockPlan(wf3), refusal("no_jobs", 3));
  await assert.rejects(recordPlanDecision(wf3, "bad id!", "pending"), refusal("invalid_review_id", 2));
  await addDraft(wf3, "a");
  await addDraft(wf3, "b", ["a"]);
  await cancelJob(wf3, "a", "dropped");
  await recordPlanDecision(wf3, "r1", "pending");
  await recordPlanDecision(wf3, "r1", "approved");
  await assert.rejects(lockPlan(wf3), refusal("needs_unavailable", 3));
  assert.equal((await getJobByKey(wf3, "b")).status, "draft");
  assert.equal(await getPhase(wf3), "plan");
});
