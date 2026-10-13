import type { Attempt } from "../ledger/attempts.ts";
import type { Job } from "../ledger/jobs.ts";
import { failure } from "../lib/errors.ts";
import { formatTier, type Catalog } from "../lib/tiers.ts";
import type { Answer, Question } from "../queue/escalation-files.ts";
import type { NotifyInput } from "../queue/lead-queue.ts";
import type { Clock } from "./attempt-run.ts";
import { automaticAttemptCount, tierForAttempt } from "./dispatch-one.ts";

// Contract §11 `watch` steps 1 and 2: mirror Escalations into the Ledger, then look at every open Attempt
// (timeout, crash, cancellation). Decisions are pure functions over a snapshot (`activeMs`, `tracked`, `judge`,
// `planMirror`, `planExpiry`); the `check*` / `mirror*` functions read the snapshot through ports and apply the decisions.

export type Log = (level: "info" | "warn" | "error", event: string, data?: Record<string, unknown>) => void;

/**
 * A problem that must not stop the loop: it is logged and the Lead gets a rate-limited `error` message. Rethrows what is
 * fatal (config or Ledger corruption), so the caller's `catch` is only for the item that failed.
 */
export type Report = (step: string, e: unknown) => void;

// --- active time (pure) ---

/** A stretch of time with an Escalation open, in epoch ms. `to: null` is still open. */
export type Span = { from: number; to: number | null };

/** Active time = (now − startedAt) − the time at least one Escalation was open (spans are clipped to the Attempt's life and merged). */
export function activeMs(startedAt: number, now: number, spans: readonly Span[]): number {
  const clipped = spans
    .map((s) => ({ from: Math.max(s.from, startedAt), to: Math.min(s.to ?? now, now) }))
    .filter((s) => s.to > s.from)
    .sort((a, b) => a.from - b.from);
  let waiting = 0;
  let covered = -Infinity;
  for (const s of clipped) {
    const from = Math.max(s.from, covered);
    if (s.to > from) waiting += s.to - from;
    covered = Math.max(covered, s.to);
  }
  return Math.max(0, now - startedAt - waiting);
}

// --- snapshot (pure) ---

/** An Escalation as the Ledger has it. `from` is when it was asked; `to` when it was answered or expired (null while open). */
export type LedgerEscalation = { id: string; attempt: string; open: boolean; from: number; to: number | null };

/** An Attempt whose Worker (or review) is meant to be going on, with what the decisions need. */
export type Tracked = {
  attempt: Attempt;
  job: Job;
  startedAt: number;
  spans: Span[];
  cancelRequested: boolean;
  timedOut: boolean;
  workerPane: string | null;
};

const OPEN_STATUSES: ReadonlySet<string> = new Set(["in_progress", "in_review"]);

/** The open Attempts (`in_progress`: Worker running; `in_review`: Reviewer or waiting for `cpd review decide`). */
export function tracked(attempts: readonly Attempt[], jobs: readonly Job[], escalations: readonly LedgerEscalation[]): Tracked[] {
  const out: Tracked[] = [];
  for (const attempt of attempts) {
    if (!OPEN_STATUSES.has(attempt.status)) continue;
    const job = jobs.find((j) => j.key === attempt.job);
    if (!job) throw failure("ledger_invalid", `Attempt ${attempt.id} has no Job ${attempt.job}`, { attempt: attempt.id });
    const startedAt = attempt.startedAt === null ? NaN : Date.parse(attempt.startedAt);
    if (Number.isNaN(startedAt)) throw failure("ledger_invalid", `Attempt ${attempt.id} has no valid started_at`, { attempt: attempt.id, startedAt: attempt.startedAt });
    out.push({
      attempt,
      job,
      startedAt,
      spans: escalations.filter((e) => e.attempt === attempt.id).map((e) => ({ from: e.from, to: e.to })),
      cancelRequested: "cancel_requested" in job.metadata,
      timedOut: attempt.metadata.timed_out === true,
      workerPane: typeof attempt.metadata.worker_pane === "string" ? attempt.metadata.worker_pane : null,
    });
  }
  return out;
}

// --- decisions (pure) ---

/** What was seen outside the Ledger. `agent` is `unchecked` when it cannot matter (the `attempt run` process is alive and the Attempt has not timed out). */
export type Observed = { runAlive: boolean; agent: "alive" | "gone" | "unchecked" };

export type Action =
  | { kind: "none" }
  | { kind: "cancel" }
  | { kind: "crash"; cause: "crashed" | "timed_out" }
  | { kind: "timeout" }
  | { kind: "close_pane" };

/**
 * First match wins:
 * - `cancel_requested`: stop the Attempt; an `in_review` Attempt only once its `attempt run` is gone (it may be merging);
 * - an `in_review` Attempt is not watched any further;
 * - the Worker agent is gone and `attempt run` is not alive: crashed (a timeout nobody finished counts as `timed_out`);
 * - already timed out: close the pane again if the Worker is still there;
 * - active time above the limit: timeout.
 */
export function judge(t: Tracked, o: Observed, now: number, limitMs: number): Action {
  if (t.cancelRequested) {
    if (t.attempt.status === "in_progress") return { kind: "cancel" };
    return o.runAlive ? { kind: "none" } : { kind: "cancel" };
  }
  if (t.attempt.status === "in_review") return { kind: "none" };
  if (!o.runAlive && o.agent === "gone") return { kind: "crash", cause: t.timedOut ? "timed_out" : "crashed" };
  if (t.timedOut) return o.agent === "alive" ? { kind: "close_pane" } : { kind: "none" };
  if (activeMs(t.startedAt, now, t.spans) > limitMs) return { kind: "timeout" };
  return { kind: "none" };
}

// --- ports ---

export type LivenessPorts = {
  wf: string;
  config: { attemptTimeoutMinutes: number; retryCap: number };
  catalog: Catalog;
  clock: Clock;
  log: Log;
  report: Report;
  ledger: {
    jobs(): Promise<Job[]>;
    attempts(): Promise<Attempt[]>;
    /** One Attempt, freshly read. */
    attempt(id: string): Promise<Attempt>;
    escalations(): Promise<LedgerEscalation[]>;
    /** Write-once `timed_out`. */
    markTimedOut(attempt: string): Promise<unknown>;
    failAttempt(attempt: string, cause: string, o: { requeue: boolean }): Promise<unknown>;
    /** `job cancel` for a Job with no open Attempt (a requested cancel that outlived its Attempt). */
    cancelJob(key: string, reason: string): Promise<unknown>;
    expireEscalation(id: string, o: { reason: "timeout" | "orphan"; expiredAt: string }): Promise<unknown>;
  };
  herdr: {
    agentAlive(name: string): Promise<boolean>;
    /** Closes the pane; a pane that is already gone is not an error. */
    closePane(id: string): Promise<void>;
  };
  /** Whether the `attempt run` process of the Attempt is alive (its slot holds its pid). */
  runAlive(attempt: string): boolean;
  /** Rename a question to `<id>.expired.json`. */
  expireQuestion(id: string): void;
  queue: { notify(input: NotifyInput): unknown };
};

/** Escalation id → Attempt id (`<attempt-id>-q<k>`). */
export const attemptOfEscalation = (escId: string): string => escId.slice(0, escId.lastIndexOf("-q"));

/** Tell the Lead. The Ledger is already changed, so a message that cannot be delivered is reported, never thrown. */
export function tell(d: Pick<LivenessPorts, "queue" | "report">, input: NotifyInput): void {
  try {
    d.queue.notify(input);
  } catch (e) {
    d.report("notify", e);
  }
}

// --- step 2: Attempts ---

/** What was done, and the Jobs as read at the start (valid only while `changed` is false). */
export type LivenessResult = { cancelled: string[]; timedOut: string[]; crashed: string[]; jobsCancelled: string[]; jobs: Job[] };

/** The Attempt as read now is still the one that was judged (no submit, no close in between). */
const unchanged = (a: Attempt, b: Attempt): boolean => a.status === b.status && a.submittedAt === b.submittedAt;

async function closeWorkerPane(d: LivenessPorts, t: Tracked): Promise<void> {
  if (t.workerPane !== null) await d.herdr.closePane(t.workerPane);
}

/** The Worker is gone or being stopped, so its open questions can never be answered: close them, or the Attempt could not be closed. */
async function orphanEscalations(d: LivenessPorts, attempt: string): Promise<void> {
  const open = (await d.ledger.escalations()).filter((e) => e.attempt === attempt && e.open);
  for (const e of open) {
    d.expireQuestion(e.id);
    await d.ledger.expireEscalation(e.id, { reason: "orphan", expiredAt: new Date(d.clock.now()).toISOString() });
    d.log("info", "escalation_expired", { escalation: e.id, reason: "orphan", why: "its Attempt was stopped" });
  }
}

/** §7 for the next automatic Attempt of a crashed one (the same rule as `attempt run`'s step 8). */
function nextAutomatic(d: LivenessPorts, t: Tracked): { next: { id: string; tier: string } } | { stop: string } {
  const n = t.attempt.number;
  const count = automaticAttemptCount(t.job, n);
  if (count >= 1 + d.config.retryCap) return { stop: `The retry cap is reached (${count} automatic Attempts, retryCap ${d.config.retryCap}).` };
  const tier = tierForAttempt(d.catalog, t.job, n + 1);
  if (tier === null) return { stop: `No higher Tier than ${t.attempt.tier} exists.` };
  return { next: { id: `${t.job.key}-a${n + 1}`, tier: formatTier(tier) } };
}

async function cancel(d: LivenessPorts, t: Tracked): Promise<void> {
  await closeWorkerPane(d, t);
  await orphanEscalations(d, t.attempt.id);
  await d.ledger.failAttempt(t.attempt.id, "cancelled", { requeue: false });
  d.log("info", "attempt_cancelled", { attempt: t.attempt.id, job: t.job.key, reason: t.job.metadata.cancel_reason ?? null });
}

/** `timed_out` is written first, so `attempt run` (which reads it before anything else) never mistakes the closed pane for a crash. */
async function timeout(d: LivenessPorts, t: Tracked, activeMinutes: number): Promise<void> {
  await d.ledger.markTimedOut(t.attempt.id);
  await closeWorkerPane(d, t);
  await orphanEscalations(d, t.attempt.id);
  d.log("warn", "attempt_timed_out", { attempt: t.attempt.id, activeMinutes: Math.round(activeMinutes * 10) / 10, limitMinutes: d.config.attemptTimeoutMinutes });
}

async function crash(d: LivenessPorts, t: Tracked, cause: "crashed" | "timed_out"): Promise<void> {
  const { attempt, job } = t;
  await closeWorkerPane(d, t);
  await orphanEscalations(d, attempt.id);
  const plan = cause === "crashed" ? nextAutomatic(d, t) : null;
  const next = plan !== null && "next" in plan ? plan.next : null;
  const stop = plan !== null && "stop" in plan ? plan.stop : "This failure is not retried automatically.";
  await d.ledger.failAttempt(attempt.id, cause, { requeue: next !== null });
  d.log("warn", "attempt_crashed", { attempt: attempt.id, cause, retry: next !== null });
  const why =
    cause === "timed_out"
      ? `it exceeded the ${d.config.attemptTimeoutMinutes}-minute active-time limit and nothing was left to close it`
      : "its Worker agent is gone and its `cpd attempt run` process is not alive";
  if (next !== null) {
    tell(d, {
      kind: "attempt-retry",
      job: job.key,
      attempt: attempt.id,
      text:
        `Attempt ${attempt.id} of Job ${job.key} ${cause === "timed_out" ? "timed out" : "crashed"}: ${why}. The next Attempt ${next.id} will start at Tier ${next.tier} ` +
        `(automatic Attempt ${automaticAttemptCount(job, attempt.number) + 1} of ${1 + d.config.retryCap}). Inspect it with: cpd attempt show ${attempt.id} --wf ${d.wf}`,
      data: { cause, nextAttempt: next.id, nextTier: next.tier, worktree: attempt.worktree },
    });
  } else {
    tell(d, {
      kind: "job-failed",
      job: job.key,
      attempt: attempt.id,
      text:
        `Job ${job.key} failed on Attempt ${attempt.id} (${cause}): ${why}. ${stop} ` +
        `Worktree kept${attempt.worktree ? ` at ${attempt.worktree}` : ""}. Try again at a Tier you choose with: cpd job retry ${job.key} --tier <T> --wf ${d.wf} ` +
        `or give up with: cpd job cancel ${job.key} --reason "<why>" --wf ${d.wf}`,
      data: { cause, worktree: attempt.worktree, branch: attempt.branch },
    });
  }
}

async function observeAgent(d: LivenessPorts, t: Tracked, runAlive: boolean): Promise<Observed["agent"]> {
  if (t.attempt.status !== "in_progress" || (runAlive && !t.timedOut)) return "unchecked";
  if (t.attempt.agent === null) return "gone"; // the Worker was never started
  return (await d.herdr.agentAlive(t.attempt.agent)) ? "alive" : "gone";
}

/**
 * Contract §11 `watch` step 2: judge every open Attempt and act. One Attempt failing does not stop the others (it is reported).
 * Also finishes a requested cancel whose Attempt ended in the meantime (the Job would otherwise be started again).
 */
export async function checkLiveness(d: LivenessPorts): Promise<LivenessResult> {
  const jobs = await d.ledger.jobs();
  const result: LivenessResult = { cancelled: [], timedOut: [], crashed: [], jobsCancelled: [], jobs };
  const attempts = await d.ledger.attempts();
  const escalations = attempts.some((a) => OPEN_STATUSES.has(a.status)) ? await d.ledger.escalations() : [];
  const now = d.clock.now();
  const limitMs = d.config.attemptTimeoutMinutes * 60_000;
  const open = tracked(attempts, jobs, escalations);

  for (const t of open) {
    const id = t.attempt.id;
    try {
      const runAlive = d.runAlive(id);
      const action = judge(t, { runAlive, agent: await observeAgent(d, t, runAlive) }, now, limitMs);
      if (action.kind === "none") continue;
      if (!unchanged(t.attempt, await d.ledger.attempt(id))) {
        d.log("info", "attempt_changed", { attempt: id, action: action.kind });
        continue;
      }
      switch (action.kind) {
        case "cancel":
          await cancel(d, t);
          result.cancelled.push(id);
          break;
        case "timeout":
          await timeout(d, t, activeMs(t.startedAt, now, t.spans) / 60_000);
          result.timedOut.push(id);
          break;
        case "close_pane":
          await closeWorkerPane(d, t);
          break;
        case "crash":
          await crash(d, t, action.cause);
          result.crashed.push(id);
          break;
      }
    } catch (e) {
      d.report(`liveness ${id}`, e);
    }
  }

  const withAttempt = new Set(open.map((t) => t.job.key));
  for (const job of jobs) {
    if (!("cancel_requested" in job.metadata) || job.status === "closed" || job.status === "cancelled" || withAttempt.has(job.key)) continue;
    try {
      const reason = typeof job.metadata.cancel_reason === "string" ? job.metadata.cancel_reason : "cancel requested (no reason recorded)";
      await d.ledger.cancelJob(job.key, reason);
      result.jobsCancelled.push(job.key);
      d.log("info", "job_cancelled", { job: job.key, reason });
    } catch (e) {
      d.report(`liveness ${job.key}`, e);
    }
  }
  return result;
}

// --- step 1: Escalations ---

/** Ids (`<attempt-id>-q<k>`) of the files in `escalations/`, by kind. `questions` includes answered ones; `expired` are `<id>.expired.json`, never open. */
export type EscalationFiles = { questions: string[]; answers: string[]; expired: string[] };

export type EscalationAction = { kind: "record" | "answer" | "expire"; id: string };

export type KnownEscalations = Map<string, "open" | "closed">;

/** Pure: new questions to record, and answers to record on Escalations that are not closed yet (or are recorded in this same pass). */
export function planMirror(files: Pick<EscalationFiles, "questions" | "answers">, known: KnownEscalations): EscalationAction[] {
  const out: EscalationAction[] = [];
  for (const id of files.questions) if (!known.has(id)) out.push({ kind: "record", id });
  for (const id of files.answers) {
    if (known.get(id) === "closed") continue;
    if (known.has(id) || files.questions.includes(id)) out.push({ kind: "answer", id });
  }
  return out;
}

/** Pure: expired questions whose Escalation is still open in the Ledger. An expired question that was never recorded cannot block an Attempt. */
export function planExpiry(expired: readonly string[], known: KnownEscalations): EscalationAction[] {
  return expired.filter((id) => known.get(id) === "open").map((id) => ({ kind: "expire", id }));
}

export type EscalationPorts = {
  wf: string;
  clock: Clock;
  log: Log;
  report: Report;
  ledger: {
    escalations(): Promise<LedgerEscalation[]>;
    recordEscalation(attempt: string, o: { escId: string; question: string; askedAt: string }): Promise<unknown>;
    recordAnswer(escId: string, o: { answer: string; answeredAt: string }): Promise<unknown>;
    expireEscalation(id: string, o: { reason: "timeout" | "orphan"; expiredAt: string }): Promise<unknown>;
  };
  files: {
    list(): EscalationFiles;
    question(id: string): Question | null;
    answer(id: string): Answer | null;
    /** `expireOrphans` of the queue module: renames the questions whose asker is dead and returns their ids. */
    expireOrphans(): string[];
  };
};

/**
 * Contract §11 `watch` step 1. In this order: record new questions, record answers, expire orphaned questions
 * (asker dead), then close the Ledger Escalation of every `<id>.expired.json` (ask timeout or orphan).
 */
export async function mirrorEscalations(d: EscalationPorts): Promise<void> {
  const first = d.files.list();
  if (first.questions.length + first.answers.length + first.expired.length === 0) return; // nothing was ever asked: no Ledger read (a `bd` call takes about a second)
  const known: KnownEscalations = new Map((await d.ledger.escalations()).map((e) => [e.id, e.open ? "open" : "closed"]));
  const failed = new Set<string>();
  const at = () => new Date(d.clock.now()).toISOString();

  for (const a of planMirror(first, known)) {
    if (failed.has(a.id)) continue; // an answer for a question that could not be recorded would only fail too
    try {
      if (a.kind === "record") {
        const q = d.files.question(a.id);
        if (q === null) continue; // expired between listing and reading
        await d.ledger.recordEscalation(attemptOfEscalation(a.id), { escId: a.id, question: q.question, askedAt: q.askedAt });
        known.set(a.id, "open");
        d.log("info", "escalation_mirrored", { escalation: a.id, attempt: q.attempt });
      } else {
        const ans = d.files.answer(a.id);
        if (ans === null) continue;
        await d.ledger.recordAnswer(a.id, { answer: ans.answer, answeredAt: ans.answeredAt });
        known.set(a.id, "closed");
        d.log("info", "escalation_answer_mirrored", { escalation: a.id });
      }
    } catch (e) {
      failed.add(a.id);
      d.report(`escalation ${a.id}`, e);
    }
  }

  let orphans = new Set<string>();
  try {
    orphans = new Set(d.files.expireOrphans());
    for (const id of orphans) d.log("info", "escalation_orphaned", { escalation: id });
  } catch (e) {
    d.report("escalation orphans", e);
  }

  for (const a of planExpiry(d.files.list().expired, known)) {
    try {
      await d.ledger.expireEscalation(a.id, { reason: orphans.has(a.id) ? "orphan" : "timeout", expiredAt: at() });
      known.set(a.id, "closed");
    } catch (e) {
      d.report(`escalation ${a.id}`, e);
    }
  }
}
