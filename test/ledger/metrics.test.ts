import { test } from "node:test";
import assert from "node:assert/strict";
import { CpdError } from "../../src/lib/errors.ts";
import { getAttempt, startAttempt } from "../../src/ledger/attempts.ts";
import { metricKeys, recordMetrics } from "../../src/ledger/metrics.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, allIssues, approvePlan, newPlan, sandbox } from "./helpers.ts";

const { repo } = sandbox();
const wf = "metrics";
const refusal = (code: string, exit = 3) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;

const m = {
  turns: 14, tokensIn: 1000, tokensOut: 250, cacheRead: 5000, cacheWrite: 300, costUsd: 0.42, durationS: 93,
  model: "p/sonnet", thinkingLevel: "high", finalStopReason: "stop", skipped: 2,
};

test("keys are flat and follow contract §14", () => {
  assert.deepEqual(metricKeys({ ...m }, "metrics"), {
    metrics_turns: 14, metrics_tokens_in: 1000, metrics_tokens_out: 250, metrics_cache_read: 5000, metrics_cache_write: 300,
    metrics_cost_usd: 0.42, metrics_duration_s: 93, metrics_model: "p/sonnet", metrics_thinking_level: "high", metrics_stop_reason: "stop",
  });
  assert.ok(Object.keys(metricKeys({ ...m, model: null }, "review_metrics")).every((k) => k.startsWith("review_metrics_") && !k.endsWith("_model")));
});

test("3. recordMetrics is write-once per prefix; Worker and Reviewer metrics coexist", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await approvePlan(wf);
  await lockPlan(wf);
  await startAttempt(wf, "a", { tier: "S3" });

  await recordMetrics(wf, "a-a1", m, { prefix: "metrics" });
  const a = await getAttempt(wf, "a-a1");
  assert.equal(a.metadata.metrics_turns, 14);
  assert.equal(a.metadata.metrics_cost_usd, 0.42);
  assert.equal(a.metadata.metrics_stop_reason, "stop");
  assert.equal(a.metadata.tier, "S3", "other keys survive");

  const before = await allIssues(wf);
  await assert.rejects(recordMetrics(wf, "a-a1", { ...m, turns: 99 }, { prefix: "metrics" }), refusal("metadata_write_once"));
  assert.deepEqual(await allIssues(wf), before);
  assert.equal((await getAttempt(wf, "a-a1")).metadata.metrics_turns, 14);

  await recordMetrics(wf, "a-a1", { ...m, turns: 3 }, { prefix: "review_metrics" });
  const after = await getAttempt(wf, "a-a1");
  assert.deepEqual([after.metadata.review_metrics_turns, after.metadata.metrics_turns], [3, 14]);

  await assert.rejects(recordMetrics(wf, "a-a1", { ...m, turns: -1 }, { prefix: "metrics" }), refusal("invalid_metrics", 2));
  await assert.rejects(recordMetrics(wf, "zz-a1", m, { prefix: "metrics" }), refusal("attempt_not_found", 4));
});
