import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CRASH_SETTLE_MS, decideReview, findSessionById, sessionTail, runAttempt, observeWorker, type RunResult } from "../../src/dispatch/attempt-run.ts";
import type { MergeOutcome } from "../../src/git/merge.ts";
import { CpdError, failure } from "../../src/lib/errors.ts";
import { reviewBriefPath, reviewDiffPath, reviewGatePath } from "../../src/lib/paths.ts";
import { tmpDir } from "../helpers/index.ts";
import { LEAD_SESSION, SHA_POST, SHA_PRE, TMUX_REVIEWER_PANE, TMUX_SOCKET, makeWorld, reviewerName, verdictText, workerName, type Script, type World } from "./fakes.ts";

process.env.CPD_HOME = tmpDir("cpd-attempt-run-");

const outcome = (result: MergeOutcome["result"], extra: Partial<MergeOutcome> = {}): MergeOutcome => ({ result, preSha: SHA_PRE, detail: `fake ${result}`, ...extra });
const merged = outcome("merged", { postSha: SHA_POST });

/** Run the lifecycle on a fake world and return both. */
async function go(script: Script = {}): Promise<{ w: World; r: RunResult }> {
  const w = makeWorld(script);
  return { w, r: await runAttempt(w.deps) };
}

const kinds = (w: World) => w.messages.map((m) => m.kind);

// --- the ten required paths ---

test("happy path: Worker submits, Reviewer approves, merge → Merged + job-merged", async () => {
  const { w, r } = await go();
  assert.deepEqual(r, { state: "merged", sha: SHA_POST });
  assert.deepEqual(w.ledger, [
    "verifyJob",
    "recordFacts worktree",
    "recordFacts branch,agent,session_path,argv,worker_pane,workspace",
    "recordMetrics metrics",
    "recordMetrics review_metrics",
    "recordVerdict approve",
    `markMerged ${SHA_POST}`,
  ]);
  assert.deepEqual(kinds(w), ["job-merged"]);
  assert.equal(w.messages[0]!.job, "api");
  assert.equal(w.messages[0]!.attempt, "api-a1");
  assert.match(w.messages[0]!.text, /cpd workflow show --wf demo/);
  assert.deepEqual(w.closedPanes, ["pane-r"], "the Reviewer pane is closed after the verdict");
  assert.deepEqual(w.closedWorkspaces, ["ws1"], "the Attempt workspace is closed on merge");
  assert.deepEqual(w.removed, ["cpd/demo--api-a1"]);
  assert.equal(w.slotReleases, 1);
  assert.deepEqual([w.state.status, w.state.outcome, w.state.jobStatus], ["closed", "approved", "closed"]);
  // The diff for the Reviewer was written under reviews/, the Reviewer was prompted with the brief.
  assert.equal(readFileSync(reviewDiffPath("demo", "api-a1"), "utf8"), "diff --git a/x b/x\n+hello\n");
  assert.ok(w.prompts.some((p) => p.name === reviewerName() && p.text.includes("/reviews/api-a1.md")));
});

test("happy path: events happen in the §12 order", async () => {
  const { w } = await go();
  const order = [
    "git.createWorktree cpd/demo--api-a1",
    "ledger.recordFacts worktree",
    "render.workerSpec",
    "herdr.openWorktree demo api a1",
    "herdr.startAgent",
    "herdr.prompt worker",
    "ledger.recordMetrics metrics",
    "git.diff",
    "render.reviewBrief",
    "herdr.split",
    "herdr.prompt reviewer",
    "ledger.recordVerdict approve",
    "git.merge",
    "ledger.markMerged",
    "herdr.closeWorkspace ws1",
    "git.removeWorktree",
    "queue.job-merged",
    "slot.release",
  ];
  let from = 0;
  for (const want of order) {
    const at = w.events.findIndex((e, i) => i >= from && e.startsWith(want));
    assert.ok(at >= from, `missing or out of order: ${want}\n${w.events.join("\n")}`);
    from = at + 1;
  }
});

test("the Worker starts with --approve at the Attempt Tier (Job S3 → sonnet:high)", async () => {
  const { w } = await go({ jobTier: "S3" });
  const start = w.agentStarts.find((a) => a.name === workerName())!;
  assert.deepEqual(start.args, ["--approve", "--no-skills", "--exclude-tools", "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox", "--model", "p/sonnet:high"]);
});

test("reviewer reject → ladder → attempt-retry with the next Tier", async () => {
  const { w, r } = await go({ reviewer: [verdictText("reject", "tests are missing")] });
  assert.deepEqual(r, { state: "retry_queued", cause: "review_rejected", nextAttempt: "api-a2", nextTier: "S4" });
  assert.deepEqual(w.ledger.slice(-2), ["recordVerdict reject", "fail review_rejected requeue=true"]);
  assert.deepEqual(kinds(w), ["attempt-retry"]);
  assert.equal(w.messages[0]!.data?.nextTier, "S4");
  assert.match(w.messages[0]!.text, /tests are missing/);
  assert.match(w.messages[0]!.text, /Tier S4/);
  assert.deepEqual(w.closedPanes.sort(), ["pane-r", "pane-w"], "Reviewer and Worker panes closed");
  assert.deepEqual(w.closedWorkspaces, [], "the worktree is kept");
  assert.deepEqual(w.removed, []);
  assert.equal(w.slotReleases, 1);
  assert.deepEqual([w.state.status, w.state.outcome, w.state.jobStatus], ["closed", "rejected", "queued"]);
});

test("reject at the last allowed Attempt (1 + retryCap) → job-failed, no ladder", async () => {
  const { w, r } = await go({ attemptNumber: 3, reviewer: [verdictText("reject", "still wrong")] });
  assert.deepEqual(r, { state: "job_failed", cause: "review_rejected" });
  assert.equal(w.ledger.at(-1), "fail review_rejected requeue=false");
  assert.deepEqual(kinds(w), ["job-failed"]);
  assert.match(w.messages[0]!.text, /retry cap is reached/);
  assert.match(w.messages[0]!.text, /cpd job retry api --tier <T> --wf demo/);
  assert.deepEqual(w.closedPanes.sort(), ["pane-r", "pane-w"]);
  assert.equal(w.slotReleases, 1);
  assert.deepEqual([w.state.outcome, w.state.jobStatus], ["rejected", "failed"]);
});

test("reject when the ladder has no next Tier (O5) → job-failed even below the cap", async () => {
  const { w, r } = await go({ jobTier: "O5", reviewer: [verdictText("reject", "no")] });
  assert.deepEqual(r, { state: "job_failed", cause: "review_rejected" });
  assert.match(w.messages[0]!.text, /No higher Tier than/);
  assert.deepEqual(kinds(w), ["job-failed"]);
});

test("the retry cap counts only the current automatic run: after `job retry` at Attempt 4, Attempt 4 may still climb", async () => {
  const { r, w } = await go({
    attemptNumber: 4,
    jobMetadata: { retry_4_tier: "S1" },
    reviewer: [verdictText("reject", "no")],
  });
  assert.deepEqual(r, { state: "retry_queued", cause: "review_rejected", nextAttempt: "api-a5", nextTier: "S2" });
  assert.deepEqual(kinds(w), ["attempt-retry"]);
});

test("timed_out → job-failed, no ladder, Worker pane closed, metrics still collected", async () => {
  const { w, r } = await go({ worker: ["timeout"] });
  assert.deepEqual(r, { state: "job_failed", cause: "timed_out" });
  assert.equal(w.ledger.at(-1), "fail timed_out requeue=false");
  assert.ok(w.ledger.includes("recordMetrics metrics"));
  assert.deepEqual(kinds(w), ["job-failed"]);
  assert.deepEqual(w.closedPanes, ["pane-w"]);
  assert.equal(w.slotReleases, 1);
  assert.deepEqual([w.state.outcome, w.state.cause, w.state.jobStatus], ["timed_out", "timed_out", "failed"]);
  assert.equal(w.agentStarts.length, 1, "no Reviewer");
});

test("crash (agent gone) → ladder → attempt-retry", async () => {
  const { w, r } = await go({ worker: ["crash"] });
  assert.deepEqual(r, { state: "retry_queued", cause: "crashed", nextAttempt: "api-a2", nextTier: "S4" });
  assert.equal(w.ledger.at(-1), "fail crashed requeue=true");
  assert.deepEqual(kinds(w), ["attempt-retry"]);
  assert.deepEqual(w.closedPanes, ["pane-w"]);
  assert.equal(w.slotReleases, 1);
  assert.deepEqual([w.state.outcome, w.state.jobStatus], ["failed", "queued"]);
});

test("crash with the agent still listed but the last stopReason not `stop` → crashed", async () => {
  const { r, w } = await go({ worker: ["tool_use"] });
  assert.equal(r.state, "retry_queued");
  assert.equal(w.ledger.at(-1), "fail crashed requeue=true");
  assert.equal(w.prompts.length, 1, "a crashed Worker is not nudged");
});

test("a vanished Worker is looked at twice: watch closes the pane first and records timed_out a moment later → timed_out, not crashed", async () => {
  const { w, r } = await go({ worker: ["crash"], onSleep: (w) => void (w.state.metadata.timed_out = true) });
  assert.deepEqual(r, { state: "job_failed", cause: "timed_out" });
  assert.ok(w.events.includes(`clock.sleep ${CRASH_SETTLE_MS}`));
  assert.equal(w.ledger.at(-1), "fail timed_out requeue=false");
});

test("the same for a cancel: the Attempt closed by watch during the settle is left alone", async () => {
  const { w, r } = await go({
    worker: ["crash"],
    onSleep: (w) => {
      w.state.status = "closed";
      w.state.jobStatus = "cancelled";
      w.state.outcome = "cancelled";
    },
  });
  assert.deepEqual(r, { state: "closed_externally", outcome: "cancelled" });
  assert.equal(w.ledger.some((l) => l.startsWith("fail")), false);
  assert.deepEqual(w.messages, []);
});

test("a Worker that is still gone after the settle is crashed (the wait happens once)", async () => {
  const { w, r } = await go({ worker: ["crash"] });
  assert.equal(r.state, "retry_queued");
  assert.equal(w.events.filter((e) => e.startsWith("clock.sleep")).length, 1);
  assert.deepEqual(w.events.filter((e) => e.startsWith("clock.sleep")), [`clock.sleep ${CRASH_SETTLE_MS}`]);
});

test("no submit after the nudge → ladder (one nudge, naming the submit command)", async () => {
  const { w, r } = await go({ worker: ["idle", "idle"] });
  assert.deepEqual(r, { state: "retry_queued", cause: "no_submit", nextAttempt: "api-a2", nextTier: "S4" });
  const worker = w.prompts.filter((p) => p.name === workerName());
  assert.equal(worker.length, 2);
  assert.match(worker[0]!.text, /Read \/specs\/api-a1\.md and follow it exactly\. Your attempt id is api-a1\./);
  assert.match(worker[1]!.text, /without running `cpd attempt submit api-a1/);
  assert.match(worker[1]!.text, /cpd ask api-a1 "<question>"/);
  assert.equal(w.ledger.at(-1), "fail no_submit requeue=true");
  assert.deepEqual(kinds(w), ["attempt-retry"]);
  assert.deepEqual(w.closedPanes, ["pane-w"]);
  assert.equal(w.slotReleases, 1);
});

test("the nudge works: idle first, submits after the nudge → reviewed and merged", async () => {
  const { r, w } = await go({ worker: ["idle", "submit"] });
  assert.equal(r.state, "merged");
  assert.equal(w.prompts.filter((p) => p.name === workerName()).length, 2);
});

test("setup_failed → job-failed, no ladder, nothing started in herdr", async () => {
  const { w, r } = await go({ setupFails: true });
  assert.deepEqual(r, { state: "job_failed", cause: "setup_failed" });
  assert.deepEqual(w.ledger, ["verifyJob", "fail setup_failed requeue=false"]);
  assert.deepEqual(kinds(w), ["job-failed"]);
  assert.equal(w.agentStarts.length, 0);
  assert.equal(w.events.some((e) => e.startsWith("herdr.")), false);
  assert.equal(w.slotReleases, 1);
  assert.deepEqual([w.state.outcome, w.state.jobStatus], ["failed", "failed"]);
});

test("verdict invalid twice → review-needed (Attempt waits, slot released); then review decide --approve → merge", async () => {
  const w = makeWorld({ reviewer: ["I think it is fine.", "Still no block, sorry."] });
  const r = await runAttempt(w.deps);
  assert.deepEqual(r, { state: "review_needed" });
  assert.deepEqual(kinds(w), ["review-needed"]);
  assert.match(w.messages[0]!.text, /cpd review decide api-a1 --approve --feedback "<text>" --wf demo/);
  assert.match(w.messages[0]!.text, /--reject/);
  assert.equal(w.prompts.filter((p) => p.name === reviewerName()).length, 2, "the Reviewer is re-prompted exactly once");
  assert.deepEqual(w.closedPanes, ["pane-r"]);
  assert.equal(w.slotReleases, 1, "the slot is not held while the Lead decides");
  assert.deepEqual([w.state.status, w.state.outcome, w.state.jobStatus], ["in_review", null, "in_review"]);
  assert.ok(w.ledger.includes("recordFacts review_needed_at"));
  assert.equal(w.mergeCalls, 0);

  const d = await decideReview({ ...w.deps, slot: null }, { verdict: "approve", feedback: "Lead read it: fine" });
  assert.deepEqual(d, { state: "merged", sha: SHA_POST });
  assert.deepEqual(w.ledger.slice(-2), ["recordVerdict approve", `markMerged ${SHA_POST}`]);
  assert.deepEqual(kinds(w), ["review-needed", "job-merged"]);
  assert.deepEqual(w.closedWorkspaces, ["ws1"]);
  assert.equal(w.slotReleases, 1, "review decide holds no slot");
  assert.deepEqual([w.state.status, w.state.outcome, w.state.jobStatus], ["closed", "approved", "closed"]);
});

test("review decide --reject continues at step 10: ladder", async () => {
  const w = makeWorld({ reviewer: ["x", "y"] });
  await runAttempt(w.deps);
  const d = await decideReview({ ...w.deps, slot: null }, { verdict: "reject", feedback: "no tests" });
  assert.deepEqual(d, { state: "retry_queued", cause: "review_rejected", nextAttempt: "api-a2", nextTier: "S4" });
  assert.deepEqual(kinds(w), ["review-needed", "attempt-retry"]);
  assert.ok(w.closedPanes.includes("pane-w"), "the Worker pane is closed (read from the Ledger, not memory)");
});

test("review decide refuses an Attempt that is not waiting after review-needed (nothing is written)", async () => {
  for (const script of [{}, { worker: ["crash" as const] }]) {
    const w = makeWorld(script);
    await runAttempt(w.deps);
    const before = [...w.ledger];
    await assert.rejects(
      decideReview({ ...w.deps, slot: null }, { verdict: "approve", feedback: "x" }),
      (e: unknown) => e instanceof CpdError && e.code === "not_awaiting_review" && e.exitCode === 3,
    );
    assert.deepEqual(w.ledger, before);
  }
  // An Attempt in_review whose Reviewer is still running (no review-needed) is not decidable either.
  const w = makeWorld({ reviewer: [verdictText("approve")] });
  w.state.status = "in_review";
  w.state.jobStatus = "in_review";
  await assert.rejects(decideReview({ ...w.deps, slot: null }, { verdict: "approve", feedback: "x" }), (e: unknown) => e instanceof CpdError && e.code === "not_awaiting_review");
});

test("merge_conflict → ladder", async () => {
  const { w, r } = await go({ merge: [outcome("conflict", { files: ["app.txt"], detail: "rebase conflict in: app.txt" })] });
  assert.deepEqual(r, { state: "retry_queued", cause: "merge_conflict", nextAttempt: "api-a2", nextTier: "S4" });
  assert.equal(w.ledger.at(-1), "fail merge_conflict requeue=true");
  assert.deepEqual(kinds(w), ["attempt-retry"]);
  assert.match(w.messages[0]!.text, /app\.txt/);
  assert.deepEqual(w.closedPanes.sort(), ["pane-r", "pane-w"]);
  assert.deepEqual(w.closedWorkspaces, []);
  assert.deepEqual([w.state.outcome, w.state.jobStatus], ["failed", "queued"], "approved is rewritten to failed");
  assert.equal(w.slotReleases, 1);
});

// --- reviewMode none: no Reviewer, submit goes straight to the merge ---

test("reviewMode none: no Reviewer, no review files, the Attempt ends with outcome merged and no verdict", async () => {
  for (const p of [reviewDiffPath("demo", "api-a1"), reviewBriefPath("demo", "api-a1"), reviewGatePath("demo", "api-a1")]) rmSync(p, { force: true }); // left by the tests above, same CPD_HOME
  const w = makeWorld({ reviewMode: "none" });
  const r = await runAttempt(w.deps);
  assert.deepEqual(r, { state: "merged", sha: SHA_POST });
  assert.deepEqual(w.ledger, [
    "verifyJob",
    "recordFacts worktree",
    "recordFacts branch,agent,session_path,argv,worker_pane,workspace",
    "recordMetrics metrics",
    `markMerged ${SHA_POST}`,
  ]);
  assert.ok(!w.events.some((e) => /herdr\.split|reviewBrief|git\.diff|recordVerdict/.test(e)), w.events.join("\n"));
  assert.ok(!w.agentStarts.some((a) => a.name === reviewerName()));
  for (const p of [reviewDiffPath("demo", "api-a1"), reviewBriefPath("demo", "api-a1")]) assert.ok(!existsSync(p), `no review file ${p}`);
  assert.deepEqual(kinds(w), ["job-merged"]);
  assert.deepEqual([w.state.status, w.state.outcome, w.state.jobStatus], ["closed", "merged", "closed"]);
});

test("reviewMode none: a failed merge still goes down the ladder", async () => {
  const w = makeWorld({ reviewMode: "none", merge: [outcome("conflict", { detail: "rebase conflict in: app.txt" })] });
  const r = await runAttempt(w.deps);
  assert.equal(r.state, "retry_queued");
  assert.equal(w.ledger.at(-1), "fail merge_conflict requeue=true");
  assert.ok(!w.events.some((e) => e.includes("herdr.split")));
});

// --- every other mergeAttempt result and the merge lock ---

test("mergeAttempt results without a listed ladder: dirty_target, tampered, cas_lost, unknown → job-failed", async () => {
  const table: [MergeOutcome["result"], string, "job_failed"][] = [
    ["dirty_target", "dirty_target", "job_failed"],
    ["tampered", "gate_tampered", "job_failed"],
    ["cas_lost", "merge_cas_lost", "job_failed"],
    ["merge_unverified", "merge_unverified", "job_failed"],
    ["unknown", "merge_failed", "job_failed"],
  ];
  for (const [result, cause, state] of table) {
    const { w, r } = await go({ merge: [outcome(result)] });
    assert.deepEqual([r.state, (r as { cause: string }).cause], [state, cause], result);
    assert.equal(w.ledger.at(-1), `fail ${cause} requeue=false`);
    assert.equal(w.slotReleases, 1);
  }
});

test("dirty_target explains that the Lead worktree must stay clean", async () => {
  const { w } = await go({ merge: [outcome("dirty_target")] });
  assert.match(w.messages[0]!.text, /Lead worktree must stay clean/);
});

const lockTimeout = () => new CpdError("lock_timeout", "timed out after 60000 ms waiting for lock", 5, { path: "/locks/merge" });

test("lock_timeout from mergeAttempt means another merge is running: it is retried, never treated as a result", async () => {
  const { w, r } = await go({ merge: [lockTimeout(), lockTimeout(), lockTimeout(), merged], mergeTakesMs: 60_000 });
  assert.equal(r.state, "merged");
  assert.equal(w.mergeCalls, 4);
  assert.ok(w.logs.some((l) => l.includes("another merge is running")));
  assert.equal(kinds(w).join(), "job-merged");
});

test("lock_timeout for 90 minutes in total → job-failed, cause merge_lock_timeout", async () => {
  const { w, r } = await go({ merge: [lockTimeout()], mergeTakesMs: 60_000 });
  assert.deepEqual(r, { state: "job_failed", cause: "merge_lock_timeout" });
  assert.equal(w.mergeCalls, 90, "one try per minute-long lock wait until 90 minutes have passed");
  assert.equal(w.ledger.at(-1), "fail merge_lock_timeout requeue=false");
  assert.deepEqual(kinds(w), ["job-failed"]);
  assert.match(w.messages[0]!.text, /merge lock stayed busy/);
  assert.equal(w.slotReleases, 1);
});

test("an error other than lock_timeout from mergeAttempt is not retried: error message, rethrown, slot released", async () => {
  const w = makeWorld({ merge: [failure("rollback_failed", "git reset failed")] });
  await assert.rejects(runAttempt(w.deps), (e: unknown) => e instanceof CpdError && e.code === "rollback_failed");
  assert.equal(w.mergeCalls, 1);
  assert.deepEqual(kinds(w), ["error"]);
  assert.match(w.messages[0]!.text, /step 11 merge/);
  assert.equal(w.slotReleases, 1);
});

// --- Reviewer modes and failures ---

test("reviewMode fork: the Reviewer is forked from the Lead session; fresh: no --fork (acceptance 2)", async () => {
  const fork = await go({ reviewMode: "fork" });
  const forkArgs = fork.w.agentStarts.find((a) => a.name === reviewerName())!.args;
  assert.deepEqual(forkArgs, ["--approve", "--no-skills", "--exclude-tools", "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox", "--fork", LEAD_SESSION, "--tools", "read,grep,find,ls", "--model", "p/sonnet:xhigh"]);

  const fresh = await go({ reviewMode: "fresh" });
  const freshArgs = fresh.w.agentStarts.find((a) => a.name === reviewerName())!.args;
  assert.deepEqual(freshArgs, ["--approve", "--no-skills", "--exclude-tools", "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox", "--tools", "read,grep,find,ls", "--model", "p/sonnet:xhigh"]);
  assert.equal(freshArgs.includes("--fork"), false);
});

test("the Reviewer runs in a pane split from the Attempt's Worker pane, in the Lead's cwd; its metrics skip the forked history", async () => {
  const { w } = await go();
  assert.ok(w.events.includes("herdr.split pane-w /lead/cwd"));
  assert.equal(w.events.filter((e) => e.startsWith("herdr.split")).length, 1);
  const reviewerMetrics = w.metricsCalls.find((c) => c.path.endsWith("reviewer.jsonl"))!;
  assert.equal(typeof reviewerMetrics.since, "number");
  assert.equal(w.metricsCalls.find((c) => c.path.endsWith("worker.jsonl"))!.since, undefined);
});

test("an Attempt without a recorded worker_pane has no Reviewer pane to split: no verdict, review-needed, and nothing is split", async () => {
  const w = makeWorld();
  const load = w.deps.ledger.load;
  w.deps.ledger.load = async () => {
    const l = await load();
    return { ...l, attempt: { ...l.attempt, metadata: { ...l.attempt.metadata, worker_pane: undefined } } };
  };
  assert.deepEqual(await runAttempt(w.deps), { state: "review_needed" });
  assert.match(w.messages[0]!.text, /worker_pane_missing/);
  assert.equal(w.events.some((e) => e.startsWith("herdr.split")), false);
});

test("a Reviewer that cannot start counts as no verdict: review-needed, with the reason", async () => {
  const { w, r } = await go({ startFails: "reviewer" });
  assert.deepEqual(r, { state: "review_needed" });
  assert.deepEqual(kinds(w), ["review-needed"]);
  assert.match(w.messages[0]!.text, /could not run/);
});

test("a valid verdict on the re-prompt is accepted", async () => {
  const { r, w } = await go({ reviewer: ["no block", verdictText("approve")] });
  assert.equal(r.state, "merged");
  assert.equal(w.prompts.filter((p) => p.name === reviewerName()).length, 2);
});

test("a Reviewer whose last turn did not end (stopReason != stop) is not read", async () => {
  const w = makeWorld({ reviewer: [verdictText("approve"), verdictText("approve")] });
  const orig = w.deps.session.lastAssistant;
  w.deps.session.lastAssistant = (p) => (p.endsWith("reviewer.jsonl") ? { stopReason: "toolUse", text: verdictText("approve") } : orig(p));
  const r = await runAttempt(w.deps);
  assert.deepEqual(r, { state: "review_needed" });
});

// --- Worker start, cancel, unexpected failures ---

test("the Worker cannot start → job-failed worker_start_failed, no ladder, the pane is closed", async () => {
  const { w, r } = await go({ startFails: "worker" });
  assert.deepEqual(r, { state: "job_failed", cause: "worker_start_failed" });
  assert.equal(w.ledger.at(-1), "fail worker_start_failed requeue=false");
  assert.deepEqual(w.closedPanes, ["pane-w"]);
  assert.deepEqual(kinds(w), ["job-failed"]);
  assert.equal(w.slotReleases, 1);
});

test("closed by `job cancel` (watch) while the Worker runs: nothing is touched, no message, slot released", async () => {
  const { w, r } = await go({ worker: ["cancel"] });
  assert.deepEqual(r, { state: "closed_externally", outcome: "cancelled" });
  assert.deepEqual(w.messages, []);
  assert.equal(w.ledger.some((l) => l.startsWith("fail")), false);
  assert.equal(w.slotReleases, 1);
});

test("an already closed Attempt is a no-op", async () => {
  const w = makeWorld();
  w.state.status = "closed";
  w.state.outcome = "approved";
  assert.deepEqual(await runAttempt(w.deps), { state: "closed_externally", outcome: "approved" });
  assert.deepEqual(w.events, ["slot.release"]);
});

test("an unexpected failure is logged, reported to the Lead as `error`, rethrown, and the slot is still released", async () => {
  const w = makeWorld({ renderThrows: true });
  await assert.rejects(runAttempt(w.deps), /template exploded/);
  assert.deepEqual(kinds(w), ["error"]);
  assert.match(w.messages[0]!.text, /step 3 spec/);
  assert.match(w.messages[0]!.text, /template exploded/);
  assert.equal(w.slotReleases, 1);
  assert.equal(w.state.status, "in_progress", "the Attempt is left as it is for the Lead to decide");
});

test("a failing pane close never stops the lifecycle (logged, not fatal)", async () => {
  const { r, w } = await go({ worker: ["crash"], closePaneFails: true });
  assert.equal(r.state, "retry_queued");
  assert.ok(w.logs.some((l) => l.includes("cleanup failed: close Worker pane pane-w")));
});

test("a locked Job that no longer matches its fingerprint → job-failed, nothing is started", async () => {
  const w = makeWorld();
  w.deps.ledger.verifyJob = async () => {
    throw new CpdError("locked_job_tampered", "Job api was changed after plan lock", 3);
  };
  const r = await runAttempt(w.deps);
  assert.deepEqual(r, { state: "job_failed", cause: "locked_job_tampered" });
  assert.equal(w.events.some((e) => e.startsWith("git.") || e.startsWith("herdr.")), false);
});

test("Worker metrics are written once, and a missing session file only logs", async () => {
  const w = makeWorld({ worker: ["crash"] });
  w.deps.session.metrics = () => {
    throw Object.assign(new Error("ENOENT: no such file"), { code: "ENOENT" });
  };
  const r = await runAttempt(w.deps);
  assert.equal(r.state, "retry_queued");
  assert.equal(w.ledger.some((l) => l.startsWith("recordMetrics")), false);
  assert.ok(w.logs.some((l) => l.includes("metrics not recorded")));
});

test("the gate output of the submit must exist before the review", async () => {
  const w = makeWorld();
  // Make the fake Worker "submit" without writing the gate file.
  const orig = w.deps.herdr.promptAgent;
  w.deps.herdr.promptAgent = async (name, text, o) => {
    const res = await orig(name, text, o);
    const { rmSync } = await import("node:fs");
    const { reviewGatePath } = await import("../../src/lib/paths.ts");
    rmSync(reviewGatePath("demo", "api-a1"), { force: true });
    return res;
  };
  await assert.rejects(runAttempt(w.deps), (e: unknown) => e instanceof CpdError && e.code === "gate_output_missing");
  assert.deepEqual(kinds(w), ["error"]);
});

// --- step 7 decision table (observeWorker) ---

test("step 7 decision table: Ledger submitted_at, timed_out, agent gone, last stopReason", async () => {
  type Row = { name: string; set(w: World): void; agentGone?: boolean; last?: { stopReason: string; text: string } | null; want: string };
  const rows: Row[] = [
    { name: "submitted wins over everything", set: (w) => { w.state.submittedAt = "x"; w.state.metadata.timed_out = true; }, agentGone: true, last: null, want: "submitted" },
    { name: "timed_out beats a gone agent", set: (w) => { w.state.metadata.timed_out = true; }, agentGone: true, want: "timed_out" },
    { name: "agent gone → crashed", set: () => {}, agentGone: true, want: "crashed" },
    { name: "stopReason toolUse → crashed", set: () => {}, last: { stopReason: "toolUse", text: "" }, want: "crashed" },
    { name: "no assistant message → crashed", set: () => {}, last: null, want: "crashed" },
    { name: "stop, not submitted → idle (herdr idle alone is not enough to call it done)", set: () => {}, last: { stopReason: "stop", text: "done" }, want: "idle" },
    { name: "closed by someone else", set: (w) => { w.state.status = "closed"; w.state.outcome = "cancelled"; }, want: "closed" },
  ];
  for (const row of rows) {
    const w = makeWorld();
    await w.deps.ledger.recordFacts({ session_path: "/sessions/worker.jsonl" });
    row.set(w);
    w.deps.herdr.agentGet = async () => (row.agentGone ? null : { pane_id: "pane-w", agent_status: "idle" });
    w.deps.session.lastAssistant = () => (row.last === undefined ? { stopReason: "stop", text: "done" } : row.last);
    assert.equal((await observeWorker(w.deps, workerName())).kind, row.want, row.name);
  }
});

test("step 7: a missing or torn Worker session file is evidence of a crash, not an error", async () => {
  for (const code of ["session_not_found", "session_malformed"]) {
    const w = makeWorld();
    await w.deps.ledger.recordFacts({ session_path: "/sessions/worker.jsonl" });
    w.deps.session.lastAssistant = () => {
      throw new CpdError(code, "bad session", 1);
    };
    assert.equal((await observeWorker(w.deps, workerName())).kind, "crashed", code);
  }
});

test("the lifecycle logs every step to the attempt log", async () => {
  const { w } = await go();
  for (const step of ["1 load", "2 worktree", "3 spec", "4 placement", "5 start", "6 prompt", "7 decide", "9 review", "11 merge", "12 merged", "slot"]) {
    assert.ok(w.logs.some((l) => l.startsWith(`[${step}]`)), `no log line for step ${step}`);
  }
});

// --- the Reviewer of a tmux Lead: a hidden window of the Lead's tmux server, not a herdr pane (contract §12 step 9) ---

const tmuxGo = (script: Script = {}) => go({ leadHost: "tmux", ...script });
/** The words of a window command that tmux runs through the shell: every word single-quoted. */
const words = (command: string): string[] => command.slice(1, -1).split("' '");
const newWindowOf = (w: World) => w.tmuxCalls.find((c) => c.args[0] === "new-window")!;
const sends = (w: World) => w.tmuxCalls.filter((c) => c.args[0] === "send-keys").map((c) => c.args);
const LEAD_ONLY = "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox";

test("tmux Lead: the Reviewer starts in a new window of the Lead's server, forked from the Lead, read-only, with respawn off; a valid verdict merges; the window is killed", async () => {
  const { w, r } = await tmuxGo();
  assert.deepEqual(r, { state: "merged", sha: SHA_POST });

  const open = newWindowOf(w);
  assert.equal(open.socket, TMUX_SOCKET);
  assert.deepEqual(open.args.slice(0, 6), ["new-window", "-d", "-n", "review api-a1", "-c", "/wt/api-a1"]);
  const argv = words(open.args[6]!);
  const id = argv[argv.indexOf("--session-id") + 1]!;
  assert.match(id, /^[0-9a-f-]{36}$/);
  assert.deepEqual(argv, [
    "pi", "--approve", "--no-skills", "--exclude-tools", LEAD_ONLY, "--fork", LEAD_SESSION, "--tools", "read,grep,find,ls", "--model", "p/sonnet:xhigh",
    "--session-id", id, "-n", "review api-a1", "--", "Read /reviews/api-a1.md and follow it exactly.",
  ]);

  assert.deepEqual(w.tmuxCalls.map((c) => c.socket), Array(w.tmuxCalls.length).fill(TMUX_SOCKET), "every call goes to the Lead's server");
  assert.deepEqual(w.tmuxCalls.slice(1).map((c) => c.args), [
    ["set-option", "-w", "-t", TMUX_REVIEWER_PANE, "remain-on-exit", "off"],
    ["kill-window", "-t", TMUX_REVIEWER_PANE],
  ]);
  assert.deepEqual(sends(w), [], "a valid first verdict needs no re-prompt");
  assert.equal(w.events.some((e) => e.startsWith("herdr.split") || e === `herdr.startAgent ${reviewerName()}`), false, "no herdr pane for the Reviewer");
  assert.equal(w.prompts.some((p) => p.name === reviewerName()), false);
  assert.equal(w.metricsCalls.find((c) => c.path.endsWith("reviewer.jsonl"))!.since! > 0, true, "its metrics skip the forked history");
  assert.ok(w.ledger.includes("recordVerdict approve"));
});

test("tmux Lead, reviewMode fresh: no --fork", async () => {
  const { w } = await tmuxGo({ reviewMode: "fresh" });
  assert.equal(words(newWindowOf(w).args[6]!).includes("--fork"), false);
});

test("tmux Lead: an invalid verdict is re-prompted once through send-keys (text, then Enter), then accepted", async () => {
  const { w, r } = await tmuxGo({ reviewer: ["I think it is fine.", verdictText("reject", "tests are missing")] });
  assert.deepEqual(r, { state: "retry_queued", cause: "review_rejected", nextAttempt: "api-a2", nextTier: "S4" });
  const [text, enter, ...more] = sends(w);
  assert.deepEqual(more, []);
  assert.deepEqual(text!.slice(0, 3), ["send-keys", "-t", TMUX_REVIEWER_PANE]);
  assert.equal(text![3], "-l");
  assert.match(text![4]!, /did not end with a valid verdict/);
  assert.deepEqual(enter, ["send-keys", "-t", TMUX_REVIEWER_PANE, "Enter"]);
  assert.equal(w.tmuxCalls.at(-1)!.args[0], "kill-window");
});

test("tmux Lead: invalid twice → review-needed, one re-prompt only, window killed, metrics recorded", async () => {
  const { w, r } = await tmuxGo({ reviewer: ["I think it is fine.", "Still no block, sorry."] });
  assert.deepEqual(r, { state: "review_needed" });
  assert.deepEqual(kinds(w), ["review-needed"]);
  assert.equal(sends(w).length, 2);
  assert.deepEqual(w.tmuxCalls.at(-1)!.args, ["kill-window", "-t", TMUX_REVIEWER_PANE]);
  assert.ok(w.ledger.includes("recordMetrics review_metrics"));
});

test("tmux Lead: a Reviewer that never finishes a turn is bounded by the Attempt timeout → review-needed, window killed", async () => {
  const { w, r } = await tmuxGo({ reviewerSilent: true });
  assert.deepEqual(r, { state: "review_needed" });
  assert.match(w.messages[0]!.text, /reviewer_timeout/);
  assert.equal(sends(w).length, 0);
  assert.deepEqual(w.tmuxCalls.at(-1)!.args, ["kill-window", "-t", TMUX_REVIEWER_PANE]);
});

test("herdr Lead: no tmux call at all", async () => {
  const { w } = await go();
  assert.deepEqual(w.tmuxCalls, []);
});

// --- the real session readers of the tmux Reviewer, on fixture session files ---

const entry = (type: string, extra: object = {}) => JSON.stringify({ type, timestamp: "2026-01-01T00:00:10.000Z", ...extra });
const message = (role: string, stopReason?: string) => entry("message", { message: { role, ...(stopReason ? { stopReason } : {}), content: [] } });
const sessionFile = (lines: string[], end = "\n") => {
  const path = join(tmpDir("cpd-session-"), "s.jsonl");
  writeFileSync(path, lines.join("\n") + end);
  return path;
};

test("sessionTail: the newest message entry, whatever its role; other entries are skipped; the mtime is the file's", () => {
  const path = sessionFile([entry("session", { id: "x", cwd: "/" }), message("assistant", "stop"), message("user"), entry("model_change")]);
  utimesSync(path, 1_000, 1_000);
  assert.deepEqual(sessionTail(path), { role: "user", stopReason: null, writtenMs: Date.parse("2026-01-01T00:00:10.000Z"), mtimeMs: 1_000_000 });
  assert.equal(sessionTail(sessionFile([message("assistant", "toolUse")]))!.stopReason, "toolUse");
  assert.equal(sessionTail(sessionFile([entry("session", { id: "x", cwd: "/" })])), null);
});

test("sessionTail: a last line still being written is ignored; a broken complete line fails loud", () => {
  assert.equal(sessionTail(sessionFile([message("user"), message("assistant", "stop").slice(0, 20)], ""))!.role, "user");
  assert.throws(() => sessionTail(sessionFile([message("user"), "{broken", message("assistant", "stop")].slice(0, 2))), (e: unknown) => e instanceof CpdError && e.code === "session_malformed");
});

test("findSessionById: <agent dir>/sessions/*/*_<id>.jsonl, null while pi has not created it", () => {
  const agent = tmpDir("cpd-agent-");
  assert.equal(findSessionById(agent, "abc"), null);
  mkdirSync(join(agent, "sessions", "--proj--"), { recursive: true });
  writeFileSync(join(agent, "sessions", "--proj--", "2026-01-01T00-00-00-000Z_abc.jsonl"), "");
  writeFileSync(join(agent, "sessions", "--proj--", "2026-01-01T00-00-00-000Z_xabc2.jsonl"), "");
  assert.equal(findSessionById(agent, "abc"), join(agent, "sessions", "--proj--", "2026-01-01T00-00-00-000Z_abc.jsonl"));
  assert.equal(findSessionById(agent, "ab"), null);
});
