import { test } from "node:test";
import assert from "node:assert/strict";
import { markSubmitted, recordAttemptFacts, recordVerdict, startAttempt } from "../../src/ledger/attempts.ts";
import { recordEscalation } from "../../src/ledger/escalations.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, approvePlan, cliSandbox, newPlan } from "../ledger/helpers.ts";

const { repo, cli } = cliSandbox();
const wf = "attshow";
const sha = "d".repeat(40);

test("attempt show prints facts, verdict comments, Escalations and provenance", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "api");
  await approvePlan(wf);
  await lockPlan(wf);
  await startAttempt(wf, "api", { tier: "S3" });
  await recordAttemptFacts(wf, "api-a1", { worktree: "/w", branch: "cpd/attshow--api-a1", agent: "c1", session_path: "/s.jsonl" });
  await recordEscalation(wf, "api-a1", { escId: "api-a1-q1", question: "which?", askedAt: "2026-01-01T00:00:00.000Z" });

  const open = await cli(["attempt", "show", "api-a1", "--wf", wf]);
  assert.equal(open.code, 0, open.stdout + open.stderr);
  assert.deepEqual(
    [open.json.attempt.id, open.json.attempt.status, open.json.attempt.tier, open.json.attempt.worktree, open.json.attempt.branch, open.json.attempt.sessionPath],
    ["api-a1", "in_progress", "S3", "/w", "cpd/attshow--api-a1", "/s.jsonl"],
  );
  assert.deepEqual(open.json.escalations, [{ id: "api-a1-q1", status: "open", question: "which?" }]);
  assert.deepEqual(open.json.provenance.map((p: any) => p.kind), ["cut"]);
  assert.deepEqual(open.json.comments, []);
});

test("attempt show after a verdict: comment and commit provenance", async () => {
  const { recordAnswer } = await import("../../src/ledger/escalations.ts");
  await recordAnswer(wf, "api-a1-q1", { answer: "this", answeredAt: "2026-01-01T00:01:00.000Z" });
  await markSubmitted(wf, "api-a1", sha);
  await recordVerdict(wf, "api-a1", { verdict: "reject", feedback: "no good" });
  const r = await cli(["attempt", "show", "api-a1", "--wf", wf]);
  assert.equal(r.code, 0, r.stdout);
  assert.deepEqual([r.json.attempt.status, r.json.attempt.outcome], ["in_review", "rejected"]);
  assert.deepEqual(r.json.comments.map((c: any) => [c.author, c.text]), [["cpd", "REJECTED: no good"]]);
  assert.deepEqual(r.json.provenance.map((p: any) => [p.kind, p.ref]).sort(), [["commit", sha], ["cut", "cpd/attshow--api-a1"]]);
  assert.deepEqual(r.json.escalations.map((e: any) => e.status), ["closed"]);
});

test("attempt show errors: unknown, malformed, missing id", async () => {
  const none = await cli(["attempt", "show", "api-a7", "--wf", wf]);
  assert.deepEqual([none.code, none.json.error.code], [4, "attempt_not_found"]);
  const ghost = await cli(["attempt", "show", "ghost-a1", "--wf", wf]);
  assert.deepEqual([ghost.code, ghost.json.error.code], [4, "attempt_not_found"]);
  assert.equal((await cli(["attempt", "show", "Bad", "--wf", wf])).code, 2);
  assert.equal((await cli(["attempt", "show", "--wf", wf])).code, 2);
});
