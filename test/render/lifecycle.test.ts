import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { failAttempt, markSubmitted, recordVerdict, startAttempt } from "../../src/ledger/attempts.ts";
import { withLedger } from "../../src/ledger/bd.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { renderWorkerSpec } from "../../src/render/spec.ts";
import { addDraft, approvePlan, jobId, newPlan, sandbox } from "../ledger/helpers.ts";

const { repo } = sandbox();

// The Worker Spec renders from Attempts written by the real lifecycle (`ledger-attempts`), not only from fixtures.
test("renderWorkerSpec reads Attempts as ledger-attempts writes them: Attempt 2 shows Attempt 1's outcome, cause and verdict", async () => {
  const wf = "life";
  await newPlan(wf, repo);
  await addDraft(wf, "api");
  await approvePlan(wf);
  await lockPlan(wf);
  await startAttempt(wf, "api", { tier: "S3", worktree: "/w/a1", branch: "cpd/life--api-a1" });
  await markSubmitted(wf, "api-a1", "a".repeat(40));
  await recordVerdict(wf, "api-a1", { verdict: "reject", feedback: "tests fail in api.spec" });
  await failAttempt(wf, "api-a1", "review_rejected", { requeue: true });
  await startAttempt(wf, "api", { tier: "S4", worktree: "/w/a2", branch: "cpd/life--api-a2" });

  const text = readFileSync(await renderWorkerSpec(wf, "api-a2"), "utf8");
  assert.ok(text.includes("Your Tier is `S4`") && text.includes("`/w/a2`"));
  const prev = text.slice(text.indexOf("## Previous Attempts"), text.indexOf("## Finish"));
  assert.match(prev, /### Attempt 1 \(Tier `S3`\)/);
  assert.match(prev, /Outcome: rejected/);
  assert.match(prev, /Cause: review_rejected/);
  assert.match(prev, /> REJECTED: tests fail in api\.spec/);
  assert.ok(text.includes("\n    cpd attempt submit api-a2 --wf life\n"));
});

test("malformed Attempt metadata is the named error attempt_metadata_invalid, not a raw ZodError", async () => {
  const wf = "meta";
  await newPlan(wf, repo);
  await addDraft(wf, "broken");
  const parent = await jobId(wf, "broken");
  await withLedger(wf, "test attempt", (l) =>
    l.write(["create", "--title=broken attempt 1", "-t", "attempt", "--parent", parent, "-s", "in_progress", "--metadata", JSON.stringify({ number: "one", tier: "S3" })]),
  );
  await assert.rejects(
    renderWorkerSpec(wf, "broken-a1"),
    (e: unknown) => e instanceof CpdError && e.code === "attempt_metadata_invalid" && e.exitCode === 1 && typeof e.details.id === "string",
  );
});
