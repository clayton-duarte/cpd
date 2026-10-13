import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { recordPlanDecision, lockPlan } from "../../src/ledger/plan.ts";
import { getPhase } from "../../src/ledger/workflow.ts";
import { workflowDir } from "../../src/lib/paths.ts";
import { runCli, tmpDir } from "../helpers/index.ts";
import { cliSandbox, rawBd, jobId, spec, writeSpec } from "../ledger/helpers.ts";

const { repo, cli, env } = cliSandbox();
const dir = tmpDir();
const specFile = writeSpec(dir, "spec.json", spec());

test("cpd workflow init creates the Workflow; a second init is workflow_exists (exit 3)", async () => {
  const r = await cli(["workflow", "init", "--wf", "cli-one", "--repo", repo, "--title", "CLI one"]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.json.ok, true);
  assert.equal(r.json.workflow.id, "cli-one");
  assert.equal(r.json.workflow.leadBranch, "cpd/cli-one");
  assert.ok(existsSync(join(workflowDir("cli-one"), "workflow.json")));
  const again = await cli(["workflow", "init", "--wf", "cli-one", "--repo", repo, "--title", "CLI one"]);
  assert.equal(again.code, 3);
  assert.equal(again.json.error.code, "workflow_exists");
});

test("workflow init usage errors exit 2", async () => {
  const noWf = await cli(["workflow", "init", "--repo", repo, "--title", "t"]);
  assert.equal(noWf.code, 2);
  assert.equal(noWf.json.error.code, "workflow_required");
  const noRepo = await cli(["workflow", "init", "--wf", "cli-x", "--title", "t"]);
  assert.equal(noRepo.code, 2);
  const unknown = await cli(["workflow", "init", "--wf", "cli-x", "--repo", repo, "--title", "t", "--bogus"]);
  assert.equal(unknown.code, 2);
  assert.equal(unknown.json.error.code, "usage");
});

test("CPD_WF selects the Workflow when --wf is absent", async () => {
  const r = await cli(["workflow", "show"]);
  assert.equal(r.json.error.code, "workflow_required");
  const viaEnv = await runCli(["workflow", "show"], { env: { ...env, CPD_WF: "cli-one" } });
  assert.equal(viaEnv.code, 0, viaEnv.stdout);
  assert.equal(viaEnv.json.workflow.id, "cli-one");
});

test("cpd workflow phase plan is the only manual transition", async () => {
  assert.equal((await cli(["workflow", "show", "--wf", "cli-one"])).json.phase, "context");
  for (const to of ["dispatch", "pr", "done", "context"]) {
    const r = await cli(["workflow", "phase", to, "--wf", "cli-one"]);
    assert.equal(r.code, 3, to);
    assert.equal(r.json.error.code, "illegal_transition");
  }
  assert.equal((await cli(["workflow", "phase", "nonsense", "--wf", "cli-one"])).code, 2);
  assert.equal((await cli(["workflow", "phase", "--wf", "cli-one"])).code, 2);
  const ok = await cli(["workflow", "phase", "plan", "--wf", "cli-one"]);
  assert.equal(ok.code, 0, ok.stdout);
  assert.deepEqual(ok.json, { ok: true, phase: "plan" });
  assert.equal(await getPhase("cli-one"), "plan");
  const again = await cli(["workflow", "phase", "plan", "--wf", "cli-one"]);
  assert.equal(again.code, 3);
  assert.equal(again.json.error.code, "illegal_transition");
});

test("workflow show prints the §11 shape, with Jobs, counts and tamper events", async () => {
  await cli(["job", "add", "--wf", "cli-one", "--key", "a", "--title", "A", "--tier", "S2", "--spec", specFile]);
  await cli(["job", "add", "--wf", "cli-one", "--key", "b", "--title", "B", "--tier", "H1", "--spec", specFile, "--needs", "a"]);
  let s = await cli(["workflow", "show", "--wf", "cli-one"]);
  assert.equal(s.code, 0, s.stdout);
  assert.deepEqual(Object.keys(s.json).sort(), ["counts", "jobs", "ok", "phase", "tamper", "workflow"]);
  assert.equal(s.json.phase, "plan");
  assert.deepEqual(s.json.jobs, [
    { key: "a", title: "A", status: "draft", tier: "S2", needs: [], attempts: [] },
    { key: "b", title: "B", status: "draft", tier: "H1", needs: ["a"], attempts: [] },
  ]);
  assert.deepEqual(s.json.counts, { draft: 2, queued: 0, running: 0, in_review: 0, closed: 0, failed: 0, cancelled: 0 });
  assert.deepEqual(s.json.tamper, []);

  await recordPlanDecision("cli-one", "r1", "pending");
  await recordPlanDecision("cli-one", "r1", "approved");
  await lockPlan("cli-one");
  await rawBd("cli-one", ["update", await jobId("cli-one", "a"), "-d", "tampered"], "mallory");
  s = await cli(["workflow", "show", "--wf", "cli-one"]);
  assert.equal(s.json.phase, "dispatch");
  assert.equal(s.json.counts.queued, 2);
  assert.equal(s.json.tamper.length, 1);
  assert.equal(s.json.tamper[0].actor, "mallory");
});
