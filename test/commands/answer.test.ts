import { test } from "node:test";
import assert from "node:assert/strict";
import { runCli } from "../helpers/index.ts";
import { setup, WF } from "../queue/helpers.ts";

test("answer on an unknown escalation is escalation_missing (exit 3)", async () => {
  const { env } = setup();
  const r = await runCli(["answer", "job-a1-q1", "x", "--wf", WF], { env });
  assert.equal(r.code, 3);
  assert.equal(r.json.error.code, "escalation_missing");
});

test("answer requires --wf and a well-formed id", async () => {
  const { env } = setup();
  assert.equal((await runCli(["answer", "job-a1-q1", "x"], { env })).json.error.code, "workflow_required");
  assert.equal((await runCli(["answer", "bad", "x", "--wf", WF], { env })).code, 2);
});
