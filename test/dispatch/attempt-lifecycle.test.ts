import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { createLeadBranch } from "../../src/git/lead-branch.ts";
import { gitOut } from "../../src/git/git.ts";
import { attemptProvenance, getAttempt, listAttempts } from "../../src/ledger/attempts.ts";
import { addJob, getJobByKey } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { initWorkflow, readWorkflow, setPhase, writeWorkflow } from "../../src/ledger/workflow.ts";
import { attemptLogPath, cpdHome, reviewBriefPath, reviewDiffPath, reviewGatePath, slotPath } from "../../src/lib/paths.ts";
import { locatePi } from "../../src/lib/pi-install.ts";
import { CLI, runCli, tmpDir } from "../helpers/index.ts";
import { createRepo, sandbox } from "../git/fixtures.ts";
import { approvePlan, CATALOG, spec } from "../ledger/helpers.ts";
import { fakeHerdrCalls, installFakeHerdr, type Scenario } from "./fake-herdr.ts";

/**
 * The whole lifecycle against the REAL Ledger (bd), git, wt, slots, queue and CLI; only herdr and pi are scripted
 * (test/dispatch/fake-herdr.ts). Jobs are dispatched with `cpd job dispatch`, which spawns the detached `attempt run`.
 */
sandbox();
process.env.PI_CODING_AGENT_DIR = locatePi().agentDir;

const WF = "life";
const STATE = tmpDir("cpd-fake-herdr-");
const BIN = tmpDir("cpd-fake-bin-");
const LEAD_ID = "lead1";
const SCENARIO: Scenario = {
  "api-a1": { worker: ["submit"], reviewer: ["approve"] },
  "rej-a1": { worker: ["submit"], reviewer: ["reject"] },
  "rej-a2": { worker: ["submit"], reviewer: ["approve"] },
  "rv-a1": { worker: ["submit"], reviewer: ["garbage", "garbage"] },
  "cr-a1": { worker: ["crash"], reviewer: [] },
  "st-a1": { worker: [], reviewer: [], startFails: true },
};
installFakeHerdr(BIN, STATE, SCENARIO);

const env: NodeJS.ProcessEnv = {
  ...process.env,
  PATH: `${BIN}:${process.env.PATH}`,
  FAKE_HERDR_DIR: STATE,
  FAKE_CPD: CLI,
  FAKE_WF: WF,
};
const cli = (args: string[]) => runCli(args, { env });

let repo = "";
let leadWorktree = "";

const queueDir = () => join(cpdHome(), "leads", LEAD_ID, "queue");
function messages(): { kind: string; job?: string; attempt?: string; text: string; data: Record<string, any> }[] {
  if (!existsSync(queueDir())) return [];
  return readdirSync(queueDir())
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => JSON.parse(readFileSync(join(queueDir(), f), "utf8")));
}
const kindsFor = (job: string) => messages().filter((m) => m.job === job).map((m) => m.kind);

async function waitFor(what: string, fn: () => Promise<boolean> | boolean, ms = 240_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (!(await fn())) {
    if (Date.now() > deadline) {
      const log = (id: string) => (existsSync(attemptLogPath(WF, id)) ? readFileSync(attemptLogPath(WF, id), "utf8") : "(no log)");
      assert.fail(`timed out waiting for ${what}\n${Object.keys(SCENARIO).map((id) => `--- ${id}\n${log(id)}`).join("\n")}`);
    }
    await sleep(500);
  }
}
const jobIs = (key: string, status: string) => async () => (await getJobByKey(WF, key)).status === status;
const slotsFree = () => [0, 1, 2].every((i) => !existsSync(slotPath(i)));

test("setup: a Workflow with a Lead Branch worktree, an attached (fake) Lead and five queued Jobs", { timeout: 240_000 }, async () => {
  mkdirSync(cpdHome(), { recursive: true });
  writeFileSync(
    join(cpdHome(), "config.json"),
    JSON.stringify({
      version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5, prPollSeconds: 120,
      herdrSession: "fake-never-the-default",
      tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
      leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
    }),
  );
  repo = await createRepo({ preStart: "true" });
  await initWorkflow({ wf: WF, repo, title: "Lifecycle" });
  await setPhase(WF, "plan");
  for (const key of ["api", "rej", "rv", "cr", "st"]) {
    await addJob(WF, { key, title: `Job ${key}`, tier: "H1", reviewTier: "H1", spec: spec({ goal: `goal of ${key}`, touches: [`feature-${key}-a1.txt`] }), needs: [] }, CATALOG);
  }
  await approvePlan(WF);
  await lockPlan(WF);
  leadWorktree = (await createLeadBranch(repo, `cpd/${WF}`, "main")).path;
  const sessionPath = join(tmpDir("cpd-lead-session-"), "lead.jsonl");
  writeFileSync(sessionPath, JSON.stringify({ type: "session", version: 3, id: "lead-session-1", timestamp: new Date().toISOString(), cwd: repo }) + "\n");
  const w = readWorkflow(WF);
  await writeWorkflow(WF, { ...w, leadWorktree, lead: { paneId: "w1:p1", sessionPath, sessionId: LEAD_ID, queueDir: queueDir(), attachedAt: new Date().toISOString() } });
  assert.equal((await getJobByKey(WF, "api")).status, "queued");
});

test("Merged: dispatch → Worker commits and submits → Reviewer approves → merge → Job closed, worktree removed, job-merged", { timeout: 400_000 }, async () => {
  const d = await cli(["job", "dispatch", "api", "--wf", WF]);
  assert.equal(d.code, 0, d.stdout + d.stderr);
  assert.equal(d.json.attempt, "api-a1");
  assert.equal(d.json.tier, "H1");
  assert.equal(typeof d.json.pid, "number");
  await waitFor("Job api closed", jobIs("api", "closed"));
  await waitFor("slot released", slotsFree);

  const job = await getJobByKey(WF, "api");
  const a = await getAttempt(WF, "api-a1");
  assert.deepEqual([a.status, a.outcome, a.tier], ["closed", "approved", "H1"]);
  assert.equal(job.closeReason, "merged");
  const head = await gitOut(leadWorktree, ["rev-parse", `cpd/${WF}`]);
  assert.equal(job.metadata.merged_sha, head);
  assert.match(await gitOut(leadWorktree, ["log", "--format=%an|%s", `cpd/${WF}`]), /Worker\|feat: api-a1/);
  assert.equal(readFileSync(join(leadWorktree, "feature-api-a1.txt"), "utf8"), `made by ${a.agent}\n`);

  // Facts, metrics and provenance as the contract says.
  assert.equal(a.branch, `cpd/${WF}--api-a1`);
  assert.equal(typeof a.worktree, "string");
  assert.equal(typeof a.metadata.worker_pane, "string");
  assert.deepEqual(a.metadata.argv, ["pi", "--approve", "--model", "github-copilot/claude-haiku-4.5:low"]);
  assert.equal(a.metadata.metrics_turns, 1);
  assert.equal(a.metadata.metrics_tokens_in, 10);
  assert.equal(a.metadata.review_metrics_turns, 1);
  assert.deepEqual((await attemptProvenance(WF, "api-a1")).map((p) => p.kind).sort(), ["commit", "cut", "land"]);

  // Review artefacts exist, the Reviewer was forked from the Lead and read-only, then worktree and branch are gone.
  for (const p of [reviewDiffPath(WF, "api-a1"), reviewGatePath(WF, "api-a1"), reviewBriefPath(WF, "api-a1")]) assert.ok(existsSync(p), p);
  assert.match(readFileSync(reviewDiffPath(WF, "api-a1"), "utf8"), /feature-api-a1\.txt/);
  const starts = fakeHerdrCalls(STATE).filter((c) => c[0] === "agent" && c[1] === "start");
  const reviewer = starts.find((c) => c.includes("--tools"))!;
  assert.deepEqual(reviewer.slice(reviewer.indexOf("--") + 1).slice(0, 2), ["--approve", "--fork"]);
  assert.ok(reviewer.includes("read,grep,find,ls"));
  assert.equal(existsSync(a.worktree!), false, "the Attempt worktree was removed");
  assert.equal(await gitOut(leadWorktree, ["branch", "--list", `cpd/${WF}--api-a1`]), "");

  assert.deepEqual(kindsFor("api"), ["job-merged"]);
  const log = readFileSync(attemptLogPath(WF, "api-a1"), "utf8");
  for (const step of ["1 load", "2 worktree", "3 spec", "4 placement", "5 start", "6 prompt", "7 decide", "9 review", "11 merge", "12 merged"]) {
    assert.ok(log.includes(`[${step}]`), `log has step ${step}`);
  }
});

test("Reviewer rejects → Attempt closed rejected, Job requeued, attempt-retry; the next dispatch starts Attempt 2 one Tier up and merges", { timeout: 600_000 }, async () => {
  const d1 = await cli(["job", "dispatch", "rej", "--wf", WF]);
  assert.equal(d1.code, 0, d1.stdout + d1.stderr);
  await waitFor("Job rej requeued", jobIs("rej", "queued"));
  await waitFor("slot released", slotsFree);
  const a1 = await getAttempt(WF, "rej-a1");
  assert.deepEqual([a1.status, a1.outcome, a1.cause], ["closed", "rejected", "review_rejected"]);
  const retry = messages().find((m) => m.job === "rej" && m.kind === "attempt-retry")!;
  assert.equal(retry.data.nextTier, "H2");
  assert.match(retry.text, /fake reviewer: please fix the thing/);
  assert.ok(existsSync(a1.worktree!), "a rejected Attempt keeps its worktree");

  const d2 = await cli(["job", "dispatch", "rej", "--wf", WF]);
  assert.equal(d2.code, 0, d2.stdout + d2.stderr);
  assert.deepEqual([d2.json.attempt, d2.json.tier], ["rej-a2", "H2"]);
  await waitFor("Job rej closed", jobIs("rej", "closed"));
  await waitFor("slot released", slotsFree); // the slot goes back last, after the Lead was told
  const attempts = await listAttempts(WF);
  assert.deepEqual(attempts.filter((a) => a.job === "rej").map((a) => [a.id, a.tier, a.outcome]), [["rej-a1", "H1", "rejected"], ["rej-a2", "H2", "approved"]]);
  // The Worker Spec of Attempt 2 shows what happened to Attempt 1.
  const specText = readFileSync(join(cpdHome(), "workflows", WF, "specs", "rej-a2.md"), "utf8");
  assert.match(specText, /Outcome: rejected/);
  assert.match(specText, /fake reviewer: please fix the thing/);
  assert.deepEqual(kindsFor("rej"), ["attempt-retry", "job-merged"]);
});

test("verdict invalid twice → review-needed (slot freed, Attempt waits) → `cpd review decide --approve` merges", { timeout: 600_000 }, async () => {
  const d = await cli(["job", "dispatch", "rv", "--wf", WF]);
  assert.equal(d.code, 0, d.stdout + d.stderr);
  await waitFor("review-needed", () => kindsFor("rv").includes("review-needed"));
  await waitFor("slot released", slotsFree);
  const waiting = await getAttempt(WF, "rv-a1");
  assert.deepEqual([waiting.status, waiting.outcome], ["in_review", null]);
  assert.equal(typeof waiting.metadata.review_needed_at, "string");
  assert.equal((await getJobByKey(WF, "rv")).status, "in_review");
  const needed = messages().find((m) => m.kind === "review-needed")!;
  assert.match(needed.text, /cpd review decide rv-a1 --approve/);

  const refused = await cli(["review", "decide", "api-a1", "--approve", "--feedback", "x", "--wf", WF]);
  assert.deepEqual([refused.code, refused.json.error.code], [3, "not_awaiting_review"]);
  const usage = await cli(["review", "decide", "rv-a1", "--feedback", "x", "--wf", WF]);
  assert.equal(usage.code, 2);

  const r = await cli(["review", "decide", "rv-a1", "--approve", "--feedback", "Lead read the diff: fine", "--wf", WF]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.json.state, "merged");
  const done = await getAttempt(WF, "rv-a1");
  assert.deepEqual([done.status, done.outcome], ["closed", "approved"]);
  assert.equal((await getJobByKey(WF, "rv")).status, "closed");
  assert.deepEqual(kindsFor("rv"), ["review-needed", "job-merged"]);
  const again = await cli(["review", "decide", "rv-a1", "--reject", "--feedback", "late", "--wf", WF]);
  assert.deepEqual([again.code, again.json.error.code], [3, "not_awaiting_review"]);
});

test("Worker crash → ladder (Job requeued, attempt-retry); nothing is merged; the Worker pane is closed", { timeout: 400_000 }, async () => {
  const d = await cli(["job", "dispatch", "cr", "--wf", WF]);
  assert.equal(d.code, 0, d.stdout + d.stderr);
  await waitFor("Job cr requeued", jobIs("cr", "queued"));
  await waitFor("slot released", slotsFree);
  const a = await getAttempt(WF, "cr-a1");
  assert.deepEqual([a.status, a.outcome, a.cause], ["closed", "failed", "crashed"]);
  assert.deepEqual(kindsFor("cr"), ["attempt-retry"]);
  assert.ok(fakeHerdrCalls(STATE).some((c) => c[0] === "pane" && c[1] === "close" && c[2] === a.metadata.worker_pane));
  assert.ok(existsSync(a.worktree!), "the worktree is kept");
});

test("the Worker cannot start → job-failed worker_start_failed (no ladder), slot freed", { timeout: 400_000 }, async () => {
  const d = await cli(["job", "dispatch", "st", "--wf", WF]);
  assert.equal(d.code, 0, d.stdout + d.stderr);
  await waitFor("Job st failed", jobIs("st", "failed"));
  await waitFor("slot released", slotsFree);
  const a = await getAttempt(WF, "st-a1");
  assert.deepEqual([a.outcome, a.cause], ["failed", "worker_start_failed"]);
  assert.deepEqual(kindsFor("st"), ["job-failed"]);
  const m = messages().find((x) => x.job === "st")!;
  assert.match(m.text, /cpd job retry st --tier <T> --wf life/);
});

test("job dispatch of a Job that is not queued is cas_lost and keeps no slot", { timeout: 200_000 }, async () => {
  const r = await cli(["job", "dispatch", "api", "--wf", WF]);
  assert.equal(r.code, 3, r.stdout);
  assert.equal(r.json.error.code, "cas_lost");
  assert.equal(slotsFree(), true);
});

test("the Lead worktree is clean and the Lead Branch history is linear after all merges", async () => {
  assert.equal(await gitOut(leadWorktree, ["status", "--porcelain"]), "");
  assert.equal(await gitOut(leadWorktree, ["rev-list", "--merges", `cpd/${WF}`]), "");
  const subjects = (await gitOut(leadWorktree, ["log", "--format=%s", `cpd/${WF}`])).split("\n");
  for (const s of ["feat: api-a1", "feat: rej-a2", "feat: rv-a1"]) assert.ok(subjects.includes(s), s);
});
