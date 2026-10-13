import { test } from "node:test";
import assert from "node:assert/strict";
import { recordAttemptFacts, startAttempt } from "../../src/ledger/attempts.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, approvePlan, cliSandbox, newPlan } from "../ledger/helpers.ts";

// `cpd attempt submit` at the CLI against a real Ledger. Every precondition with real git and wt:
// test/dispatch/attempt-submit.test.ts; the success path with a real Gate: test/dispatch/attempt-lifecycle.test.ts.
const { repo, cli } = cliSandbox();
const wf = "subcli";

test("setup: a locked Workflow with a started Attempt whose worktree is elsewhere", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "api");
  await approvePlan(wf);
  await lockPlan(wf);
  await startAttempt(wf, "api", { tier: "S3" });
  await recordAttemptFacts(wf, "api-a1", { worktree: "/definitely/not/here" });
});

test("a Worker running outside the Attempt worktree is refused with wrong_worktree (exit 3)", async () => {
  const r = await cli(["attempt", "submit", "api-a1", "--wf", wf]);
  assert.equal(r.code, 3, r.stdout + r.stderr);
  assert.equal(r.json.error.code, "wrong_worktree");
  assert.match(r.json.error.message, /inside the Attempt worktree/);
});

test("an unknown Attempt is exit 4; arguments are checked (exit 2)", async () => {
  const none = await cli(["attempt", "submit", "api-a7", "--wf", wf]);
  assert.deepEqual([none.code, none.json.error.code], [4, "attempt_not_found"]);
  assert.equal((await cli(["attempt", "submit", "--wf", wf])).code, 2);
  assert.equal((await cli(["attempt", "submit", "Bad", "--wf", wf])).code, 2);
  assert.equal((await cli(["attempt", "submit", "api-a1"])).json.error.code, "workflow_required");
});
