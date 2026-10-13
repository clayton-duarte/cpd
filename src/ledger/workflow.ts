import { existsSync, mkdirSync, readFileSync, realpathSync, renameSync, statSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { parseArgs, type ParseArgsConfig } from "node:util";
import { z } from "zod";
import { failure, notFound, refused, usage } from "../lib/errors.ts";
import { run } from "../lib/exec.ts";
import { withLock } from "../lib/lock.ts";
import {
  assertWorkflowId,
  escalationsDir,
  leadBranch,
  ledgerLockPath,
  logsDir,
  plannotatorDir,
  reviewsDir,
  specsDir,
  wfLocksDir,
  workflowJsonPath,
} from "../lib/paths.ts";
import { createIssue, initLedger, metadataArgs, readLedger, withLedger, workflowIssue, type Ledger, type ReadLedger } from "./bd.ts";
import { logEvent } from "./audit.ts";

// --- workflow.json (contract §6) ---

const Lead = z.strictObject({
  paneId: z.string(),
  sessionPath: z.string(),
  sessionId: z.string(),
  queueDir: z.string(),
  attachedAt: z.string(),
});
const Pr = z.strictObject({ number: z.number().int().positive(), url: z.string(), openedAt: z.string() });

export const WorkflowJson = z.strictObject({
  v: z.literal(1),
  id: z.string(),
  title: z.string(),
  repo: z.string(),
  base: z.string(),
  leadBranch: z.string(),
  leadWorktree: z.string().nullable(),
  createdAt: z.string(),
  lead: Lead.nullable(),
  pr: Pr.nullable(),
});
export type WorkflowJson = z.infer<typeof WorkflowJson>;

/** `--wf <id>` or `CPD_WF`; error `workflow_required` (exit 2). */
export function resolveWorkflowId(flag: string | undefined): string {
  const wf = flag ?? process.env.CPD_WF;
  if (wf === undefined || wf === "") throw usage("workflow_required", "pass --wf <id> or set CPD_WF");
  assertWorkflowId(wf);
  return wf;
}

type StringOptions = Record<string, { type: "string" } | { type: "boolean" }>;

/** `parseArgs` in strict mode; any argument problem is a usage error (exit 2). */
export function parseCommand<const O extends StringOptions>(
  name: string,
  argv: string[],
  options: O,
  maxPositionals = 0,
): { values: ReturnType<typeof parseArgs<{ options: O; strict: true; allowPositionals: true }>>["values"]; positionals: string[] } {
  try {
    const r = parseArgs({ args: argv, options, allowPositionals: true, strict: true } as ParseArgsConfig & { options: O; strict: true; allowPositionals: true });
    if (r.positionals.length > maxPositionals) throw new Error(`unexpected argument: ${r.positionals[maxPositionals]}`);
    return r as never;
  } catch (e) {
    throw usage("usage", `cpd ${name}: ${(e as Error).message}`);
  }
}

/** Write JSON through a temp file and rename. The caller holds the ledger lock. */
export function writeJsonAtomic(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true });
  const tmp = `${path}.${process.pid}.tmp`;
  writeFileSync(tmp, JSON.stringify(value, null, 2) + "\n");
  renameSync(tmp, path);
}

export function readWorkflow(wf: string): WorkflowJson {
  const path = workflowJsonPath(wf);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") throw notFound("workflow_not_found", `workflow ${wf} does not exist`, { wf });
    throw failure("workflow_unreadable", `cannot read ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("workflow_invalid", `${path} is not valid JSON`, { path });
  }
  const parsed = WorkflowJson.safeParse(raw);
  if (!parsed.success) throw failure("workflow_invalid", `${path} does not match the schema`, { path, issues: parsed.error.issues });
  return parsed.data;
}

/** Validate and write `workflow.json` atomically under `locks/ledger.lock`. Do not call while holding the lock. */
export function writeWorkflow(wf: string, value: WorkflowJson): Promise<void> {
  const parsed = WorkflowJson.parse(value);
  return withLock(ledgerLockPath(wf), "write workflow.json", () => writeJsonAtomic(workflowJsonPath(wf), parsed));
}

// --- init ---

async function defaultBase(repo: string): Promise<string> {
  const r = await run("git", ["-C", repo, "symbolic-ref", "--short", "refs/remotes/origin/HEAD"], { cwd: repo, env: process.env });
  const m = r.code === 0 ? /^origin\/(.+)$/.exec(r.stdout.trim()) : null;
  return m ? m[1]! : "main";
}

export type InitInput = { wf: string; repo: string; title: string; base?: string };

/** Contract §11 `workflow init`. Never writes into the product repo. */
export async function initWorkflow(input: InitInput): Promise<WorkflowJson> {
  const { wf } = input;
  assertWorkflowId(wf);
  if (input.title.trim() === "") throw usage("invalid_title", "title must not be empty");
  let repo: string;
  try {
    repo = realpathSync(input.repo);
    if (!statSync(repo).isDirectory()) throw new Error("not a directory");
  } catch {
    throw usage("repo_not_found", `repo path does not exist: ${input.repo}`, { repo: input.repo });
  }
  const git = await run("git", ["-C", repo, "rev-parse", "--git-dir"], { cwd: repo, env: process.env });
  if (git.code !== 0) throw usage("not_a_git_repo", `not a git repository: ${repo}`, { repo });
  if (existsSync(workflowJsonPath(wf))) throw refused("workflow_exists", `workflow ${wf} already exists`, { wf });

  const base = input.base ?? (await defaultBase(repo));
  for (const d of [escalationsDir(wf), specsDir(wf), reviewsDir(wf), plannotatorDir(wf), wfLocksDir(wf), logsDir(wf)]) {
    mkdirSync(d, { recursive: true });
  }
  await initLedger(wf);
  const wfJson: WorkflowJson = {
    v: 1,
    id: wf,
    title: input.title,
    repo,
    base,
    leadBranch: leadBranch(wf),
    leadWorktree: null,
    createdAt: new Date().toISOString(),
    lead: null,
    pr: null,
  };
  await withLedger(wf, "workflow init", async (l) => {
    // Idempotent if an earlier init died after `bd init`: the workflow issue is created once.
    const existing = await l.read(["list", "--all", "-n", "0", "-t", "workflow"], z.array(z.unknown()));
    if (existing.length === 0) {
      await createIssue(l, [input.title, "-t", "workflow", "--metadata", JSON.stringify({ phase: "context" })]);
    }
    if (existsSync(workflowJsonPath(wf))) throw refused("workflow_exists", `workflow ${wf} already exists`, { wf });
    writeJsonAtomic(workflowJsonPath(wf), WorkflowJson.parse(wfJson));
  });
  logEvent(wf, "info", "workflow_init", { repo, base });
  return wfJson;
}

// --- phase (contract §11) ---

export const PHASES = ["context", "plan", "dispatch", "pr", "done"] as const;
export type Phase = (typeof PHASES)[number];
const Phase = z.enum(PHASES);

/** The only legal transitions. `context→plan` is the `workflow phase plan` command; the rest are driven by `plan lock`, `pr open`, `pr poll`. */
const NEXT: Readonly<Partial<Record<Phase, Phase>>> = { context: "plan", plan: "dispatch", dispatch: "pr", pr: "done" };

export function parsePhase(v: string): Phase {
  const p = Phase.safeParse(v);
  if (!p.success) throw usage("invalid_phase", `invalid phase ${JSON.stringify(v)}: expected one of ${PHASES.join(", ")}`, { phase: v });
  return p.data;
}

async function phaseOf(l: ReadLedger): Promise<{ phase: Phase; id: string; metadata: Record<string, unknown> }> {
  const issue = await workflowIssue(l);
  const phase = Phase.safeParse(issue.metadata?.phase);
  if (!phase.success) throw failure("ledger_invalid", `workflow issue has no valid phase: ${JSON.stringify(issue.metadata?.phase)}`, { id: issue.id });
  return { phase: phase.data, id: issue.id, metadata: issue.metadata ?? {} };
}

/** Current phase (the Workflow issue's `metadata.phase` is the single source of truth). */
export async function getPhase(wf: string): Promise<Phase> {
  return (await phaseOf(readLedger(wf))).phase;
}

/** Phase of an already-locked handle. */
export async function getPhaseIn(l: ReadLedger): Promise<Phase> {
  return (await phaseOf(l)).phase;
}

/** Move to `to` inside an existing lock; `extra` metadata is written in the same update. */
export async function setPhaseIn(l: Ledger, to: Phase, extra: Record<string, unknown> = {}): Promise<void> {
  const { phase, id, metadata } = await phaseOf(l);
  if (NEXT[phase] !== to) {
    throw refused("illegal_transition", `phase ${phase} cannot move to ${to}`, { from: phase, to });
  }
  await l.write(["update", id, ...metadataArgs(metadata, { ...extra, phase: to })]);
  logEvent(l.wf, "info", "phase", { from: phase, to });
}

/**
 * Legal transitions only (context→plan→dispatch→pr→done). Exported for the Jobs that own the
 * later transitions: `plan lock` (→dispatch), `pr open` (→pr), `pr poll` (→done).
 */
export function setPhase(wf: string, to: Phase): Promise<void> {
  return withLedger(wf, `phase ${to}`, (l) => setPhaseIn(l, to));
}
