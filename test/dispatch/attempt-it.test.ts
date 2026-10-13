import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { createLeadBranch } from "../../src/git/lead-branch.ts";
import { gitOut } from "../../src/git/git.ts";
import { closeWorkspace, rootPaneOfWorkspace } from "../../src/herdr/panes.ts";
import { readSessionHeader } from "../../src/herdr/session.ts";
import { herdr } from "../../src/herdr/herdr.ts";
import { workspaceForWorktree } from "../../src/herdr/worktrees.ts";
import { attemptProvenance, getAttempt } from "../../src/ledger/attempts.ts";
import { addJob, getJobByKey } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { initWorkflow, readWorkflow, setPhase, writeWorkflow } from "../../src/ledger/workflow.ts";
import { attemptLogPath, cpdHome, reviewBriefPath, reviewDiffPath, reviewGatePath, slotPath } from "../../src/lib/paths.ts";
import { CLI, runCli, tmpDir } from "../helpers/index.ts";
import { createRepo } from "../git/fixtures.ts";
import { approvePlan, CATALOG, spec } from "../ledger/helpers.ts";
import { z } from "zod";

/**
 * CPD_IT=1 only. One Attempt to Merged with REAL herdr (session `cpd-test`, never the default), real pi (Worker and
 * Reviewer both `github-copilot/claude-haiku-4.5:low`), real bd, git and wt. Needs `herdr --session cpd-test server`
 * running. HOME and XDG_CONFIG_HOME stay real (herdr finds its socket there); CPD_HOME is a temp dir.
 * The Worker's pane has the server's environment, so its Spec context tells it how to run `cpd` against this CPD_HOME.
 * Set CPD_IT_TRANSCRIPT=<file> to get a transcript of the run.
 */
const IT = process.env.CPD_IT === "1";
const WF = "itrun";

test("CPD_IT: one Attempt runs to Merged (real herdr cpd-test, real pi haiku:low Worker and Reviewer)", { skip: !IT && "set CPD_IT=1 (needs a herdr cpd-test server and a pi model)", timeout: 1_200_000 }, async (t) => {
  const root = realpathSync(tmpDir("cpd-it-attempt-"));
  process.env.CPD_HOME = join(root, "cpd");
  mkdirSync(cpdHome(), { recursive: true });
  const transcript: string[] = [];
  const say = (s: string) => {
    transcript.push(s);
    process.stderr.write(`# ${s}\n`);
  };
  writeFileSync(
    join(cpdHome(), "config.json"),
    JSON.stringify({
      version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5, prPollSeconds: 120,
      herdrSession: "cpd-test",
      tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
      leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
    }),
  );

  // A toy repo with the CPD Gate wiring, a Workflow with one Job, a Lead Branch worktree.
  const repo = await createRepo({ preStart: "true" });
  // `herdr worktree open` also opens a workspace for the repo itself. It is closed last (after the linked ones).
  t.after(async () => {
    const ws = await workspaceForWorktree(repo, repo).catch(() => null);
    if (ws !== null) await closeWorkspace(ws).catch(() => {});
  });
  await initWorkflow({ wf: WF, repo, title: "IT run" });
  await setPhase(WF, "plan");
  const cpdCmd = `CPD_HOME=${cpdHome()} ${process.execPath} ${CLI}`;
  await addJob(
    WF,
    {
      key: "hello",
      title: "Create hello.txt",
      tier: "H1",
      reviewTier: "H1",
      needs: [],
      spec: spec({
        goal: "Create the file hello.txt in the repository root containing exactly the word hello followed by a newline.",
        context:
          `ENVIRONMENT NOTE: in this environment the command \`cpd\` is not on PATH. Wherever this Spec says \`cpd <args>\`, run \`${cpdCmd} <args>\` instead.\n\n` +
          "Keep it simple: one new file, one commit.",
        touches: ["hello.txt"],
        acceptance: ["`hello.txt` exists in the repository root and its content is exactly `hello` plus a newline", "the change is committed"],
        outOfScope: ["any other file"],
      }),
    },
    CATALOG,
  );
  await approvePlan(WF);
  await lockPlan(WF);
  const leadWorktree = (await createLeadBranch(repo, `cpd/${WF}`, "main")).path;

  // The Lead: a real (tiny) pi session file to fork, and a plain pane to split Reviewer panes from.
  const sessionPath = join(root, "lead-session.jsonl");
  const lead = spawnSync(
    "pi",
    ["--session", sessionPath, "-p", "--no-tools", "--no-extensions", "--no-context-files", "--model", "github-copilot/claude-haiku-4.5:off", "Reply with the single word OK."],
    { cwd: leadWorktree, encoding: "utf8", timeout: 120_000 },
  );
  assert.equal(lead.status, 0, lead.stderr);
  assert.ok(existsSync(sessionPath), "pi wrote the Lead session file");
  const Created = z.looseObject({ workspace: z.looseObject({ workspace_id: z.string() }) });
  const leadWs = (await herdr(["workspace", "create", "--cwd", leadWorktree, "--label", "cpd it lead", "--no-focus"], Created)).workspace.workspace_id;
  t.after(() => closeWorkspace(leadWs).catch(() => {}));
  const leadPane = await rootPaneOfWorkspace(leadWs);
  const queueDir = join(cpdHome(), "leads", "itlead", "queue");
  const w = readWorkflow(WF);
  await writeWorkflow(WF, { ...w, leadWorktree, lead: { paneId: leadPane, sessionPath, sessionId: "itlead", queueDir, attachedAt: new Date().toISOString() } });
  say(`repo ${repo}; Lead worktree ${leadWorktree}; Lead pane ${leadPane}; Lead session ${sessionPath} (cwd ${readSessionHeader(sessionPath).cwd})`);

  const d = await runCli(["job", "dispatch", "hello", "--wf", WF], { env: { ...process.env, CPD_HOME: cpdHome() } });
  assert.equal(d.code, 0, d.stdout + d.stderr);
  say(`cpd job dispatch hello --wf ${WF} → ${d.stdout.trim()}`);
  t.after(async () => {
    // The Attempt workspace (if a failed run left it open) must not outlive the test.
    const a = await getAttempt(WF, "hello-a1").catch(() => null);
    const ws = a && typeof a.metadata.workspace === "string" ? a.metadata.workspace : null;
    if (ws !== null) await closeWorkspace(ws).catch(() => {});
  });

  const deadline = Date.now() + 1_000_000;
  let status = "";
  while (Date.now() < deadline) {
    status = (await getJobByKey(WF, "hello")).status;
    if (status === "closed" || status === "failed" || status === "queued" || status === "in_review") {
      // `in_review` also means "still being reviewed": wait for the Attempt to end, i.e. the slot to go back.
      if (!existsSync(slotPath(0))) break;
    }
    await sleep(2000);
  }
  const log = existsSync(attemptLogPath(WF, "hello-a1")) ? readFileSync(attemptLogPath(WF, "hello-a1"), "utf8") : "(no attempt log)";
  say(`attempt log:\n${log}`);
  const a = await getAttempt(WF, "hello-a1");
  say(`Job status ${status}; Attempt ${a.status} outcome=${a.outcome} cause=${a.cause}`);
  say(`Attempt metrics/facts: ${JSON.stringify(Object.fromEntries(Object.entries(a.metadata).filter(([k]) => /^(metrics_|review_metrics_|argv|agent|session_path|worker_pane|workspace|worktree|branch)/.test(k))))}`);
  const msgs = existsSync(queueDir) ? readdirSync(queueDir).filter((f) => f.endsWith(".json")).map((f) => JSON.parse(readFileSync(join(queueDir, f), "utf8"))) : [];
  say(`Lead queue: ${msgs.map((m: any) => `${m.kind}: ${m.text}`).join("\n  ")}`);
  if (process.env.CPD_IT_TRANSCRIPT) appendFileSync(process.env.CPD_IT_TRANSCRIPT, transcript.join("\n\n") + "\n");

  assert.equal(status, "closed", `the Job did not end Merged:\n${log}`);
  assert.deepEqual([a.status, a.outcome], ["closed", "approved"]);
  const merged = (await getJobByKey(WF, "hello")).metadata.merged_sha;
  assert.equal(merged, await gitOut(leadWorktree, ["rev-parse", `cpd/${WF}`]));
  assert.equal(readFileSync(join(leadWorktree, "hello.txt"), "utf8"), "hello\n");
  assert.deepEqual(a.metadata.argv, ["pi", "--approve", "--no-skills", "--exclude-tools", "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox", "--model", "github-copilot/claude-haiku-4.5:low"]);
  for (const k of ["metrics_turns", "metrics_tokens_in", "metrics_tokens_out", "metrics_cost_usd", "metrics_duration_s", "review_metrics_turns", "review_metrics_cost_usd"]) {
    assert.equal(typeof a.metadata[k], "number", k);
  }
  assert.ok((a.metadata.metrics_turns as number) >= 1 && (a.metadata.review_metrics_turns as number) >= 1);
  assert.equal(a.metadata.metrics_thinking_level, "low");
  assert.deepEqual((await attemptProvenance(WF, "hello-a1")).map((p) => p.kind).sort(), ["commit", "cut", "land"]);
  for (const p of [reviewDiffPath(WF, "hello-a1"), reviewGatePath(WF, "hello-a1"), reviewBriefPath(WF, "hello-a1")]) assert.ok(existsSync(p), p);
  assert.ok(msgs.some((m: any) => m.kind === "job-merged"));
  assert.equal(existsSync(a.worktree!), false, "the Attempt worktree was removed after the merge");
  const { workspaces } = await herdr(["workspace", "list"], z.looseObject({ workspaces: z.array(z.looseObject({ workspace_id: z.string() })) }));
  assert.equal(workspaces.some((x) => x.workspace_id === a.metadata.workspace), false, "the Attempt workspace was closed");
});
