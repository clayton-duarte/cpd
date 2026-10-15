import { before, test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { CpdError } from "../../src/lib/errors.ts";
import {
  attemptCount, failAttempt, getAttempt, listAttempts, markMerged, markSubmitted, recordAttemptFacts,
  recordVerdict, runningAttempts, startAttempt,
} from "../../src/ledger/attempts.ts";
import { getJobByKey, readyJobs } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, allIssues, approvePlan, bdJson, newPlan, sandbox } from "./helpers.ts";

const { repo } = sandbox();

const refusal = (code: string, exit = 3) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;
const sha = (c: string) => c.repeat(40);

/** A locked Workflow whose Jobs are all `queued`. */
async function locked(wf: string, jobs: [key: string, needs?: string[]][]): Promise<void> {
  await newPlan(wf, repo);
  for (const [key, needs] of jobs) await addDraft(wf, key, needs ?? [], "S3");
  await approvePlan(wf);
  await lockPlan(wf);
}

const issuesOf = async (wf: string, type: string, parent?: string): Promise<any[]> =>
  bdJson(wf, ["list", "--all", "-n", "0", "-t", type, ...(parent ? ["--parent", parent] : [])]);
const comments = async (wf: string, id: string): Promise<string[]> => (await bdJson(wf, ["comments", id])).map((c: any) => c.text);
const provenance = async (wf: string, id: string): Promise<any[]> => bdJson(wf, ["provenance", "log", id]);

test("1. full lifecycle: start, submit, reject (requeue), start 2, submit, approve, merged", async () => {
  const wf = "life";
  await locked(wf, [["api"]]);
  const job = await getJobByKey(wf, "api");
  assert.equal(await attemptCount(wf, "api"), 0);

  // Attempt 1
  const a1 = await startAttempt(wf, "api", { tier: "S3" });
  assert.ok(a1);
  assert.deepEqual([a1.id, a1.number, a1.tier, a1.status, a1.job], ["api-a1", 1, "S3", "in_progress", "api"]);
  assert.equal((await getJobByKey(wf, "api")).status, "running");
  await recordAttemptFacts(wf, "api-a1", { worktree: "/w/a1", branch: "cpd/life--api-a1", agent: "c1", session_path: "/s/1.jsonl", argv: ["pi", "--x"] });
  assert.deepEqual((await runningAttempts(wf)).map((a) => a.id), ["api-a1"]);
  await markSubmitted(wf, "api-a1", sha("a"));
  assert.equal((await getJobByKey(wf, "api")).status, "in_review");
  assert.deepEqual((await runningAttempts(wf)).map((a) => a.id), []);
  await recordVerdict(wf, "api-a1", { verdict: "reject", feedback: "tests fail in api.spec" });
  await failAttempt(wf, "api-a1", "review_rejected", { requeue: true });
  assert.equal((await getJobByKey(wf, "api")).status, "queued");

  // Attempt 2 at the next Tier
  const a2 = await startAttempt(wf, "api", { tier: "S4", worktree: "/w/a2", branch: "cpd/life--api-a2" });
  assert.deepEqual([a2?.id, a2?.number, a2?.tier, a2?.worktree], ["api-a2", 2, "S4", "/w/a2"]);
  await markSubmitted(wf, "api-a2", sha("b"));
  await recordVerdict(wf, "api-a2", { verdict: "approve", feedback: "all acceptance items pass" });
  assert.equal((await getAttempt(wf, "api-a2")).outcome, "approved");
  assert.equal((await getAttempt(wf, "api-a2")).status, "in_review");
  const merged = await markMerged(wf, "api-a2", sha("c"));
  assert.deepEqual([merged.status, merged.outcome, merged.closeReason], ["closed", "approved", "approved"]);

  // Final Ledger state (t2 F4)
  const children: any[] = (await issuesOf(wf, "attempt", job.id)).sort((x, y) => x.metadata.number - y.metadata.number);
  assert.deepEqual(
    children.map((c) => [c.metadata.number, c.metadata.tier, c.status, c.metadata.outcome, c.metadata.cause ?? null, c.close_reason]),
    [[1, "S3", "closed", "rejected", "review_rejected", "rejected"], [2, "S4", "closed", "approved", null, "approved"]],
  );
  for (const c of children) {
    assert.match(c.metadata.started_at, /^\d{4}-/);
    assert.match(c.metadata.submitted_at, /^\d{4}-/);
    assert.match(c.metadata.ended_at, /^\d{4}-/);
  }
  assert.deepEqual(children[0].metadata.argv, ["pi", "--x"]);
  assert.deepEqual(await comments(wf, children[0].id), ["REJECTED: tests fail in api.spec"]);
  assert.deepEqual(await comments(wf, children[1].id), ["APPROVED: all acceptance items pass"]);
  const kinds = async (c: any) => (await provenance(wf, c.id)).map((p) => [p.kind, p.ref]).sort();
  assert.deepEqual(await kinds(children[0]), [["commit", sha("a")], ["cut", "cpd/life--api-a1"]]);
  assert.deepEqual(await kinds(children[1]), [["commit", sha("b")], ["cut", "cpd/life--api-a2"], ["land", sha("c")]]);

  const [j]: any[] = await bdJson(wf, ["show", job.id]);
  assert.deepEqual([j.status, j.close_reason, j.metadata.merged_sha], ["closed", "merged", sha("c")]);
  assert.deepEqual((await listAttempts(wf)).map((a) => a.id), ["api-a1", "api-a2"]);
  assert.deepEqual((await listAttempts(wf, { status: "closed" })).length, 2);
  assert.deepEqual((await listAttempts(wf, { status: "in_review" })).length, 0);
  await assert.rejects(listAttempts(wf, { status: "bogus" }), refusal("invalid_status", 2));
  assert.equal(await attemptCount(wf, "api"), 2);
});

test("2. two concurrent startAttempt calls on one queued Job: exactly one gets an Attempt", async () => {
  const wf = "race";
  await locked(wf, [["solo"]]);
  const script = `
    import { startAttempt } from ${JSON.stringify(new URL("../../src/ledger/attempts.ts", import.meta.url).href)};
    const a = await startAttempt(${JSON.stringify(wf)}, "solo", { tier: "S3" });
    process.stdout.write(JSON.stringify(a ? a.id : null));`;
  const child = () =>
    new Promise<string>((resolve, reject) => {
      const c = spawn(process.execPath, ["--input-type=module", "-e", script], { env: process.env, stdio: ["ignore", "pipe", "pipe"] });
      let out = "";
      let err = "";
      c.stdout.on("data", (d) => (out += d));
      c.stderr.on("data", (d) => (err += d));
      c.on("close", (code) => (code === 0 ? resolve(out) : reject(new Error(`exit ${code}: ${err}`))));
    });
  const results = (await Promise.all([child(), child()])).map((r) => JSON.parse(r) as string | null);
  assert.deepEqual(results.filter((r) => r !== null), ["solo-a1"]);
  assert.equal(results.length, 2);
  assert.equal(await attemptCount(wf, "solo"), 1);
  assert.equal((await getJobByKey(wf, "solo")).status, "running");
});

test("startAttempt: returns null for a Job that is not queued, refuses a blocked Job, verifies the fingerprint", async () => {
  const wf = "start";
  await locked(wf, [["a"], ["b", ["a"]], ["c"]]);
  await assert.rejects(startAttempt(wf, "b", { tier: "S3" }), refusal("job_not_ready"));
  assert.equal(await attemptCount(wf, "b"), 0);
  assert.ok(await startAttempt(wf, "a", { tier: "S3" }));
  assert.equal(await startAttempt(wf, "a", { tier: "S3" }), null, "already running");
  await assert.rejects(startAttempt(wf, "a", { tier: "S9" }), refusal("invalid_tier", 2));
  await assert.rejects(startAttempt(wf, "ghost", { tier: "S3" }), refusal("job_not_found", 4));

  const [c] = (await issuesOf(wf, "job")).filter((j) => j.metadata.key === "c");
  await bdJson(wf, ["update", c.id, "--set-metadata", "tier=O5"], "intruder");
  await assert.rejects(startAttempt(wf, "c", { tier: "S3" }), refusal("locked_job_tampered"));
  assert.equal(await attemptCount(wf, "c"), 0);
  assert.equal((await getJobByKey(wf, "c")).status, "queued");
});

test("startAttempt refuses a queued Job that still has an open Attempt", async () => {
  const wf = "open";
  await locked(wf, [["a"]]);
  assert.ok(await startAttempt(wf, "a", { tier: "S3" }));
  const [j] = await issuesOf(wf, "job");
  await bdJson(wf, ["update", j.id, "-s", "queued"]); // simulate a corrupted Ledger
  await assert.rejects(startAttempt(wf, "a", { tier: "S3" }), refusal("attempt_open"));
  assert.equal(await attemptCount(wf, "a"), 1);
});

test("3. write-once: a second fact with the same key fails; facts with a dedicated writer are refused", async () => {
  const wf = "facts";
  await locked(wf, [["a"]]);
  await startAttempt(wf, "a", { tier: "S3" });
  await recordAttemptFacts(wf, "a-a1", { worktree: "/w", agent: "c1" });
  const before = await allIssues(wf);
  await assert.rejects(recordAttemptFacts(wf, "a-a1", { agent: "c2", branch: "b" }), refusal("metadata_write_once"));
  await assert.rejects(recordAttemptFacts(wf, "a-a1", { started_at: "x" }), refusal("metadata_write_once"));
  assert.deepEqual(await allIssues(wf), before, "a refused write changes nothing");
  assert.equal((await getAttempt(wf, "a-a1")).agent, "c1");
  for (const key of ["outcome", "number", "metrics_turns", "Bad-Key"]) {
    await assert.rejects(recordAttemptFacts(wf, "a-a1", { [key]: "x" }), refusal("invalid_facts", 2), key);
  }
  await assert.rejects(recordAttemptFacts(wf, "a-a1", {}), refusal("invalid_facts", 2));
  await assert.rejects(getAttempt(wf, "a-a9"), refusal("attempt_not_found", 4));
  await assert.rejects(getAttempt(wf, "nope-a1"), refusal("attempt_not_found", 4));
});

test("verdict, submit and merge preconditions are checked before anything is written", async () => {
  const wf = "pre";
  await locked(wf, [["a"]]);
  await startAttempt(wf, "a", { tier: "S3" });
  await assert.rejects(markSubmitted(wf, "a-a1", "abc"), refusal("invalid_sha", 2));
  await assert.rejects(recordVerdict(wf, "a-a1", { verdict: "approve", feedback: "x" }), refusal("illegal_transition"));
  await markSubmitted(wf, "a-a1", sha("1"));
  await assert.rejects(markSubmitted(wf, "a-a1", sha("1")), refusal("illegal_transition"));
  await assert.rejects(recordVerdict(wf, "a-a1", { verdict: "approve", feedback: "" }), refusal("invalid_verdict", 2));

  // merge without an approved verdict
  const before = await allIssues(wf);
  await assert.rejects(markMerged(wf, "a-a1", sha("2")), refusal("attempt_not_approved"));
  assert.deepEqual(await allIssues(wf), before);

  await recordVerdict(wf, "a-a1", { verdict: "approve", feedback: "ok" });
  await assert.rejects(recordVerdict(wf, "a-a1", { verdict: "reject", feedback: "changed my mind" }), refusal("metadata_write_once"));
  assert.deepEqual(await comments(wf, (await issuesOf(wf, "attempt"))[0].id), ["APPROVED: ok"]);

  // approved, then the merge fails: the one allowed outcome rewrite
  const failed = await failAttempt(wf, "a-a1", "merge_conflict", { requeue: true });
  assert.deepEqual([failed.outcome, failed.cause, failed.status], ["failed", "merge_conflict", "closed"]);
  assert.equal((await getJobByKey(wf, "a")).status, "queued");
  await assert.rejects(failAttempt(wf, "a-a1", "crashed", { requeue: true }), refusal("illegal_transition"));
});

test("markMerged without a review (reviewed: false): outcome merged, no verdict comment, a verdict refuses it; the Job still closes as merged", async () => {
  const wf = "noreview";
  await locked(wf, [["a"], ["b"]]);
  await startAttempt(wf, "a", { tier: "S3" });
  await markSubmitted(wf, "a-a1", sha("1"));
  const merged = await markMerged(wf, "a-a1", sha("2"), { reviewed: false });
  assert.deepEqual([merged.status, merged.outcome, merged.closeReason], ["closed", "merged", "merged"]);
  const [attempt] = await issuesOf(wf, "attempt");
  assert.deepEqual(await comments(wf, attempt.id), [], "no APPROVED: comment, so nothing fakes a Reviewer verdict");
  assert.deepEqual((await provenance(wf, attempt.id)).map((p) => p.kind), ["commit", "land"]);
  const job = await getJobByKey(wf, "a");
  assert.deepEqual([job.status, job.closeReason, job.metadata.merged_sha], ["closed", "merged", sha("2")]);

  // an Attempt that has a verdict was reviewed: merging it as unreviewed is refused and nothing is written
  await startAttempt(wf, "b", { tier: "S3" });
  await markSubmitted(wf, "b-a1", sha("3"));
  await recordVerdict(wf, "b-a1", { verdict: "approve", feedback: "ok" });
  const before = await allIssues(wf);
  await assert.rejects(markMerged(wf, "b-a1", sha("4"), { reviewed: false }), refusal("attempt_has_verdict"));
  assert.deepEqual(await allIssues(wf), before);
});

test("4. markMerged when the Job is not in_review: illegal_transition and nothing is written", async () => {
  const wf = "notrev";
  await locked(wf, [["a"]]);
  await startAttempt(wf, "a", { tier: "S3" });
  const before = await allIssues(wf);
  await assert.rejects(markMerged(wf, "a-a1", sha("a")), refusal("illegal_transition"));
  assert.deepEqual(await allIssues(wf), before);
  assert.equal((await getJobByKey(wf, "a")).status, "running");
  assert.equal((await getJobByKey(wf, "a")).metadata.merged_sha, undefined);

  // Job moved out from under the Attempt: Attempt in_review, Job not
  await markSubmitted(wf, "a-a1", sha("a"));
  await recordVerdict(wf, "a-a1", { verdict: "approve", feedback: "ok" });
  const [j] = await issuesOf(wf, "job");
  await bdJson(wf, ["update", j.id, "-s", "running"]);
  const before2 = await allIssues(wf);
  await assert.rejects(markMerged(wf, "a-a1", sha("a")), refusal("illegal_transition"));
  assert.deepEqual(await allIssues(wf), before2);
  assert.deepEqual((await provenance(wf, (await issuesOf(wf, "attempt"))[0].id)).map((p) => p.kind), ["commit"]);
});

test("6. failAttempt with requeue false fails the Job and its dependents stay blocked", async () => {
  const wf = "fail";
  await locked(wf, [["a"], ["b", ["a"]], ["c"]]);
  assert.deepEqual((await readyJobs(wf)).map((j) => j.key), ["a", "c"]);
  await startAttempt(wf, "a", { tier: "S3" });
  const closed = await failAttempt(wf, "a-a1", "no_submit", { requeue: false });
  assert.deepEqual([closed.status, closed.outcome, closed.cause, closed.closeReason], ["closed", "failed", "no_submit", "failed"]);
  assert.match(closed.endedAt ?? "", /^\d{4}-/);
  assert.equal((await getJobByKey(wf, "a")).status, "failed");
  assert.deepEqual((await readyJobs(wf)).map((j) => j.key), ["c"], "b stays blocked behind the failed a");
  await assert.rejects(startAttempt(wf, "b", { tier: "S3" }), refusal("job_not_ready"));

  // timed out -> requeue
  await startAttempt(wf, "c", { tier: "S3" });
  const t = await failAttempt(wf, "c-a1", "timed_out", { requeue: true });
  assert.deepEqual([t.outcome, t.cause], ["timed_out", "timed_out"]);
  assert.equal((await getJobByKey(wf, "c")).status, "queued");
});

test("failAttempt with cause cancelled cancels the Job; a cancelled Attempt cannot be requeued", async () => {
  const wf = "cancel";
  await locked(wf, [["a"], ["b", ["a"]]]);
  await startAttempt(wf, "a", { tier: "S3" });
  await assert.rejects(failAttempt(wf, "a-a1", "cancelled", { requeue: true }), refusal("invalid_requeue", 2));
  await assert.rejects(failAttempt(wf, "a-a1", "Not Snake", { requeue: false }), refusal("invalid_cause", 2));
  const a = await failAttempt(wf, "a-a1", "cancelled", { requeue: false });
  assert.deepEqual([a.outcome, a.cause], ["cancelled", "cancelled"]);
  const job = await getJobByKey(wf, "a");
  assert.equal(job.status, "cancelled");
  assert.match(String(job.metadata.cancelled_at), /^\d{4}-/);
  assert.deepEqual((await readyJobs(wf)).map((j) => j.key), [], "dependents stay blocked behind a cancelled Job");
});
