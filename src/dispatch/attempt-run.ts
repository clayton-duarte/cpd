import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, appendFileSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { z } from "zod";
import { createAttemptWorktree, removeAttemptWorktree } from "../git/attempt-worktree.ts";
import { git } from "../git/git.ts";
import { mergeAttempt, type MergeOutcome } from "../git/merge.ts";
import { agentGet, promptAgent, reviewerArgs, startAgent, workerArgs, type Agent, type StartAgent } from "../herdr/agents.ts";
import { closePane, closeWorkspace, rootPaneOfWorkspace, splitPane } from "../herdr/panes.ts";
import { lastAssistant, readSessionHeader } from "../herdr/session.ts";
import { openWorktree } from "../herdr/worktrees.ts";
import {
  failAttempt,
  findAttempt,
  markMerged,
  recordAttemptFacts,
  recordVerdict,
  type Attempt,
  type AttemptFacts,
  type VerdictInput,
} from "../ledger/attempts.ts";
import { readLedger } from "../ledger/bd.ts";
import { verifyJob } from "../ledger/fingerprint.ts";
import type { Job } from "../ledger/jobs.ts";
import { recordMetrics, type MetricsPrefix } from "../ledger/metrics.ts";
import { readWorkflow } from "../ledger/workflow.ts";
import { loadConfig, type Config } from "../lib/config.ts";
import { locatePi } from "../lib/pi-install.ts";
import { CpdError, failure, refused } from "../lib/errors.ts";
import { agentName, attemptBranch, attemptLogPath, logsDir, reviewDiffPath, reviewGatePath } from "../lib/paths.ts";
import { releaseSlot } from "../lib/slots.ts";
import { loadCatalog, piModelArg, type Catalog } from "../lib/tiers.ts";
import { sessionMetrics, type SessionMetrics } from "../metrics/session.ts";
import { notifyLead, type NotifyInput } from "../queue/lead-queue.ts";
import { renderReviewBrief, type ReviewInputs } from "../render/review.ts";
import { renderWorkerSpec } from "../render/spec.ts";
import { writeTextAtomic } from "../render/template.ts";
import { killWindow, newWindow, sendKeys, setWindowOption, shellQuote } from "../tmux/tmux.ts";
import { tierForAttempt, automaticAttemptCount } from "./dispatch-one.ts";
import { parseVerdict, type ParsedVerdict } from "./verdict.ts";

// Contract §12: one Attempt, end to end. Each step is a small function over injected ports (`Deps`), so the whole
// lifecycle runs against fakes in tests; `realDeps` wires the real modules for `cpd attempt run` and `cpd review decide`.

// --- ports ---

export type LoadedAttempt = { attempt: Attempt; job: Job; siblings: Attempt[] };

export type LedgerPort = {
  /** The Attempt, its Job and the Job's Attempts, freshly read. */
  load(): Promise<LoadedAttempt>;
  verifyJob(): Promise<void>;
  recordFacts(facts: AttemptFacts): Promise<unknown>;
  recordVerdict(v: VerdictInput): Promise<unknown>;
  fail(cause: string, opts: { requeue: boolean }): Promise<unknown>;
  /** `reviewed: false` (`reviewMode: "none"`): the Ledger records the merge with outcome `merged`, not a Reviewer verdict. */
  markMerged(sha: string, opts: { reviewed: boolean }): Promise<unknown>;
  recordMetrics(m: SessionMetrics, prefix: MetricsPrefix): Promise<void>;
};
export type GitPort = {
  createWorktree(branch: string): Promise<{ path: string }>;
  removeWorktree(branch: string): Promise<void>;
  /** `git diff <lead>...<branch>` */
  diff(branch: string): Promise<string>;
  merge(attemptWorktree: string, branch: string): Promise<MergeOutcome>;
};
export type HerdrPort = {
  openWorktree(o: { repo: string; path: string; label: string }): Promise<string>;
  rootPane(workspaceId: string): Promise<string>;
  /** A new pane split from `target` with working directory `cwd`, never focused. */
  split(target: string, cwd: string): Promise<string>;
  startAgent(o: StartAgent): Promise<{ argv: string[]; sessionPath: string }>;
  promptAgent(name: string, text: string, o: { timeoutMs?: number }): Promise<unknown>;
  agentGet(name: string): Promise<Pick<Agent, "pane_id" | "agent_status"> | null>;
  closePane(id: string): Promise<void>;
  closeWorkspace(id: string): Promise<void>;
};
/** `tmux -S <socket> …` calls on the Lead's tmux server (contract §12 step 9, for a tmux Lead). */
export type TmuxPort = {
  newWindow(socket: string, o: { name: string; cwd: string; command: string }): Promise<string>;
  setWindowOption(socket: string, pane: string, option: string, value: string): Promise<void>;
  sendKeys(socket: string, pane: string, keys: string[], literal?: boolean): Promise<void>;
  killWindow(socket: string, pane: string): Promise<void>;
};
/** The newest `message` entry of a session: who wrote it, how it stopped, when it was written, and when the file last changed. */
export type SessionTail = { role: string; stopReason: string | null; writtenMs: number; mtimeMs: number };
export type SessionPort = {
  lastAssistant(path: string): { stopReason: string; text: string } | null;
  /** The session file with this id (`<pi agent dir>/sessions/<dir>/<time>_<id>.jsonl`), or null while pi has not created it. */
  findById(id: string): string | null;
  /** Null while the session has no `message` entry. */
  tail(path: string): SessionTail | null;
  cwdOf(path: string): string;
  /** Metrics of the entries written at or after `sinceMs` (a forked Reviewer session starts with the Lead's history). */
  metrics(path: string, sinceMs?: number): SessionMetrics;
};
export type QueuePort = { notify(input: NotifyInput): unknown };
export type RenderPort = {
  workerSpec(): Promise<string>;
  reviewBrief(inputs: ReviewInputs): Promise<string>;
};
export type Clock = { now(): number; sleep(ms: number): Promise<void> };

export type Deps = {
  wf: string;
  attemptId: string;
  config: Pick<Config, "retryCap" | "reviewMode" | "attemptTimeoutMinutes">;
  catalog: Catalog;
  workflow: { repo: string; leadWorktree: string; leadBranch: string; lead: { sessionPath: string } & ({ host: "herdr" } | { host: "tmux"; tmuxSocket: string }) };
  ledger: LedgerPort;
  git: GitPort;
  herdr: HerdrPort;
  tmux: TmuxPort;
  session: SessionPort;
  queue: QueuePort;
  render: RenderPort;
  clock: Clock;
  /** The slot `cpd watch` acquired for this run; released on every exit path. `null` for `review decide`, which holds none. */
  slot: { release(): Promise<unknown> } | null;
  log(step: string, message: string, data?: Record<string, unknown>): void;
  /** How long a merge may keep waiting for the per-repo merge lock. Default `MERGE_LOCK_WAIT_MS`. */
  mergeLockWaitMs?: number;
};

export type RunResult =
  | { state: "merged"; sha: string }
  | { state: "retry_queued"; cause: string; nextAttempt: string; nextTier: string }
  | { state: "job_failed"; cause: string }
  | { state: "review_needed" }
  | { state: "closed_externally"; outcome: string | null };

/** §12 step 11 (Stage 2 note): `lock_timeout` from `mergeAttempt` means another merge is running; give up after this long in total. */
export const MERGE_LOCK_WAIT_MS = 90 * 60_000;
/**
 * `cpd watch` closes a Worker's pane for a timeout or a cancel and records why a moment later, so a vanished Worker is
 * looked at twice, this far apart, before it is called crashed.
 */
export const CRASH_SETTLE_MS = 5_000;
/** `herdr agent start` readiness timeout (§12 steps 5 and 9). */
const START_TIMEOUT_MS = 60_000;
const FEEDBACK_IN_MESSAGE = 600;
/** A tmux Reviewer's turn is over when its session file has been quiet this long (§12 step 9). */
export const REVIEWER_QUIET_MS = 5_000;
const REVIEWER_POLL_MS = 1_000;

// --- small helpers ---

const msg = (e: unknown): string => (e instanceof Error ? e.message : String(e));
const clip = (s: string, n = FEEDBACK_IN_MESSAGE): string => (s.length > n ? `${s.slice(0, n)}…` : s);
const isoAt = (d: Deps): string => new Date(d.clock.now()).toISOString();
const asString = (v: unknown): string | null => (typeof v === "string" ? v : null);

/** Cleanup that must not stop the lifecycle: a failure is logged, never silent. Only cpd's own errors are tolerated. */
async function cleanup(d: Deps, step: string, what: string, fn: () => Promise<unknown>): Promise<string | null> {
  try {
    await fn();
    return null;
  } catch (e) {
    if (!(e instanceof CpdError)) throw e;
    d.log(step, `cleanup failed: ${what}: ${e.code}: ${e.message}`, { code: e.code });
    return `${what}: ${e.message}`;
  }
}

/** `herdr agent prompt --wait`. A herdr error is logged and the caller decides from the artefacts (t1 F7). */
async function prompt(d: Deps, step: string, name: string, text: string, timeoutMs?: number): Promise<void> {
  try {
    await d.herdr.promptAgent(name, text, timeoutMs === undefined ? {} : { timeoutMs });
  } catch (e) {
    if (!(e instanceof CpdError) || !e.code.startsWith("herdr_")) throw e;
    d.log(step, `prompt to ${name} failed: ${e.code}: ${e.message}`, { code: e.code });
  }
}

const closeWorkerPane = async (d: Deps, step: string, pane: string | null): Promise<void> => {
  if (pane !== null) await cleanup(d, step, `close Worker pane ${pane}`, () => d.herdr.closePane(pane));
};

// --- Lead messages ---

function notifyRetry(d: Deps, l: LoadedAttempt, cause: string, detail: string, next: { id: string; tier: string }, autoCount: number): void {
  d.queue.notify({
    kind: "attempt-retry",
    job: l.job.key,
    attempt: l.attempt.id,
    text:
      `Attempt ${l.attempt.id} of Job ${l.job.key} did not merge (${cause}): ${clip(detail)} ` +
      `The next Attempt ${next.id} will start at Tier ${next.tier} (automatic Attempt ${autoCount + 1} of ${1 + d.config.retryCap}). ` +
      `Inspect it with: cpd attempt show ${l.attempt.id} --wf ${d.wf}`,
    data: { cause, detail, nextAttempt: next.id, nextTier: next.tier, worktree: l.attempt.worktree },
  });
}

function notifyJobFailed(d: Deps, l: LoadedAttempt, cause: string, detail: string, why: string): void {
  d.queue.notify({
    kind: "job-failed",
    job: l.job.key,
    attempt: l.attempt.id,
    text:
      `Job ${l.job.key} failed on Attempt ${l.attempt.id} (${cause}): ${clip(detail)} ${why} ` +
      `Worktree kept${l.attempt.worktree ? ` at ${l.attempt.worktree}` : ""}. ` +
      `Try again at a Tier you choose with: cpd job retry ${l.job.key} --tier <T> --wf ${d.wf} ` +
      `or give up with: cpd job cancel ${l.job.key} --reason "<why>" --wf ${d.wf}`,
    data: { cause, detail, why, worktree: l.attempt.worktree, branch: l.attempt.branch },
  });
}

// --- step 1: load ---

async function load(d: Deps): Promise<{ l: LoadedAttempt } | { done: RunResult }> {
  d.log("1 load", `Attempt ${d.attemptId} of Workflow ${d.wf}`);
  const l = await d.ledger.load();
  if (l.attempt.status === "closed") {
    d.log("1 load", "the Attempt is already closed: nothing to do", { outcome: l.attempt.outcome });
    return { done: { state: "closed_externally", outcome: l.attempt.outcome } };
  }
  if (l.attempt.status === "in_review") {
    throw refused("attempt_in_review", `Attempt ${d.attemptId} is already past its Worker (in_review); see cpd review decide`, { attempt: d.attemptId });
  }
  try {
    await d.ledger.verifyJob();
  } catch (e) {
    if (e instanceof CpdError && e.code === "locked_job_tampered") {
      return { done: await fail(d, "locked_job_tampered", { ladder: false, detail: e.message }) };
    }
    throw e;
  }
  return { l };
}

// --- steps 2 to 6: place and start the Worker ---

async function worktree(d: Deps): Promise<{ path: string } | { done: RunResult }> {
  const branch = attemptBranch(d.wf, d.attemptId);
  d.log("2 worktree", `creating ${branch} from ${d.workflow.leadBranch} (blocks while setup runs)`);
  let path: string;
  try {
    path = (await d.git.createWorktree(branch)).path;
  } catch (e) {
    if (e instanceof CpdError && e.code === "setup_failed") {
      return { done: await fail(d, "setup_failed", { ladder: false, detail: `${e.message} ${JSON.stringify(e.details)}` }) };
    }
    throw e;
  }
  await d.ledger.recordFacts({ worktree: path }); // the Worker Spec renders `worktree` from the Ledger (step 3)
  d.log("2 worktree", `ready at ${path}`);
  return { path };
}

async function spec(d: Deps): Promise<string> {
  d.log("3 spec", "rendering the Worker Spec");
  const path = await d.render.workerSpec();
  d.log("3 spec", `rendered ${path}`);
  return path;
}

/** Steps 4 and 5. A herdr failure here is a Worker that could not start: no ladder, the Lead decides. */
async function startWorker(d: Deps, l: LoadedAttempt, path: string): Promise<{ name: string } | { done: RunResult }> {
  const name = agentName(d.wf, d.attemptId, "w");
  let pane: string | null = null;
  d.log("4 placement", `opening ${path} in herdr`);
  try {
    const workspace = await d.herdr.openWorktree({ repo: d.workflow.repo, path, label: `${d.wf} ${l.job.key} a${l.attempt.number}` });
    pane = await d.herdr.rootPane(workspace);
    d.log("4 placement", `workspace ${workspace}, root pane ${pane}`);
    const started = await d.herdr.startAgent({
      name,
      kind: "pi",
      pane,
      timeoutMs: START_TIMEOUT_MS,
      args: workerArgs(piModelArgFor(d, l.attempt.tier)),
    });
    await d.ledger.recordFacts({
      branch: attemptBranch(d.wf, d.attemptId),
      agent: name,
      session_path: started.sessionPath,
      argv: started.argv,
      worker_pane: pane,
      workspace,
    });
    d.log("5 start", `Worker ${name} started`, { argv: started.argv, session: started.sessionPath });
    return { name };
  } catch (e) {
    if (!(e instanceof CpdError)) throw e;
    d.log("5 start", `the Worker could not be started: ${e.code}: ${e.message}`);
    await closeWorkerPane(d, "5 start", pane);
    return { done: await fail(d, "worker_start_failed", { ladder: false, detail: `${e.code}: ${e.message}` }) };
  }
}

const piModelArgFor = (d: Deps, tier: string): string => piModelArg(d.catalog, tier);

// --- step 7: decide what happened ---

type Observation =
  | { kind: "closed"; outcome: string | null }
  | { kind: "submitted" }
  | { kind: "timed_out" }
  | { kind: "crashed"; why: string }
  | { kind: "idle" };

/** The last assistant message of the Worker's session, or why there is none (a missing or torn file is evidence of a crash). */
function lastOfWorker(d: Deps, sessionPath: string | null): { stopReason: string } | { missing: string } {
  if (sessionPath === null) return { missing: "no session path was recorded" };
  try {
    return d.session.lastAssistant(sessionPath) ?? { missing: "the session has no assistant message" };
  } catch (e) {
    if (e instanceof CpdError && (e.code === "session_not_found" || e.code === "session_malformed")) return { missing: e.message };
    throw e;
  }
}

/**
 * The step 7 decision table, from the Ledger (`submitted_at`, `timed_out`), the session's last `stopReason` and
 * `agentGet`. Never from herdr's idle status alone (t1 F7). Rows, first match wins:
 * closed by someone else (cancel) · submitted · timed_out · agent gone, or last stopReason not `stop` · idle without submitting.
 */
export async function observeWorker(d: Deps, name: string): Promise<Observation> {
  const { attempt } = await d.ledger.load();
  if (attempt.status === "closed") return { kind: "closed", outcome: attempt.outcome };
  if (attempt.submittedAt !== null) return { kind: "submitted" };
  if (attempt.metadata.timed_out === true) return { kind: "timed_out" };
  const gone = (await d.herdr.agentGet(name)) === null;
  const last = lastOfWorker(d, attempt.sessionPath);
  if (gone) return { kind: "crashed", why: "the Worker agent no longer exists" };
  if ("missing" in last) return { kind: "crashed", why: last.missing };
  if (last.stopReason !== "stop") return { kind: "crashed", why: `the last assistant stopReason is ${last.stopReason}, not stop` };
  return { kind: "idle" };
}

async function decide(d: Deps, name: string): Promise<RunResult | "submitted"> {
  d.log("7 decide", "observing the Worker");
  let o = await observeWorker(d, name);
  d.log("7 decide", `first observation: ${o.kind}`, o);
  if (o.kind === "crashed") {
    await d.clock.sleep(CRASH_SETTLE_MS);
    o = await observeWorker(d, name);
    d.log("7 decide", `observation after ${CRASH_SETTLE_MS} ms: ${o.kind}`, o);
  }
  if (o.kind === "idle") {
    await prompt(
      d,
      "7 decide",
      name,
      `You ended your turn without running \`cpd attempt submit ${d.attemptId} --wf ${d.wf}\`. Finish the Spec and submit, or run \`cpd ask ${d.attemptId} "<question>" --wf ${d.wf}\` if you are blocked.`,
    );
    o = await observeWorker(d, name);
    d.log("7 decide", `observation after the nudge: ${o.kind}`, o);
  }
  if (o.kind === "closed") return { state: "closed_externally", outcome: o.outcome };
  await collectMetrics(d, "metrics", (await d.ledger.load()).attempt.sessionPath);
  switch (o.kind) {
    case "submitted":
      return "submitted";
    case "timed_out":
      return fail(d, "timed_out", { ladder: false, detail: `the Attempt exceeded the ${d.config.attemptTimeoutMinutes}-minute active-time limit` });
    case "crashed":
      return fail(d, "crashed", { ladder: true, detail: o.why });
    case "idle":
      return fail(d, "no_submit", { ladder: true, detail: "the Worker ended its turn twice without running cpd attempt submit" });
  }
}

/** Metrics (§14), recorded once per prefix. A problem with the session file is logged, and never blocks the Attempt. */
async function collectMetrics(d: Deps, prefix: MetricsPrefix, sessionPath: string | null, sinceMs?: number): Promise<void> {
  const step = prefix === "metrics" ? "7 metrics" : "9 review metrics";
  if (sessionPath === null) {
    d.log(step, "no session, no metrics");
    return;
  }
  try {
    await d.ledger.recordMetrics(d.session.metrics(sessionPath, sinceMs), prefix);
    d.log(step, "recorded");
  } catch (e) {
    if (!(e instanceof CpdError) && (e as NodeJS.ErrnoException).code !== "ENOENT") throw e;
    d.log(step, `metrics not recorded: ${msg(e)}`, { code: e instanceof CpdError ? e.code : "ENOENT" });
  }
}

// --- step 8 and 10: failure handling and the ladder ---

/** §7: automatic Attempts are capped at `1 + retryCap` per run (a `job retry` starts a new run) and stop when the ladder ends. */
function nextAutomatic(d: Deps, l: LoadedAttempt): { next: { id: string; tier: string } } | { stop: string } {
  const count = automaticAttemptCount(l.job, l.attempt.number);
  if (count >= 1 + d.config.retryCap) return { stop: `The retry cap is reached (${count} automatic Attempts, retryCap ${d.config.retryCap}).` };
  const tier = tierForAttempt(d.catalog, l.job, l.attempt.number + 1);
  if (tier === null) return { stop: `No higher Tier than ${l.attempt.tier} exists.` };
  return { next: { id: `${l.job.key}-a${l.attempt.number + 1}`, tier: `${tier.letter}${tier.effort}` } };
}

/**
 * Close the Attempt as not merged. With `ladder` and room on the ladder the Job goes back to `queued` and the Lead gets
 * `attempt-retry`; otherwise the Job is `failed` and the Lead gets `job-failed`. The Worker pane is closed, the worktree kept.
 * The caller has already recorded the Worker's metrics.
 */
async function fail(d: Deps, cause: string, o: { ladder: boolean; detail: string }): Promise<RunResult> {
  const l = await d.ledger.load();
  d.log("8 failure", `${cause}: ${o.detail}`, { ladder: o.ladder });
  await closeWorkerPane(d, "8 failure", asString(l.attempt.metadata.worker_pane));
  const next = o.ladder ? nextAutomatic(d, l) : null;
  if (next !== null && "next" in next) {
    await d.ledger.fail(cause, { requeue: true });
    notifyRetry(d, l, cause, o.detail, next.next, automaticAttemptCount(l.job, l.attempt.number));
    d.log("8 failure", `Attempt closed; Job requeued for ${next.next.id} at Tier ${next.next.tier}`);
    return { state: "retry_queued", cause, nextAttempt: next.next.id, nextTier: next.next.tier };
  }
  await d.ledger.fail(cause, { requeue: false });
  notifyJobFailed(d, l, cause, o.detail, next === null ? "This failure is not retried automatically." : next.stop);
  d.log("8 failure", "Attempt closed; Job failed");
  return { state: "job_failed", cause };
}

/** Step 10. */
function rejected(d: Deps, feedback: string): Promise<RunResult> {
  return fail(d, "review_rejected", { ladder: true, detail: `Reviewer feedback: ${feedback}` });
}

// --- step 9: review ---

async function readVerdict(d: Deps, sessionPath: string): Promise<ParsedVerdict> {
  let last: { stopReason: string; text: string } | null;
  try {
    last = d.session.lastAssistant(sessionPath);
  } catch (e) {
    if (e instanceof CpdError && (e.code === "session_not_found" || e.code === "session_malformed")) return { verdict: "invalid", reason: e.message };
    throw e;
  }
  if (last === null) return { verdict: "invalid", reason: "the Reviewer session has no assistant message" };
  if (last.stopReason !== "stop") return { verdict: "invalid", reason: `the Reviewer's last stopReason is ${last.stopReason}, not stop` };
  return parseVerdict(last.text);
}

const REPROMPT = `Your last message did not end with a valid verdict. Reply again and end with exactly one fenced json block {"verdict": "approve" or "reject", "feedback": "<text>"} and nothing after it.`;

/** Run the Reviewer where the Lead runs: a hidden tmux window for a tmux Lead, a herdr pane otherwise. */
function runReviewer(d: Deps, l: LoadedAttempt, brief: string): Promise<ParsedVerdict> {
  const { lead } = d.workflow;
  return lead.host === "tmux" ? runTmuxReviewer(d, l, brief, lead.tmuxSocket) : runHerdrReviewer(d, l, brief);
}

/**
 * Run the Reviewer in a herdr pane split from the Attempt's Worker pane (so, in the Attempt's herdr workspace, whatever the Lead's host), a read-only pi (forked from the Lead session unless `reviewMode` is
 * `fresh`), one re-prompt when the verdict is missing or invalid. A Reviewer that cannot run at all counts as no verdict.
 */
async function runHerdrReviewer(d: Deps, l: LoadedAttempt, brief: string): Promise<ParsedVerdict> {
  const name = agentName(d.wf, d.attemptId, "r");
  const sinceMs = d.clock.now();
  const timeoutMs = d.config.attemptTimeoutMinutes * 60_000;
  let pane: string | null = null;
  let sessionPath: string | null = null;
  try {
    const { lead } = d.workflow;
    const workerPane = asString(l.attempt.metadata.worker_pane);
    if (workerPane === null) throw failure("worker_pane_missing", `Attempt ${d.attemptId} has no worker_pane recorded, so the Reviewer pane cannot be split from it`);
    pane = await d.herdr.split(workerPane, d.session.cwdOf(lead.sessionPath));
    const started = await d.herdr.startAgent({
      name,
      kind: "pi",
      pane,
      timeoutMs: START_TIMEOUT_MS,
      args: reviewerArgs(d.config.reviewMode === "fork" ? lead.sessionPath : null, piModelArgFor(d, l.job.reviewTier)),
    });
    sessionPath = started.sessionPath;
    d.log("9 review", `Reviewer ${name} started (${d.config.reviewMode})`, { argv: started.argv });
    await prompt(d, "9 review", name, `Read ${brief} and follow it exactly.`, timeoutMs);
    let v = await readVerdict(d, sessionPath);
    if (v.verdict === "invalid") {
      d.log("9 review", `no valid verdict: ${v.reason}; re-prompting once`);
      await prompt(
        d,
        "9 review",
        name,
        REPROMPT,
        timeoutMs,
      );
      v = await readVerdict(d, sessionPath);
    }
    return v;
  } catch (e) {
    if (!(e instanceof CpdError)) throw e;
    d.log("9 review", `the Reviewer could not run: ${e.code}: ${e.message}`);
    return { verdict: "invalid", reason: `the Reviewer could not run: ${e.code}: ${e.message}` };
  } finally {
    if (sessionPath !== null) await collectMetrics(d, "review_metrics", sessionPath, sinceMs);
    if (pane !== null) await cleanup(d, "9 review", `close Reviewer pane ${pane}`, () => d.herdr.closePane(pane!));
  }
}

/**
 * Run the Reviewer for a tmux Lead: a read-only pi, forked from the Lead session unless `reviewMode` is `fresh`, in a new
 * detached window of the Lead's tmux server, in the Attempt's worktree. The window must not respawn when pi exits
 * (the cockpit's `remain-on-exit on` and `pane-died` hook would), so `remain-on-exit` is turned off for it. The
 * verdict comes from the session file, found by the id given to pi, once its last message is a finished assistant turn
 * and the file has been quiet for `REVIEWER_QUIET_MS`. One re-prompt through `send-keys`; the whole wait is bounded by
 * the Attempt timeout. A Reviewer that cannot run at all counts as no verdict.
 */
async function runTmuxReviewer(d: Deps, l: LoadedAttempt, brief: string, socket: string): Promise<ParsedVerdict> {
  const sinceMs = d.clock.now();
  const deadlineMs = sinceMs + d.config.attemptTimeoutMinutes * 60_000;
  const sessionId = randomUUID();
  const title = `review ${d.attemptId}`;
  let pane: string | null = null;
  let sessionPath: string | null = null;

  /** Wait for a finished assistant turn written at or after `fromMs`. */
  const settled = async (fromMs: number): Promise<string> => {
    for (;;) {
      sessionPath ??= d.session.findById(sessionId);
      const tail = sessionPath === null ? null : d.session.tail(sessionPath);
      if (sessionPath !== null && tail !== null && tail.role === "assistant" && tail.stopReason === "stop" && tail.writtenMs >= fromMs && d.clock.now() - tail.mtimeMs >= REVIEWER_QUIET_MS) {
        return sessionPath;
      }
      if (d.clock.now() >= deadlineMs) throw failure("reviewer_timeout", `the Reviewer did not finish a turn within ${d.config.attemptTimeoutMinutes} minutes`, { sessionId });
      await d.clock.sleep(REVIEWER_POLL_MS);
    }
  };

  try {
    const { lead } = d.workflow;
    const worktree = l.attempt.worktree;
    if (worktree === null) throw failure("worktree_missing", `Attempt ${d.attemptId} has no worktree recorded, so the Reviewer window has no directory`);
    const args = [
      ...reviewerArgs(d.config.reviewMode === "fork" ? lead.sessionPath : null, piModelArgFor(d, l.job.reviewTier)),
      "--session-id",
      sessionId,
      "-n",
      title,
      "--",
      `Read ${brief} and follow it exactly.`,
    ];
    pane = await d.tmux.newWindow(socket, { name: title, cwd: worktree, command: ["pi", ...args].map(shellQuote).join(" ") });
    await d.tmux.setWindowOption(socket, pane, "remain-on-exit", "off");
    d.log("9 review", `Reviewer ${title} started in tmux pane ${pane} (${d.config.reviewMode})`, { argv: ["pi", ...args], sessionId });
    let v = await readVerdict(d, await settled(sinceMs));
    if (v.verdict === "invalid") {
      d.log("9 review", `no valid verdict: ${v.reason}; re-prompting once`);
      const sentMs = d.clock.now();
      await d.tmux.sendKeys(socket, pane, [REPROMPT], true);
      await d.tmux.sendKeys(socket, pane, ["Enter"]);
      v = await readVerdict(d, await settled(sentMs));
    }
    return v;
  } catch (e) {
    if (!(e instanceof CpdError)) throw e;
    d.log("9 review", `the Reviewer could not run: ${e.code}: ${e.message}`);
    return { verdict: "invalid", reason: `the Reviewer could not run: ${e.code}: ${e.message}` };
  } finally {
    if (pane !== null) await cleanup(d, "9 review", `kill Reviewer window ${pane}`, () => d.tmux.killWindow(socket, pane!));
    if (sessionPath !== null) await collectMetrics(d, "review_metrics", sessionPath, sinceMs);
  }
}

async function review(d: Deps, l: LoadedAttempt): Promise<RunResult> {
  d.log("9 review", "preparing the review");
  const branch = attemptBranch(d.wf, d.attemptId);
  const diffPath = reviewDiffPath(d.wf, d.attemptId);
  const gatePath = reviewGatePath(d.wf, d.attemptId);
  if (!existsSync(gatePath)) throw failure("gate_output_missing", `the Gate output of the submit is missing: ${gatePath}`, { path: gatePath });
  const worktreePath = l.attempt.worktree;
  if (worktreePath === null) throw failure("worktree_missing", `Attempt ${d.attemptId} has no worktree recorded`);
  const diff = await d.git.diff(branch);
  writeTextAtomic(diffPath, diff.endsWith("\n") || diff === "" ? diff : `${diff}\n`);
  const brief = await d.render.reviewBrief({ diffPath, gatePath, worktree: worktreePath });
  d.log("9 review", `diff ${diffPath}, gate ${gatePath}, brief ${brief}`);

  const v = await runReviewer(d, l, brief);
  if (v.verdict === "invalid") return reviewNeeded(d, l, v.reason, brief, diffPath, gatePath);
  d.log("9 review", `verdict ${v.verdict}`, { feedback: v.feedback });
  await d.ledger.recordVerdict({ verdict: v.verdict, feedback: v.feedback });
  return v.verdict === "approve" ? merge(d) : rejected(d, v.feedback);
}

async function reviewNeeded(d: Deps, l: LoadedAttempt, reason: string, brief: string, diffPath: string, gatePath: string): Promise<RunResult> {
  await d.ledger.recordFacts({ review_needed_at: isoAt(d) });
  d.queue.notify({
    kind: "review-needed",
    job: l.job.key,
    attempt: l.attempt.id,
    text:
      `The Reviewer gave no valid verdict twice for Attempt ${l.attempt.id} of Job ${l.job.key} (${clip(reason)}). ` +
      `Read the brief ${brief}, the diff ${diffPath} and the Gate output ${gatePath}, then decide with: ` +
      `cpd review decide ${l.attempt.id} --approve --feedback "<text>" --wf ${d.wf} or ` +
      `cpd review decide ${l.attempt.id} --reject --feedback "<text>" --wf ${d.wf}`,
    data: { reason, brief, diff: diffPath, gate: gatePath },
  });
  d.log("9 review", "review-needed sent; waiting for cpd review decide");
  return { state: "review_needed" };
}

// --- steps 11 and 12: merge ---

const isLockTimeout = (e: unknown): boolean => e instanceof CpdError && e.code === "lock_timeout" && e.exitCode === 5;

/** Step 11: merge under the per-repo merge lock and map every `mergeAttempt` result to its §12 handling. */
async function merge(d: Deps): Promise<RunResult> {
  const l = await d.ledger.load();
  const worktreePath = l.attempt.worktree;
  if (worktreePath === null) throw failure("worktree_missing", `Attempt ${d.attemptId} has no worktree recorded`);
  const branch = attemptBranch(d.wf, d.attemptId);
  const limit = d.mergeLockWaitMs ?? MERGE_LOCK_WAIT_MS;
  const began = d.clock.now();
  let out: MergeOutcome;
  for (;;) {
    d.log("11 merge", `merging ${branch} into ${d.workflow.leadBranch}`);
    try {
      out = await d.git.merge(worktreePath, branch);
      break;
    } catch (e) {
      if (!isLockTimeout(e)) throw e;
      const waited = d.clock.now() - began;
      d.log("11 merge", `another merge is running (merge lock busy); waited ${Math.round(waited / 1000)} s so far`);
      if (waited >= limit) {
        return fail(d, "merge_lock_timeout", { ladder: false, detail: `the merge lock stayed busy for ${Math.round(waited / 60_000)} minutes` });
      }
    }
  }
  d.log("11 merge", `result ${out.result}`, { detail: out.detail, files: out.files, pre: out.preSha, post: out.postSha });
  switch (out.result) {
    case "merged":
      if (out.postSha === undefined) throw failure("merge_result_invalid", "mergeAttempt reported merged without a post-merge SHA", { out });
      return merged(d, out.postSha, { reviewed: l.attempt.outcome === "approved" }); // no verdict recorded: reviewMode none
    case "conflict":
      return fail(d, "merge_conflict", { ladder: true, detail: out.detail });
    case "cas_lost":
      return fail(d, "merge_cas_lost", { ladder: false, detail: `the Lead Branch kept moving: ${out.detail}` });
    case "dirty_target":
      return fail(d, "dirty_target", { ladder: false, detail: `${out.detail} The Lead worktree must stay clean: commit or stash there, then retry the Job.` });
    case "tampered":
      return fail(d, "gate_tampered", { ladder: false, detail: out.detail });
    case "merge_unverified":
      return fail(d, "merge_unverified", { ladder: false, detail: out.detail });
    case "unknown":
      return fail(d, "merge_failed", { ladder: false, detail: out.detail });
  }
}

/** Step 12. The Ledger is the truth, so it is written first; cleanup problems are logged and reported to the Lead, not fatal. */
async function merged(d: Deps, sha: string, o: { reviewed: boolean }): Promise<RunResult> {
  await d.ledger.markMerged(sha, o);
  const l = await d.ledger.load();
  d.log("12 merged", `Job ${l.job.key} merged at ${sha}`);
  const problems: string[] = [];
  const workspace = asString(l.attempt.metadata.workspace);
  if (workspace !== null) {
    const p = await cleanup(d, "12 merged", `close workspace ${workspace}`, () => d.herdr.closeWorkspace(workspace));
    if (p) problems.push(p);
  }
  const p = await cleanup(d, "12 merged", "remove the Attempt worktree and branch", () => d.git.removeWorktree(attemptBranch(d.wf, d.attemptId)));
  if (p) problems.push(p);
  d.queue.notify({
    kind: "job-merged",
    job: l.job.key,
    attempt: l.attempt.id,
    text:
      `Job ${l.job.key} is merged into ${d.workflow.leadBranch} at ${sha.slice(0, 12)} (Attempt ${l.attempt.id}, Tier ${l.attempt.tier}).` +
      `${problems.length > 0 ? ` Cleanup left something behind: ${problems.join("; ")}.` : ""} See the Workflow with: cpd workflow show --wf ${d.wf}`,
    data: { sha, tier: l.attempt.tier, cleanupProblems: problems },
  });
  return { state: "merged", sha };
}

// --- drivers ---

/** Wrap a lifecycle so an unexpected failure is logged and reported to the Lead (`error`) before it propagates. */
async function guarded<T>(d: Deps, fn: (d: Deps) => Promise<T>): Promise<T> {
  let step = "start";
  const traced: Deps = {
    ...d,
    log: (s, m, data) => {
      step = s;
      d.log(s, m, data);
    },
  };
  try {
    return await fn(traced);
  } catch (e) {
    const code = e instanceof CpdError ? e.code : "unexpected";
    d.log(step, `unexpected failure: ${code}: ${msg(e)}`, { code, details: e instanceof CpdError ? e.details : undefined });
    try {
      d.queue.notify({
        kind: "error",
        attempt: d.attemptId,
        text:
          `Attempt ${d.attemptId} hit an unexpected failure at step ${step}: ${clip(msg(e))} (${code}). ` +
          `The Attempt was left as it is. Read logs/attempt-${d.attemptId}.log and run: cpd attempt show ${d.attemptId} --wf ${d.wf}`,
        data: { step, code, message: msg(e) },
      });
    } catch (n) {
      d.log(step, `the Lead could not be notified: ${msg(n)}`);
    }
    throw e;
  }
}

/** `cpd attempt run`: steps 1 to 12. The slot is released on every exit path. */
export async function runAttempt(d: Deps): Promise<RunResult> {
  try {
    return await guarded(d, async (t) => {
      const loaded = await load(t);
      if ("done" in loaded) return loaded.done;
      const w = await worktree(t);
      if ("done" in w) return w.done;
      const specPath = await spec(t);
      const worker = await startWorker(t, loaded.l, w.path);
      if ("done" in worker) return worker.done;
      t.log("6 prompt", `prompting ${worker.name} to follow ${specPath}`);
      await prompt(t, "6 prompt", worker.name, `Read ${specPath} and follow it exactly. Your attempt id is ${t.attemptId}.`);
      const decided = await decide(t, worker.name);
      if (decided !== "submitted") return decided;
      if (t.config.reviewMode === "none") {
        t.log("9 review", "reviewMode is none: no Reviewer, straight to the merge");
        return merge(t);
      }
      return review(t, await t.ledger.load());
    });
  } finally {
    if (d.slot !== null) {
      try {
        await d.slot.release();
        d.log("slot", "released");
      } catch (e) {
        d.log("slot", `could not release the slot: ${msg(e)}`);
      }
    }
  }
}

/** `cpd review decide`: the Lead's verdict for an Attempt waiting after `review-needed`; continues at step 10 or 11. */
export async function decideReview(d: Deps, v: { verdict: "approve" | "reject"; feedback: string }): Promise<RunResult> {
  const l = await d.ledger.load();
  const waiting = l.attempt.status === "in_review" && l.attempt.outcome === null && l.attempt.metadata.review_needed_at !== undefined;
  if (!waiting) {
    throw refused("not_awaiting_review", `Attempt ${d.attemptId} is not waiting for a review decision (status ${l.attempt.status}, outcome ${l.attempt.outcome ?? "none"})`, {
      attempt: d.attemptId,
      status: l.attempt.status,
      outcome: l.attempt.outcome,
    });
  }
  return guarded(d, async (t) => {
    t.log("10 decide", `the Lead decided: ${v.verdict}`, { feedback: v.feedback });
    await t.ledger.recordVerdict(v);
    return v.verdict === "approve" ? merge(t) : rejected(t, v.feedback);
  });
}

// --- real wiring ---

/** Metrics of the session entries written at or after `sinceMs`; the header and `sinceMs === undefined` mean the whole file. */
export function metricsSince(path: string, sinceMs?: number): SessionMetrics {
  if (sinceMs === undefined) return sessionMetrics(path);
  const lines = readFileSync(path, "utf8").split("\n").filter((l) => l !== "");
  const written = (line: string): number => {
    try {
      const t = Date.parse((JSON.parse(line) as { timestamp?: unknown }).timestamp as string);
      return Number.isNaN(t) ? Infinity : t; // an entry we cannot date is kept, so sessionMetrics reports it loudly
    } catch {
      return Infinity;
    }
  };
  const dir = mkdtempSync(join(tmpdir(), "cpd-metrics-"));
  try {
    const file = join(dir, "session.jsonl");
    writeFileSync(file, lines.filter((l, i) => i === 0 || written(l) >= sinceMs).join("\n") + "\n");
    return sessionMetrics(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const SessionEntry = z.looseObject({ type: z.string() });
const MessageEntry = z.looseObject({
  timestamp: z.string(),
  message: z.looseObject({ role: z.string(), stopReason: z.string().optional() }),
});

/** The newest `message` entry of a session file (`SessionTail`). A last line pi is still writing (no newline yet) is not looked at. */
export function sessionTail(path: string): SessionTail | null {
  const text = readFileSync(path, "utf8");
  const mtimeMs = statSync(path).mtimeMs; // after the read: a write in between only makes the file look less quiet
  const lines = text.split("\n");
  lines.pop(); // after the final newline: "" or the line being written
  const bad = (i: number, why: string) => failure("session_malformed", `${path}:${i + 1}: ${why}`, { path, line: i + 1 });
  for (let i = lines.length - 1; i >= 0; i--) {
    let json: unknown;
    try {
      json = JSON.parse(lines[i]!);
    } catch (e) {
      throw bad(i, `invalid JSON: ${(e as Error).message}`);
    }
    const entry = SessionEntry.safeParse(json);
    if (!entry.success) throw bad(i, `not a session entry: ${z.prettifyError(entry.error)}`);
    if (entry.data.type !== "message") continue;
    const m = MessageEntry.safeParse(json);
    if (!m.success) throw bad(i, `bad message entry: ${z.prettifyError(m.error)}`);
    const writtenMs = Date.parse(m.data.timestamp);
    if (Number.isNaN(writtenMs)) throw bad(i, `bad timestamp: ${m.data.timestamp}`);
    return { role: m.data.message.role, stopReason: m.data.message.stopReason ?? null, writtenMs, mtimeMs };
  }
  return null;
}

/** `<pi agent dir>/sessions/<dir>/<time>_<id>.jsonl`, or null when pi has not created it yet. */
export function findSessionById(agentDir: string, id: string): string | null {
  const root = join(agentDir, "sessions");
  if (!existsSync(root)) return null;
  for (const dir of readdirSync(root, { withFileTypes: true })) {
    if (!dir.isDirectory()) continue;
    const hit = readdirSync(join(root, dir.name)).find((f) => f.endsWith(`_${id}.jsonl`));
    if (hit !== undefined) return join(root, dir.name, hit);
  }
  return null;
}

/** Append one line to `logs/attempt-<id>.log`. */
export function attemptLogger(wf: string, attemptId: string): Deps["log"] {
  const path = attemptLogPath(wf, attemptId);
  return (step, message, data) => {
    mkdirSync(logsDir(wf), { recursive: true });
    appendFileSync(path, `${new Date().toISOString()} [${step}] ${message}${data === undefined ? "" : ` ${JSON.stringify(data)}`}\n`);
  };
}

/** The real ports for one Attempt. `slot` is the slot `cpd watch` acquired for it, or null (`review decide`). */
export async function realDeps(wf: string, attemptId: string, slot: number | null): Promise<Deps> {
  const config = loadConfig();
  const catalog = await loadCatalog(config);
  const w = readWorkflow(wf);
  if (w.lead === null) throw refused("lead_not_attached", `Workflow ${wf} has no attached Lead`, { wf });
  if (w.leadWorktree === null) throw refused("lead_worktree_missing", `Workflow ${wf} has no Lead worktree (plan lock has not run)`, { wf });
  const { repo, leadWorktree, leadBranch } = { repo: w.repo, leadWorktree: w.leadWorktree, leadBranch: w.leadBranch };
  const mergeParams = (attemptWorktree: string, branch: string) => ({ repo, leadWorktree, leadBranch, attemptWorktree, branch });
  return {
    wf,
    attemptId,
    config,
    catalog,
    workflow: { repo, leadWorktree, leadBranch, lead: { sessionPath: w.lead.sessionPath, ...(w.lead.host === "tmux" ? { host: "tmux" as const, tmuxSocket: w.lead.tmuxSocket! } : { host: "herdr" as const }) } }, // `Lead` guarantees tmuxSocket for tmux
    ledger: {
      load: () => findAttempt(readLedger(wf), attemptId),
      verifyJob: async () => verifyJob(wf, (await findAttempt(readLedger(wf), attemptId)).job.key),
      recordFacts: (facts) => recordAttemptFacts(wf, attemptId, facts),
      recordVerdict: (v) => recordVerdict(wf, attemptId, v),
      fail: (cause, opts) => failAttempt(wf, attemptId, cause, opts),
      markMerged: (sha, opts) => markMerged(wf, attemptId, sha, opts),
      recordMetrics: (m, prefix) => recordMetrics(wf, attemptId, m, { prefix }),
    },
    git: {
      createWorktree: (branch) => createAttemptWorktree(leadWorktree, branch, leadBranch),
      removeWorktree: (branch) => removeAttemptWorktree(repo, leadWorktree, branch),
      async diff(branch) {
        const r = await git(leadWorktree, ["diff", `${leadBranch}...${branch}`]);
        if (r.code !== 0) throw failure("git_failed", `git diff exited ${r.code ?? r.signal}`, { stderr: r.stderr.slice(-2000) });
        return r.stdout;
      },
      merge: (attemptWorktree, branch) => mergeAttempt(mergeParams(attemptWorktree, branch)),
    },
    herdr: {
      openWorktree,
      rootPane: rootPaneOfWorkspace,
      split: (target, cwd) => splitPane(target, { direction: "right", cwd, noFocus: true }),
      startAgent,
      promptAgent: (name, text, o) => promptAgent(name, text, { wait: true, ...o }),
      agentGet,
      closePane,
      closeWorkspace,
    },
    tmux: { newWindow, setWindowOption, sendKeys, killWindow },
    session: {
      lastAssistant,
      findById: (id) => findSessionById(locatePi().agentDir, id),
      tail: sessionTail,
      cwdOf: (p) => readSessionHeader(p).cwd,
      metrics: metricsSince,
    },
    queue: { notify: (input) => notifyLead(wf, input) },
    render: {
      workerSpec: () => renderWorkerSpec(wf, attemptId),
      reviewBrief: (inputs) => renderReviewBrief(wf, attemptId, inputs),
    },
    clock: { now: () => Date.now(), sleep: (ms) => sleep(ms) },
    slot: slot === null ? null : { release: () => releaseSlot(slot, attemptId) },
    log: attemptLogger(wf, attemptId),
  };
}

/**
 * `cpd attempt run <id> --slot <i>` for real: build the ports, then run. The slot goes back even when the ports cannot be
 * built (no Lead attached, no pi catalog), so a failed start never holds capacity.
 */
export async function runWithSlot(wf: string, attemptId: string, slot: number): Promise<RunResult> {
  let deps: Deps;
  try {
    deps = await realDeps(wf, attemptId, slot);
  } catch (e) {
    try {
      await releaseSlot(slot, attemptId);
    } catch (r) {
      process.stderr.write(`cpd: could not release slot ${slot} of ${attemptId}: ${msg(r)}\n`);
    }
    throw e;
  }
  return runAttempt(deps);
}
