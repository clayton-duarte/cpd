import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { cpdHome, slotsDir, workflowJsonPath } from "../../src/lib/paths.ts";
import { cliSandbox } from "../ledger/helpers.ts";

// `cpd job dispatch` at the CLI. The dispatch order and the spawn are covered in test/dispatch/dispatch-one.test.ts
// (fakes) and test/dispatch/attempt-lifecycle.test.ts (real Ledger, real detached spawn).
const { cli } = cliSandbox();

/** A workflow.json written by hand (no Ledger): enough for the refusals that happen before the Ledger is read. */
function handWritten(wf: string, lead: boolean): void {
  mkdirSync(join(cpdHome(), "workflows", wf), { recursive: true });
  writeFileSync(
    workflowJsonPath(wf),
    JSON.stringify({
      v: 1, id: wf, title: "t", repo: "/r", base: "main", leadBranch: `cpd/${wf}`, leadWorktree: null, createdAt: "2026-01-01T00:00:00.000Z",
      lead: lead ? { paneId: "w1:p1", sessionPath: "/s.jsonl", sessionId: "s1", queueDir: join(cpdHome(), "leads", "s1", "queue"), attachedAt: "2026-01-01T00:00:00.000Z" } : null,
      pr: null,
    }),
  );
}

test("usage errors exit 2: no Job key, no Workflow, bad key, unknown flag", async () => {
  assert.equal((await cli(["job", "dispatch", "--wf", "demo"])).code, 2);
  const noWf = await cli(["job", "dispatch", "api"]);
  assert.deepEqual([noWf.code, noWf.json.error.code], [2, "workflow_required"]);
  assert.equal((await cli(["job", "dispatch", "Bad_Key", "--wf", "demo"])).code, 2);
  assert.equal((await cli(["job", "dispatch", "api", "--wf", "demo", "--slot", "1"])).code, 2);
});

test("an unknown Workflow is exit 4", async () => {
  const r = await cli(["job", "dispatch", "api", "--wf", "nosuch"]);
  assert.deepEqual([r.code, r.json.error.code], [4, "workflow_not_found"]);
});

test("no attached Lead, or no Lead worktree: refused (exit 3) before any slot is taken", async () => {
  handWritten("nolead", false);
  const a = await cli(["job", "dispatch", "api", "--wf", "nolead"]);
  assert.deepEqual([a.code, a.json.error.code], [3, "lead_not_attached"]);
  handWritten("nowt", true);
  const b = await cli(["job", "dispatch", "api", "--wf", "nowt"]);
  assert.deepEqual([b.code, b.json.error.code], [3, "lead_worktree_missing"]);
  assert.equal(existsSync(slotsDir()) ? readdirSync(slotsDir()).length : 0, 0, "no slot file was created");
});
