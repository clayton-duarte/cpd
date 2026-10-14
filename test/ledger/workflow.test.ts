import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { getPhase, initWorkflow, readWorkflow, setPhase, WorkflowJson } from "../../src/ledger/workflow.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { ledgerLockPath, workflowDir } from "../../src/lib/paths.ts";
import { checksums, gitOut, sandbox } from "./helpers.ts";

const { repo } = sandbox();

const refusal = (code: string, exit: number) => (e: unknown) =>
  e instanceof CpdError && e.code === code && e.exitCode === exit;

/** What F1 compares: status, local config, and the checksum of every file (hooks included). */
const snapshot = () => ({
  status: gitOut(repo, "status", "--porcelain"),
  config: gitOut(repo, "config", "--list", "--local"),
  files: checksums(repo), // includes .git/hooks and .git/config
});

test("workflow init leaves the product repo byte-identical and creates the §4 layout", async () => {
  snapshot(); // git status may refresh the index once
  const before = snapshot();
  const wf = await initWorkflow({ wf: "wf-one", repo, title: "First" });
  assert.deepEqual(snapshot(), before);

  const dir = workflowDir("wf-one");
  for (const d of ["ledger/.beads", "escalations", "specs", "reviews", "plannotator", "locks", "logs"]) {
    assert.ok(existsSync(join(dir, d)), `missing ${d}`);
  }
  const onDisk = WorkflowJson.parse(JSON.parse(readFileSync(join(dir, "workflow.json"), "utf8")));
  assert.deepEqual(onDisk, wf);
  assert.equal(onDisk.id, "wf-one");
  assert.equal(onDisk.leadBranch, "cpd/wf-one");
  assert.equal(onDisk.base, "main"); // no origin: default main
  assert.equal(onDisk.leadWorktree, null);
  assert.equal(onDisk.lead, null);
  assert.equal(onDisk.pr, null);
  assert.equal(await getPhase("wf-one"), "context");
  assert.equal(existsSync(ledgerLockPath("wf-one")), false, "lock released");
});

test("a second init is workflow_exists (exit 3) and changes nothing", async () => {
  const before = readFileSync(join(workflowDir("wf-one"), "workflow.json"), "utf8");
  await assert.rejects(initWorkflow({ wf: "wf-one", repo, title: "Again" }), refusal("workflow_exists", 3));
  assert.equal(readFileSync(join(workflowDir("wf-one"), "workflow.json"), "utf8"), before);
});

test("init validates its inputs before writing anything", async () => {
  await assert.rejects(initWorkflow({ wf: "Bad_Id", repo, title: "x" }), refusal("invalid_workflow_id", 2));
  await assert.rejects(initWorkflow({ wf: "wf-bad", repo: "/nonexistent/repo", title: "x" }), refusal("repo_not_found", 2));
  await assert.rejects(initWorkflow({ wf: "wf-bad", repo, title: "  " }), refusal("invalid_title", 2));
  assert.equal(existsSync(workflowDir("wf-bad")), false);
});

test("--base is recorded as given", async () => {
  const wf = await initWorkflow({ wf: "wf-base", repo, title: "Base", base: "develop" });
  assert.equal(wf.base, "develop");
  assert.equal(readWorkflow("wf-base").base, "develop");
});

test("readWorkflow of an unknown Workflow is workflow_not_found (exit 4)", () => {
  assert.throws(() => readWorkflow("nope-nope"), refusal("workflow_not_found", 4));
});

test("phase transitions follow the table and nothing else", async () => {
  assert.equal(await getPhase("wf-one"), "context");
  for (const to of ["dispatch", "pr", "done", "context"] as const) {
    await assert.rejects(setPhase("wf-one", to), refusal("illegal_transition", 3), `context -> ${to}`);
  }
  await setPhase("wf-one", "plan");
  assert.equal(await getPhase("wf-one"), "plan");
  await assert.rejects(setPhase("wf-one", "plan"), refusal("illegal_transition", 3));
  await assert.rejects(setPhase("wf-one", "pr"), refusal("illegal_transition", 3));
  await setPhase("wf-one", "dispatch");
  await setPhase("wf-one", "pr");
  await setPhase("wf-one", "done");
  assert.equal(await getPhase("wf-one"), "done");
  await assert.rejects(setPhase("wf-one", "plan"), refusal("illegal_transition", 3));
});

test("a workflow.json lead without host (written before tmux) parses as herdr", () => {
  const v0 = {
    v: 1, id: "wf-old", title: "T", repo: "/r", base: "main", leadBranch: "cpd/wf-old", leadWorktree: null, createdAt: "2026-01-01T00:00:00Z", pr: null,
    lead: { paneId: "w1:p1", sessionPath: "/s.jsonl", sessionId: "sid", queueDir: "/q", attachedAt: "2026-01-01T00:00:00Z" },
  };
  assert.equal(WorkflowJson.parse(v0).lead?.host, "herdr");
  assert.equal(WorkflowJson.safeParse({ ...v0, lead: { ...v0.lead, host: "tmux" } }).success, false); // tmux needs tmuxSocket
});
