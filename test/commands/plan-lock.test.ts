import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { getPhase, initWorkflow, setPhase } from "../../src/ledger/workflow.ts";
import { listJobs } from "../../src/ledger/jobs.ts";
import { fakeBin } from "../helpers/index.ts";
import { createRepo } from "../git/fixtures.ts";
import { addDraft, approvePlan, cliSandbox } from "../ledger/helpers.ts";
import { runCli } from "../helpers/index.ts";

// `cpd plan lock` at the CLI, real Ledger, git and wt. herdr is a script that records its argv (it must not be called with --no-watch).
const sb = cliSandbox();
const herdr = fakeBin("herdr", "");
const cli = (args: string[]) => runCli(args, { env: { ...sb.env, PATH: `${herdr.dir}:${sb.env.PATH}` } });
const wf = "lockcli";

test("usage: --wf is required, unknown flags and arguments are refused (exit 2)", async () => {
  const none = await cli(["plan", "lock"]);
  assert.deepEqual([none.code, none.json.error.code], [2, "workflow_required"]);
  assert.equal((await cli(["plan", "lock", "--wf", wf, "--watch"])).code, 2);
  assert.equal((await cli(["plan", "lock", "--wf", wf, "now"])).code, 2);
});

test("an unknown Workflow is exit 4", async () => {
  const r = await cli(["plan", "lock", "--wf", "nosuch", "--no-watch"]);
  assert.deepEqual([r.code, r.json.error.code], [4, "workflow_not_found"]);
});

test("setup: a Workflow in phase plan with one draft Job and no Lead attached", { timeout: 300_000 }, async () => {
  const repo = await createRepo({ preStart: "true" });
  await initWorkflow({ wf, repo, title: "Lock CLI" });
  await setPhase(wf, "plan");
  await addDraft(wf, "api");
});

test("an unapproved plan is refused (exit 3) with or without --no-watch, and stays a draft", { timeout: 300_000 }, async () => {
  for (const flags of [[], ["--no-watch"]]) {
    const r = await cli(["plan", "lock", "--wf", wf, ...flags]);
    assert.deepEqual([r.code, r.json.error.code], [3, "plan_not_approved"], flags.join(" "));
  }
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["plan", ["draft"]]);
});

test("approved, but without --no-watch the Lead pane is needed: lead_not_attached (exit 3), and nothing is locked", { timeout: 300_000 }, async () => {
  await approvePlan(wf);
  const r = await cli(["plan", "lock", "--wf", wf]);
  assert.deepEqual([r.code, r.json.error.code], [3, "lead_not_attached"]);
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["plan", ["draft"]]);
});

test("--no-watch with an approved plan: {ok, leadBranch, leadWorktree, watchPane: null}, Jobs queued, herdr never called", { timeout: 300_000 }, async () => {
  const r = await cli(["plan", "lock", "--wf", wf, "--no-watch"]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.deepEqual(Object.keys(r.json).sort(), ["leadBranch", "leadWorktree", "ok", "watchPane"]);
  assert.deepEqual([r.json.ok, r.json.leadBranch, r.json.watchPane], [true, `cpd/${wf}`, null]);
  assert.ok(existsSync(r.json.leadWorktree));
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["dispatch", ["queued"]]);
  assert.deepEqual(herdr.calls(), []);

  const again = await cli(["plan", "lock", "--wf", wf, "--no-watch"]);
  assert.deepEqual([again.code, again.json.error.code], [3, "illegal_phase"]);
});
