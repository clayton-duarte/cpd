import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { withLedger } from "../../src/ledger/bd.ts";
import { addJob, type JobSpec } from "../../src/ledger/jobs.ts";
import { recordPlanDecision } from "../../src/ledger/plan.ts";
import { initWorkflow, setPhase } from "../../src/ledger/workflow.ts";
import { beadsDir, ledgerDir } from "../../src/lib/paths.ts";
import type { Catalog } from "../../src/lib/tiers.ts";
import { locatePi } from "../../src/lib/pi-install.ts";
import { runCli, sandboxEnv, tmpDir } from "../helpers/index.ts";

/** Hand-built catalog (no pi needed): O 1-5, S 0-5, H 0-3. */
export const CATALOG: Catalog = {
  O: { model: "p/opus", efforts: [1, 2, 3, 4, 5] },
  S: { model: "p/sonnet", efforts: [0, 1, 2, 3, 4, 5] },
  H: { model: "p/haiku", efforts: [0, 1, 2, 3] },
};

/** Point HOME and CPD_HOME of this test process at a temp dir, so `bd` and cpd state are sandboxed. */
export function sandbox(): { env: NodeJS.ProcessEnv; repo: string } {
  const env = sandboxEnv();
  process.env.HOME = env.HOME!;
  process.env.CPD_HOME = env.CPD_HOME!;
  return { env, repo: makeRepo() };
}

function git(cwd: string, ...args: string[]): void {
  const r = spawnSync("git", args, { cwd, env: { ...process.env, GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t" } });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
}

export function makeRepo(): string {
  const repo = tmpDir("cpd-repo-");
  git(repo, "init", "-q", "-b", "main");
  writeFileSync(join(repo, "README.md"), "hello\n");
  git(repo, "add", ".");
  git(repo, "commit", "-q", "-m", "init");
  return repo;
}

export const spec = (over: Partial<JobSpec> = {}): JobSpec => ({
  goal: "do the thing",
  context: "because",
  touches: ["src/a.ts"],
  acceptance: ["npm test passes"],
  outOfScope: ["everything else"],
  ...over,
});

/** Initialise a Workflow in phase `plan`. */
export async function newPlan(wf: string, repo: string): Promise<void> {
  await initWorkflow({ wf, repo, title: `Workflow ${wf}` });
  await setPhase(wf, "plan");
}

export async function addDraft(wf: string, key: string, needs: string[] = [], tier = "S3") {
  return (await addJob(wf, { key, title: `Job ${key}`, tier, reviewTier: "S4", spec: spec({ goal: `goal of ${key}` }), needs }, CATALOG)).job;
}

export async function approvePlan(wf: string, id = "r1"): Promise<void> {
  await recordPlanDecision(wf, id, "pending");
  await recordPlanDecision(wf, id, "approved");
}

/** Run `bd` for a Workflow as `actor` (a hand-run bd, outside cpd), with the §8 environment otherwise. */
export function rawBd(wf: string, args: string[], actor = "cpd"): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const c = spawn("bd", [...args, "--json"], {
      cwd: ledgerDir(wf),
      env: { ...process.env, BEADS_DIR: beadsDir(wf), BEADS_ACTOR: actor, BD_DISABLE_METRICS: "1", DO_NOT_TRACK: "1", BD_NON_INTERACTIVE: "1" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    c.stdout.setEncoding("utf8").on("data", (d: string) => (stdout += d));
    c.stderr.setEncoding("utf8").on("data", (d: string) => (stderr += d));
    c.on("error", reject);
    c.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

export async function bdJson(wf: string, args: string[], actor = "cpd"): Promise<any> {
  const r = await rawBd(wf, args, actor);
  if (r.code !== 0) throw new Error(`bd ${args.join(" ")} exited ${r.code}: ${r.stdout}${r.stderr}`);
  return JSON.parse(r.stdout);
}

/** Every issue (all types, all statuses), sorted by id: the "nothing was written" probe. */
export async function allIssues(wf: string): Promise<unknown[]> {
  const all: any[] = await bdJson(wf, ["list", "--all", "-n", "0"]);
  return all.sort((a, b) => a.id.localeCompare(b.id));
}

/** Test-only: move a Job queued → running → in_review, then close it as Merged (`bd close` only from in_review). */
export async function mergeJob(wf: string, key: string): Promise<void> {
  const jobs: any[] = await bdJson(wf, ["list", "--all", "-n", "0", "-t", "job"]);
  const job = jobs.find((j) => j.metadata?.key === key);
  if (!job) throw new Error(`no job ${key}`);
  await withLedger(wf, "test merge", async (l) => {
    await l.write(["update", job.id, "--if-status", "queued", "-s", "running"]);
    await l.write(["update", job.id, "--if-status", "running", "-s", "in_review"]);
    await l.write(["close", job.id, "--reason", "merged"]);
  });
}

export async function jobId(wf: string, key: string): Promise<string> {
  const jobs: any[] = await bdJson(wf, ["list", "--all", "-n", "0", "-t", "job"]);
  const job = jobs.find((j) => j.metadata?.key === key);
  if (!job) throw new Error(`no job ${key}`);
  return job.id;
}

/** Test-only: an Attempt issue under a Job (the lifecycle itself belongs to the ledger-attempts Job). */
export async function makeAttempt(wf: string, key: string, number: number, status: "in_progress" | "closed", tier = "S3"): Promise<void> {
  const parent = await jobId(wf, key);
  await withLedger(wf, "test attempt", async (l) => {
    const created: any = await l.write(
      ["create", `--title=${key} attempt ${number}`, "-t", "attempt", "--parent", parent, "-s", "in_progress", "--metadata", JSON.stringify({ number, tier })],
    );
    if (status === "closed") await l.write(["close", created.id, "--reason", "failed"]);
  });
}

export async function setJobStatus(wf: string, key: string, status: string): Promise<void> {
  const id = await jobId(wf, key);
  await withLedger(wf, "test status", (l) => l.write(["update", id, "-s", status]));
}

/** Checksums of every file under `dir` (relative path → sha256), for byte-identical comparisons. */
export function checksums(dir: string): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (d: string) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else out[p.slice(dir.length)] = createHash("sha256").update(readFileSync(p)).digest("hex");
    }
  };
  walk(dir);
  return out;
}

export function gitOut(cwd: string, ...args: string[]): string {
  return spawnSync("git", args, { cwd, encoding: "utf8" }).stdout;
}

const CONFIG = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5,
  prPollSeconds: 120, herdrSession: null,
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
};

/** `sandbox()` plus a config.json and the real pi agent dir (read-only), so the CLI sees pi's real Tier catalog. */
export function cliSandbox(): { env: NodeJS.ProcessEnv; repo: string; cli: (args: string[]) => ReturnType<typeof runCli> } {
  const { env, repo } = sandbox();
  mkdirSync(env.CPD_HOME!, { recursive: true });
  writeFileSync(join(env.CPD_HOME!, "config.json"), JSON.stringify(CONFIG));
  const cliEnv = { ...env, PI_CODING_AGENT_DIR: locatePi().agentDir };
  return { env: cliEnv, repo, cli: (args) => runCli(args, { env: cliEnv }) };
}

export function writeSpec(dir: string, name: string, s: unknown): string {
  const path = join(dir, name);
  writeFileSync(path, typeof s === "string" ? s : JSON.stringify(s));
  return path;
}
