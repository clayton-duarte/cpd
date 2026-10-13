import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gitOut } from "../../src/git/git.ts";
import { planLock } from "../../src/dispatch/plan-lock.ts";
import { getPhase, initWorkflow, readWorkflow, setPhase, writeWorkflow } from "../../src/ledger/workflow.ts";
import { listJobs } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { cpdHome } from "../../src/lib/paths.ts";
import { baseConfig } from "../herdr/setup.ts";
import { fakeBin, tmpDir } from "../helpers/index.ts";
import { createRepo, sandbox } from "../git/fixtures.ts";
import { addDraft, approvePlan } from "../ledger/helpers.ts";

// `plan lock` against the real Ledger (bd), git and wt; only herdr is a script that records its argv.
const ORIGINAL_HOME = process.env.HOME;
const ORIGINAL_XDG = process.env.XDG_CONFIG_HOME;
sandbox();
mkdirSync(cpdHome(), { recursive: true });
writeFileSync(join(cpdHome(), "config.json"), JSON.stringify({ ...baseConfig, herdrSession: "fake-session" }));

const ORIGINAL_PATH = process.env.PATH ?? "";
const FAIL = join(tmpDir("cpd-fake-herdr-"), "fail-split");
const herdr = fakeBin(
  "herdr",
  `case "$*" in
  *" pane split "*)
    if [ -e '${FAIL}' ]; then echo '{"id":"fake","error":{"code":"pane_not_found","message":"pane w1:p1 not found"}}'; exit 1; fi
    echo '{"id":"fake","result":{"pane":{"pane_id":"w1:p9","workspace_id":"w1"},"type":"pane_info"}}' ;;
esac`,
);
process.env.PATH = `${herdr.dir}:${process.env.PATH}`;

const LEAD = { paneId: "w1:p1", sessionPath: "/sessions/lead.jsonl", sessionId: "lead1", queueDir: join(tmpDir("cpd-queue-"), "queue"), attachedAt: "2026-01-01T00:00:00.000Z" };

async function plan(wf: string, repo: string, opts: { lead: boolean }): Promise<void> {
  await initWorkflow({ wf, repo, title: `Workflow ${wf}` });
  await setPhase(wf, "plan");
  await addDraft(wf, "api");
  if (opts.lead) await writeWorkflow(wf, { ...readWorkflow(wf), lead: LEAD });
}

const code = (c: string, exit: number) => (e: unknown) => e instanceof CpdError && e.code === c && e.exitCode === exit;

test("plan lock: refuses without approval and without a Lead pane to split from; with both it locks, creates the Lead Branch worktree and starts `cpd watch` in a new pane", { timeout: 600_000 }, async () => {
  const wf = "lock-a";
  const repo = await createRepo({ preStart: "true" });
  await plan(wf, repo, { lead: false });

  // Not approved: refused first, whether or not a Lead is attached; nothing changes, nothing is created, herdr is not called.
  await assert.rejects(planLock(wf, { watch: true }), code("plan_not_approved", 3));
  await writeWorkflow(wf, { ...readWorkflow(wf), lead: LEAD });
  await assert.rejects(planLock(wf, { watch: true }), code("plan_not_approved", 3));
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["plan", ["draft"]]);
  assert.equal(await gitOut(repo, ["branch", "--list", `cpd/${wf}`]), "", "no Lead Branch");
  assert.equal(readWorkflow(wf).leadWorktree, null);
  assert.deepEqual(herdr.calls(), []);

  // Approved but no Lead pane to split from: refused before anything is locked.
  await approvePlan(wf);
  await writeWorkflow(wf, { ...readWorkflow(wf), lead: null });
  await assert.rejects(planLock(wf, { watch: true }), code("lead_not_attached", 3));
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["plan", ["draft"]]);
  assert.deepEqual(herdr.calls(), []);

  await writeWorkflow(wf, { ...readWorkflow(wf), lead: LEAD });
  const r = await planLock(wf, { watch: true });

  assert.deepEqual(Object.keys(r).sort(), ["leadBranch", "leadWorktree", "watchPane"]);
  assert.equal(r.leadBranch, `cpd/${wf}`);
  assert.equal(r.watchPane, "w1:p9");
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["dispatch", ["queued"]]);

  // The Lead Branch lives in its own worktree, and workflow.json records it.
  assert.equal(readWorkflow(wf).leadWorktree, r.leadWorktree);
  assert.ok(existsSync(r.leadWorktree));
  assert.equal(await gitOut(r.leadWorktree, ["rev-parse", "--abbrev-ref", "HEAD"]), `cpd/${wf}`);
  assert.match(await gitOut(repo, ["worktree", "list", "--porcelain"]), new RegExp(`branch refs/heads/cpd/${wf}\\n`));
  assert.equal(await gitOut(repo, ["rev-parse", "--abbrev-ref", "HEAD"]), "main", "the primary checkout is untouched");

  // The pane split and the run were recorded: split from the Lead pane, no focus, cwd = the Lead worktree; then `cpd watch --wf` typed into it.
  assert.deepEqual(herdr.calls(), [
    ["--session", "fake-session", "pane", "split", "w1:p1", "--direction", "right", "--cwd", r.leadWorktree, "--no-focus"],
    ["--session", "fake-session", "pane", "run", "w1:p9", `cpd watch --wf ${wf}`],
  ]);

  // A locked plan cannot be locked again, and nothing more is started.
  await assert.rejects(planLock(wf, { watch: true }), code("illegal_phase", 3));
  assert.equal(herdr.calls().length, 2);
});

test("plan lock interrupted after the lock continues from the Lead Branch; a watch pane that cannot start is watch_pane_failed, with the plan locked and the worktree there", { timeout: 600_000 }, async () => {
  const wf = "lock-c";
  const repo = await createRepo({ preStart: "true" });
  await plan(wf, repo, { lead: true });
  await approvePlan(wf);
  await lockPlan(wf); // as if `plan lock` died right after the Ledger part
  assert.equal(readWorkflow(wf).leadWorktree, null);

  const before = herdr.calls().length;
  writeFileSync(FAIL, "");
  await assert.rejects(planLock(wf, { watch: true }), (e: unknown) => {
    assert.ok(e instanceof CpdError);
    assert.equal(e.code, "watch_pane_failed");
    assert.equal(e.exitCode, 1);
    assert.equal(e.details.cause, "herdr_pane_not_found");
    assert.match(e.message, new RegExp(`cpd watch --wf ${wf}`));
    return true;
  });
  const w = readWorkflow(wf);
  assert.ok(w.leadWorktree !== null && existsSync(w.leadWorktree), "the Lead Branch worktree was created and recorded");
  assert.equal(await gitOut(w.leadWorktree!, ["rev-parse", "--abbrev-ref", "HEAD"]), `cpd/${wf}`);
  assert.deepEqual([await getPhase(wf), (await listJobs(wf)).map((j) => j.status)], ["dispatch", ["queued"]]);
  assert.equal(herdr.calls().length, before + 1, "the split failed, so nothing was typed");
});

test("real herdr: `pane run` types the command into a split pane's shell (CPD_IT=1, session cpd-test)", { skip: process.env.CPD_IT !== "1", timeout: 120_000 }, async () => {
  const { spawnSync } = await import("node:child_process");
  const { realpathSync, readFileSync } = await import("node:fs");
  const { setTimeout: sleep } = await import("node:timers/promises");
  const { runInPane } = await import("../../src/dispatch/plan-lock.ts");
  const { splitPane, closePane } = await import("../../src/herdr/panes.ts");
  const fakePath = process.env.PATH;
  const sandboxEnv = { HOME: process.env.HOME, XDG_CONFIG_HOME: process.env.XDG_CONFIG_HOME };
  process.env.PATH = ORIGINAL_PATH; // the real herdr, not the script above
  process.env.HOME = ORIGINAL_HOME; // and it finds its sockets under the real HOME
  if (ORIGINAL_XDG === undefined) delete process.env.XDG_CONFIG_HOME;
  else process.env.XDG_CONFIG_HOME = ORIGINAL_XDG;
  writeFileSync(join(cpdHome(), "config.json"), JSON.stringify({ ...baseConfig, herdrSession: "cpd-test" }));
  const dir = realpathSync(tmpDir("cpd-it-run-"));
  const herdrCli = (...a: string[]) => JSON.parse(spawnSync("herdr", ["--session", "cpd-test", ...a], { encoding: "utf8" }).stdout).result;
  const ws = herdrCli("workspace", "create", "--cwd", dir, "--label", "cpd it plan-lock", "--no-focus");
  try {
    const pane = await splitPane(ws.root_pane.pane_id, { direction: "right", cwd: dir, noFocus: true });
    await sleep(1500); // the shell of a fresh pane needs a moment before it reads input
    await runInPane(pane, `pwd > ${dir}/ran.txt`);
    const deadline = Date.now() + 20_000;
    while (!existsSync(join(dir, "ran.txt")) && Date.now() < deadline) await sleep(250);
    assert.equal(readFileSync(join(dir, "ran.txt"), "utf8").trim(), dir, "the command ran in the pane, in its cwd");
    await assert.rejects(runInPane("w99:p99", "echo nope"), (e: unknown) => e instanceof CpdError && e.code === "herdr_pane_not_found");
    await closePane(pane);
  } finally {
    herdrCli("workspace", "close", ws.workspace.workspace_id);
    process.env.PATH = fakePath;
    process.env.HOME = sandboxEnv.HOME;
    process.env.XDG_CONFIG_HOME = sandboxEnv.XDG_CONFIG_HOME;
    writeFileSync(join(cpdHome(), "config.json"), JSON.stringify({ ...baseConfig, herdrSession: "fake-session" }));
  }
});
