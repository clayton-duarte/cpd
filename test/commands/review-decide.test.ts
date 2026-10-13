import { test } from "node:test";
import assert from "node:assert/strict";
import { cliSandbox } from "../ledger/helpers.ts";

// `cpd review decide` at the CLI: argument checks and the unknown Workflow. The decision itself, including
// `not_awaiting_review` and the merge, runs in test/dispatch/attempt-run.test.ts (fakes) and attempt-lifecycle.test.ts (real).
const { cli } = cliSandbox();

test("exactly one of --approve and --reject, and a non-blank --feedback, are required (exit 2)", async () => {
  const base = ["review", "decide", "api-a1", "--wf", "demo"];
  for (const args of [
    [...base, "--feedback", "x"],
    [...base, "--approve", "--reject", "--feedback", "x"],
    [...base, "--approve"],
    [...base, "--approve", "--feedback", "   "],
    ["review", "decide", "--wf", "demo", "--approve", "--feedback", "x"],
    ["review", "decide", "Bad", "--wf", "demo", "--approve", "--feedback", "x"],
    [...base, "--approve", "--feedback", "x", "--nope"],
  ]) {
    const r = await cli(args);
    assert.equal(r.code, 2, args.join(" "));
    assert.equal(r.json.ok, false);
  }
  const noWf = await cli(["review", "decide", "api-a1", "--approve", "--feedback", "x"]);
  assert.deepEqual([noWf.code, noWf.json.error.code], [2, "workflow_required"]);
});

test("an unknown Workflow is exit 4", async () => {
  const r = await cli(["review", "decide", "api-a1", "--approve", "--feedback", "fine", "--wf", "nosuch"]);
  assert.deepEqual([r.code, r.json.error.code], [4, "workflow_not_found"]);
});
