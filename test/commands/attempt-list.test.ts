import { test } from "node:test";
import assert from "node:assert/strict";
import { markSubmitted, startAttempt } from "../../src/ledger/attempts.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, approvePlan, cliSandbox, newPlan } from "../ledger/helpers.ts";

const { repo, cli } = cliSandbox();
const wf = "attlist";

test("attempt list: empty, then Attempts in order, filtered by status", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await addDraft(wf, "b");
  await approvePlan(wf);
  await lockPlan(wf);
  const empty = await cli(["attempt", "list", "--wf", wf]);
  assert.deepEqual([empty.code, empty.json], [0, { ok: true, attempts: [] }]);

  await startAttempt(wf, "a", { tier: "S3" });
  await startAttempt(wf, "b", { tier: "S4" });
  await markSubmitted(wf, "b-a1", "e".repeat(40));
  const all = await cli(["attempt", "list", "--wf", wf]);
  assert.equal(all.code, 0, all.stdout + all.stderr);
  assert.deepEqual(
    all.json.attempts.map((a: any) => [a.id, a.job, a.number, a.tier, a.status, a.outcome, a.cause]),
    [["a-a1", "a", 1, "S3", "in_progress", null, null], ["b-a1", "b", 1, "S4", "in_review", null, null]],
  );
  const running = await cli(["attempt", "list", "--wf", wf, "--status", "in_progress"]);
  assert.deepEqual(running.json.attempts.map((a: any) => a.id), ["a-a1"]);
  assert.deepEqual((await cli(["attempt", "list", "--wf", wf, "--status", "closed"])).json.attempts, []);
});

test("attempt list: usage errors", async () => {
  const bad = await cli(["attempt", "list", "--wf", wf, "--status", "bogus"]);
  assert.deepEqual([bad.code, bad.json.error.code], [2, "invalid_status"]);
  assert.equal((await cli(["attempt", "list"])).json.error.code, "workflow_required");
  assert.equal((await cli(["attempt", "list", "--wf", wf, "--bogus", "1"])).code, 2);
  assert.equal((await cli(["attempt", "list", "--wf", wf, "extra"])).code, 2);
});
