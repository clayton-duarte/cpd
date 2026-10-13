import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cpdHome, slotPath, workflowJsonPath } from "../../src/lib/paths.ts";
import { acquireSlot } from "../../src/lib/slots.ts";
import { cliSandbox } from "../ledger/helpers.ts";

// `cpd attempt run` at the CLI. The lifecycle itself: test/dispatch/attempt-run.test.ts (fakes) and attempt-lifecycle.test.ts (real).
const { cli } = cliSandbox();

test("usage errors exit 2: no id, bad id, no --slot, bad --slot, no Workflow", async () => {
  assert.equal((await cli(["attempt", "run", "--wf", "demo", "--slot", "0"])).code, 2);
  assert.equal((await cli(["attempt", "run", "Bad", "--wf", "demo", "--slot", "0"])).code, 2);
  const noSlot = await cli(["attempt", "run", "api-a1", "--wf", "demo"]);
  assert.deepEqual([noSlot.code, noSlot.json.error.code], [2, "usage"]);
  assert.match(noSlot.json.error.message, /--slot/);
  assert.equal((await cli(["attempt", "run", "api-a1", "--wf", "demo", "--slot", "x"])).code, 2);
  assert.equal((await cli(["attempt", "run", "api-a1", "--wf", "demo", "--slot", "-1"])).code, 2);
  const noWf = await cli(["attempt", "run", "api-a1", "--slot", "0"]);
  assert.deepEqual([noWf.code, noWf.json.error.code], [2, "workflow_required"]);
});

test("when the ports cannot be built the slot still goes back (unknown Workflow: exit 4)", async () => {
  const slot = await acquireSlot(3, { wf: "ghost", attempt: "api-a1" });
  assert.equal(slot, 0);
  const r = await cli(["attempt", "run", "api-a1", "--wf", "ghost", "--slot", "0"]);
  assert.deepEqual([r.code, r.json.error.code], [4, "workflow_not_found"]);
  assert.equal(existsSync(slotPath(0)), false, "the slot was released on the failed start");
});

test("no attached Lead: exit 3, and the slot goes back", async () => {
  mkdirSync(join(cpdHome(), "workflows", "nolead"), { recursive: true });
  writeFileSync(
    workflowJsonPath("nolead"),
    JSON.stringify({ v: 1, id: "nolead", title: "t", repo: "/r", base: "main", leadBranch: "cpd/nolead", leadWorktree: null, createdAt: "2026-01-01T00:00:00.000Z", lead: null, pr: null }),
  );
  assert.equal(await acquireSlot(3, { wf: "nolead", attempt: "api-a1" }), 0);
  const r = await cli(["attempt", "run", "api-a1", "--wf", "nolead", "--slot", "0"]);
  assert.deepEqual([r.code, r.json.error.code], [3, "lead_not_attached"]);
  assert.equal(existsSync(slotPath(0)), false);
});

test("a slot held by another Attempt is not released by a failed start of this one", async () => {
  assert.equal(await acquireSlot(3, { wf: "ghost", attempt: "other-a1" }), 0);
  const r = await cli(["attempt", "run", "api-a1", "--wf", "ghost", "--slot", "0"]);
  assert.equal(r.code, 4);
  assert.equal(existsSync(slotPath(0)), true, "someone else's slot stays");
});
