import { z } from "zod";
import { failure, notFound, refused, usage } from "../lib/errors.ts";
import { assertAttemptId, assertJobKey, attemptId } from "../lib/paths.ts";
import { formatTier, parseTier } from "../lib/tiers.ts";
import { logEvent } from "./audit.ts";
import { createIssue, listIssues, metadataArgs, readLedger, withLedger, type Ledger, type ReadLedger } from "./bd.ts";
import { verifyJob } from "./fingerprint.ts";
import { loadJobs, type Job } from "./jobs.ts";
import { BdIssues, type BdIssue } from "./schema.ts";

// Contract §8 (Attempt mapping), §11 (`attempt list|show`), §12 (the Ledger writes of each step).
// Every write runs under `locks/ledger.lock`, checks its preconditions first (nothing is written
// when one fails), and uses `--if-status` compare-and-swap for state changes.

export const ATTEMPT_STATUSES = ["in_progress", "in_review", "closed"] as const;
export type AttemptStatus = (typeof ATTEMPT_STATUSES)[number];
const AttemptStatus = z.enum(ATTEMPT_STATUSES);

export const OUTCOMES = ["approved", "rejected", "failed", "timed_out", "cancelled"] as const;
export type Outcome = (typeof OUTCOMES)[number];

const AttemptMeta = z.looseObject({ number: z.number().int().positive(), tier: z.string() });

export type Attempt = {
  /** `<job-key>-a<n>` */
  id: string;
  /** beads issue id */
  issue: string;
  /** Job key */
  job: string;
  /** beads issue id of the Job */
  jobIssue: string;
  number: number;
  tier: string;
  status: AttemptStatus;
  outcome: string | null;
  cause: string | null;
  worktree: string | null;
  branch: string | null;
  agent: string | null;
  sessionPath: string | null;
  startedAt: string | null;
  submittedAt: string | null;
  endedAt: string | null;
  closeReason: string | null;
  metadata: Record<string, unknown>;
};

const str = (m: Record<string, unknown>, k: string): string | null => (typeof m[k] === "string" ? (m[k] as string) : null);

function toAttempt(issue: BdIssue, job: Job): Attempt {
  const status = AttemptStatus.safeParse(issue.status);
  const meta = AttemptMeta.safeParse(issue.metadata ?? {});
  if (!status.success || !meta.success) {
    throw failure("ledger_invalid", `Attempt issue ${issue.id} does not have the expected shape`, {
      id: issue.id,
      issues: [...(status.success ? [] : status.error.issues), ...(meta.success ? [] : meta.error.issues)],
    });
  }
  const m = meta.data;
  return {
    id: attemptId(job.key, m.number),
    issue: issue.id,
    job: job.key,
    jobIssue: job.id,
    number: m.number,
    tier: m.tier,
    status: status.data,
    outcome: str(m, "outcome"),
    cause: str(m, "cause"),
    worktree: str(m, "worktree"),
    branch: str(m, "branch"),
    agent: str(m, "agent"),
    sessionPath: str(m, "session_path"),
    startedAt: str(m, "started_at"),
    submittedAt: str(m, "submitted_at"),
    endedAt: str(m, "ended_at"),
    closeReason: issue.close_reason ?? null,
    metadata: m,
  };
}

const byNumber = (a: Attempt, b: Attempt) => a.number - b.number;

/** Every Attempt of one Job, in Attempt-number order. */
async function attemptsOf(l: ReadLedger, job: Job): Promise<Attempt[]> {
  return (await listIssues(l, "attempt", ["--parent", job.id])).map((i) => toAttempt(i, job)).sort(byNumber);
}

function findJob(jobs: Job[], key: string): Job {
  const job = jobs.find((j) => j.key === key);
  if (!job) throw notFound("job_not_found", `no Job with key ${key}`, { key });
  return job;
}

/** Split `<job-key>-a<n>`. The key may itself contain `-a<n>`, so the last `-a<n>` is the number. */
export function splitAttemptId(id: string): { key: string; number: number } {
  assertAttemptId(id);
  const i = id.lastIndexOf("-a");
  return { key: id.slice(0, i), number: Number(id.slice(i + 2)) };
}

/** Attempt and its Job, read through `l` (a locked handle or a read handle). */
export async function findAttempt(l: ReadLedger, id: string): Promise<{ attempt: Attempt; job: Job; siblings: Attempt[] }> {
  const { key, number } = splitAttemptId(id);
  const job = (await loadJobs(l)).jobs.find((j) => j.key === key);
  const siblings = job ? await attemptsOf(l, job) : [];
  const attempt = siblings.find((a) => a.number === number);
  if (!job || !attempt) throw notFound("attempt_not_found", `no Attempt ${id}`, { attempt: id });
  return { attempt, job, siblings };
}

// --- reads ---

export async function attemptCount(wf: string, jobKey: string): Promise<number> {
  assertJobKey(jobKey);
  const l = readLedger(wf);
  return (await attemptsOf(l, findJob((await loadJobs(l)).jobs, jobKey))).length;
}

/** Every Attempt, ordered by Job creation then Attempt number. `status` is the beads status of the Attempt. */
export async function listAttempts(wf: string, opts: { status?: string } = {}): Promise<Attempt[]> {
  let status: AttemptStatus | undefined;
  if (opts.status !== undefined) {
    const s = AttemptStatus.safeParse(opts.status);
    if (!s.success) throw usage("invalid_status", `invalid status ${JSON.stringify(opts.status)}: expected one of ${ATTEMPT_STATUSES.join(", ")}`);
    status = s.data;
  }
  const l = readLedger(wf);
  const { jobs } = await loadJobs(l);
  const out: Attempt[] = [];
  for (const issue of await listIssues(l, "attempt")) {
    const job = jobs.find((j) => j.id === issue.parent);
    if (!job) throw failure("ledger_invalid", `Attempt issue ${issue.id} has no Job parent`, { id: issue.id });
    out.push(toAttempt(issue, job));
  }
  out.sort((a, b) => jobs.findIndex((j) => j.key === a.job) - jobs.findIndex((j) => j.key === b.job) || byNumber(a, b));
  return status ? out.filter((a) => a.status === status) : out;
}

export async function getAttempt(wf: string, id: string): Promise<Attempt> {
  return (await findAttempt(readLedger(wf), id)).attempt;
}

/** Attempts whose Worker is meant to be alive: beads status `in_progress` (an `in_review` Attempt is waiting for a verdict). */
export const runningAttempts = (wf: string): Promise<Attempt[]> => listAttempts(wf, { status: "in_progress" });

const Provenance = z.array(z.looseObject({ kind: z.string() }));

/** `bd provenance log` rows of an Attempt (cut, commit, land). */
export async function attemptProvenance(wf: string, id: string): Promise<Record<string, unknown>[]> {
  // `provenance` is not in bd.ts's read-only list, so this goes through the writer handle.
  return withLedger(wf, `provenance ${id}`, async (l) => {
    const { attempt } = await findAttempt(l, id);
    return l.write(["provenance", "log", attempt.issue], Provenance);
  });
}

// --- write helpers ---

const now = () => new Date().toISOString();
const SHA = /^[0-9a-f]{40}$/;

function assertSha(sha: unknown): asserts sha is string {
  if (typeof sha !== "string" || !SHA.test(sha)) throw usage("invalid_sha", `invalid commit sha ${JSON.stringify(sha)}: expected 40 lowercase hex characters`, { sha });
}

const attemptSet = (a: Attempt, patch: Record<string, unknown>, once = true) => metadataArgs(a.metadata, patch, once);

async function provenance(l: Ledger, a: Attempt, kind: "cut" | "commit" | "land", ref: string, refKind: "branch" | "git-sha", payload: Record<string, unknown>) {
  await l.write(["provenance", "record", "--issue", a.issue, "--kind", kind, "--source", "cpd", "--ref", ref, "--ref-kind", refKind, "--payload", JSON.stringify(payload)]);
}

/** `bd close` refuses an issue with open children; for an Attempt that means an open Escalation. */
async function closeIssue(l: Ledger, id: string, reason: string, openEscalation?: string): Promise<void> {
  try {
    await l.write(["close", id, "--reason", reason]);
  } catch (e) {
    if (openEscalation !== undefined && (e as { code?: string }).code === "bd_failed" && /open child/i.test(String((e as Error).message))) {
      throw refused("escalation_open", `Attempt has an open Escalation`, { id, escalation: openEscalation });
    }
    throw e;
  }
}

/** Refuse (before any write) while the Attempt has an unanswered Escalation. */
async function assertNoOpenEscalation(l: ReadLedger, a: Attempt): Promise<void> {
  const open = (await listIssues(l, "escalation", ["--parent", a.issue])).filter((e) => e.status !== "closed");
  if (open.length > 0) {
    const ids = open.map((e) => (typeof e.metadata?.esc_id === "string" ? e.metadata.esc_id : e.id));
    throw refused("escalation_open", `Attempt ${a.id} has an open Escalation: ${ids.join(", ")}`, { attempt: a.id, escalations: ids });
  }
}

function illegal(what: string, a: Attempt, job: Job): never {
  throw refused("illegal_transition", `cannot ${what}: Attempt ${a.id} is ${a.status}, Job ${job.key} is ${job.status}`, {
    attempt: a.id,
    attemptStatus: a.status,
    job: job.key,
    jobStatus: job.status,
  });
}

// --- start ---

export type StartOptions = { tier: string; worktree?: string; branch?: string };

/**
 * Start the next Attempt of a queued Job. The Job's fingerprint is verified first. Compare-and-swap
 * `queued → running`: a lost race (or a Job that is no longer `queued`) returns null and creates nothing.
 */
export async function startAttempt(wf: string, jobKey: string, opts: StartOptions): Promise<Attempt | null> {
  assertJobKey(jobKey);
  const tier = formatTier(parseTier(opts.tier));
  return withLedger(wf, `attempt start ${jobKey}`, async (l) => {
    await verifyJob(wf, jobKey, l);
    const job = findJob((await loadJobs(l)).jobs, jobKey);
    if (job.status !== "queued") {
      logEvent(wf, "info", "cas_lost", { job: jobKey, status: job.status });
      return null;
    }
    const ready = await l.read(["ready", "-t", "job", "-n", "0"], BdIssues);
    if (!ready.some((i) => i.id === job.id)) throw refused("job_not_ready", `Job ${jobKey} is queued but blocked by its Dependencies`, { job: jobKey });
    const existing = await attemptsOf(l, job);
    const open = existing.find((a) => a.status !== "closed");
    if (open) throw refused("attempt_open", `Job ${jobKey} already has an open Attempt ${open.id}`, { job: jobKey, attempt: open.id });

    try {
      await l.write(["update", job.id, "--if-status", "queued", "-s", "running"]);
    } catch (e) {
      if ((e as { code?: string }).code === "cas_lost") {
        logEvent(wf, "info", "cas_lost", { job: jobKey });
        return null;
      }
      throw e;
    }
    const number = existing.length + 1;
    const meta: Record<string, unknown> = {
      number,
      tier,
      started_at: now(),
      ...(opts.worktree === undefined ? {} : { worktree: opts.worktree }),
      ...(opts.branch === undefined ? {} : { branch: opts.branch }),
    };
    let issueId: string;
    try {
      issueId = await createIssue(l, [`--title=${jobKey} attempt ${number}`, "-t", "attempt", "--parent", job.id, "-s", "in_progress", "--metadata", JSON.stringify(meta)]);
    } catch (e) {
      // Do not leave a `running` Job without an Attempt.
      await l.write(["update", job.id, "--if-status", "running", "-s", "queued"]).catch(() => undefined);
      throw e;
    }
    const attempt = (await attemptsOf(l, job)).find((a) => a.issue === issueId)!;
    if (opts.branch !== undefined) await provenance(l, attempt, "cut", opts.branch, "branch", { tier, worktree: opts.worktree ?? null, at: attempt.startedAt });
    logEvent(wf, "info", "attempt_started", { attempt: attempt.id, tier });
    return attempt;
  });
}

// --- facts ---

/** Keys with a dedicated writer. */
const RESERVED = new Set(["number", "tier", "outcome", "cause", "submitted_at", "ended_at"]);
const FactKey = z
  .string()
  .regex(/^[a-z][a-z0-9_]*$/)
  .refine((k) => !RESERVED.has(k) && !k.startsWith("metrics_") && !k.startsWith("review_metrics_"), "key has a dedicated writer");
const Facts = z.record(FactKey, z.union([z.string(), z.number(), z.boolean(), z.array(z.string())])).refine((f) => Object.keys(f).length > 0, "no facts given");
export type AttemptFacts = z.input<typeof Facts>;

/** Write-once facts of an Attempt (`worktree, branch, agent, session_path, started_at, argv, …`). A second write of a key is `metadata_write_once`. Recording `branch` also records provenance `cut`. */
export async function recordAttemptFacts(wf: string, id: string, facts: AttemptFacts): Promise<Attempt> {
  const parsed = Facts.safeParse(facts);
  if (!parsed.success) throw usage("invalid_facts", "invalid Attempt facts", { issues: parsed.error.issues });
  return withLedger(wf, `attempt facts ${id}`, async (l) => {
    const { attempt } = await findAttempt(l, id);
    await l.write(["update", attempt.issue, ...attemptSet(attempt, parsed.data)]);
    const after = (await findAttempt(l, id)).attempt;
    if (typeof parsed.data.branch === "string") {
      await provenance(l, after, "cut", parsed.data.branch, "branch", { tier: after.tier, worktree: after.worktree, at: now() });
    }
    return after;
  });
}

// --- submit, verdict, fail, merged ---

/** Worker submitted: Attempt `in_progress → in_review` with `submitted_at`, provenance `commit`, Job `running → in_review`. */
export async function markSubmitted(wf: string, id: string, commitSha: string): Promise<Attempt> {
  assertSha(commitSha);
  return withLedger(wf, `attempt submit ${id}`, async (l) => {
    const { attempt, job } = await findAttempt(l, id);
    if (attempt.status !== "in_progress" || job.status !== "running") illegal("submit", attempt, job);
    await l.write(["update", attempt.issue, "--if-status", "in_progress", "-s", "in_review", ...attemptSet(attempt, { submitted_at: now() })]);
    await l.write(["update", job.id, "--if-status", "running", "-s", "in_review"]);
    const after = (await findAttempt(l, id)).attempt;
    await provenance(l, after, "commit", commitSha, "git-sha", { at: after.submittedAt });
    logEvent(wf, "info", "attempt_submitted", { attempt: id, commit: commitSha });
    return after;
  });
}

const Verdict = z.strictObject({ verdict: z.enum(["approve", "reject"]), feedback: z.string().min(1) });
export type VerdictInput = z.input<typeof Verdict>;

/** Reviewer (or Lead) verdict on an `in_review` Attempt: the feedback as a comment `APPROVED:|REJECTED:`, plus write-once `outcome=approved|rejected`. */
export async function recordVerdict(wf: string, id: string, v: VerdictInput): Promise<Attempt> {
  const parsed = Verdict.safeParse(v);
  if (!parsed.success) throw usage("invalid_verdict", "invalid verdict", { issues: parsed.error.issues });
  const outcome = parsed.data.verdict === "approve" ? "approved" : "rejected";
  return withLedger(wf, `attempt verdict ${id}`, async (l) => {
    const { attempt, job } = await findAttempt(l, id);
    if (attempt.status !== "in_review") illegal("record a verdict", attempt, job);
    const args = attemptSet(attempt, { outcome }); // refuses a second verdict before anything is written
    await l.write(["comments", "add", attempt.issue, `${outcome.toUpperCase()}: ${parsed.data.feedback}`]);
    await l.write(["update", attempt.issue, ...args]);
    logEvent(wf, "info", "attempt_verdict", { attempt: id, outcome });
    return (await findAttempt(l, id)).attempt;
  });
}

const Cause = z.string().regex(/^[a-z][a-z0-9_]*$/);

/** cause → Attempt outcome (contract §8 outcomes). */
export function outcomeForCause(cause: string): Outcome {
  if (cause === "review_rejected") return "rejected";
  if (cause === "timed_out") return "timed_out";
  if (cause === "cancelled") return "cancelled";
  return "failed";
}

/**
 * Close an open Attempt as not merged. `outcome` follows from `cause` (`review_rejected → rejected`,
 * `timed_out`, `cancelled`, anything else `failed`). The Job becomes `queued` with `requeue`, `cancelled`
 * for cause `cancelled`, else `failed`. A verdict already recorded as the same outcome stays as it is;
 * the one rewrite allowed is `approved → failed` (approved, but the merge or post-merge Gate failed).
 * Refused with `escalation_open` while the Attempt has an unanswered Escalation.
 */
export async function failAttempt(wf: string, id: string, cause: string, opts: { requeue: boolean }): Promise<Attempt> {
  if (!Cause.safeParse(cause).success) throw usage("invalid_cause", `invalid cause ${JSON.stringify(cause)}`, { cause });
  if (cause === "cancelled" && opts.requeue) throw usage("invalid_requeue", "a cancelled Attempt cannot be requeued", { cause });
  const outcome = outcomeForCause(cause);
  return withLedger(wf, `attempt fail ${id}`, async (l) => {
    const { attempt, job } = await findAttempt(l, id);
    const jobOk = attempt.status === "in_progress" ? job.status === "running" : job.status === "in_review";
    if (attempt.status === "closed" || !jobOk) illegal("fail", attempt, job);
    await assertNoOpenEscalation(l, attempt);

    const override = attempt.outcome === "approved" && outcome === "failed";
    const base = override ? Object.fromEntries(Object.entries(attempt.metadata).filter(([k]) => k !== "outcome")) : attempt.metadata;
    if (attempt.outcome !== null && attempt.outcome !== outcome && !override) {
      throw refused("metadata_write_once", `Attempt ${id} already has outcome ${attempt.outcome}; cause ${cause} means ${outcome}`, { key: "outcome" });
    }
    const patch = { ...(attempt.outcome === outcome ? {} : { outcome }), cause, ended_at: now() };
    await l.write(["update", attempt.issue, "--if-status", attempt.status, ...metadataArgs(base, patch, true)]);
    await closeIssue(l, attempt.issue, outcome, id);

    const target = cause === "cancelled" ? "cancelled" : opts.requeue ? "queued" : "failed";
    await l.write([
      "update",
      job.id,
      "--if-status",
      job.status,
      "-s",
      target,
      ...(target === "cancelled" ? metadataArgs(job.metadata, { cancelled_at: now() }, true) : []),
    ]);
    logEvent(wf, "info", "attempt_failed", { attempt: id, outcome, cause, jobStatus: target });
    return (await findAttempt(l, id)).attempt;
  });
}

/**
 * The Attempt's commit landed on the Lead Branch. Only from an `in_review` Job and Attempt whose verdict
 * is `approved`, else `illegal_transition` / `attempt_not_approved` and nothing is written. Closes the Attempt
 * `approved`, records `merged_sha` on the Job, closes the Job with reason `merged`, records provenance `land`.
 */
export async function markMerged(wf: string, id: string, mergedSha: string): Promise<Attempt> {
  assertSha(mergedSha);
  return withLedger(wf, `attempt merged ${id}`, async (l) => {
    const { attempt, job, siblings } = await findAttempt(l, id);
    if (attempt.status !== "in_review" || job.status !== "in_review" || attempt.number !== siblings.length) illegal("mark merged", attempt, job);
    if (attempt.outcome !== "approved") {
      throw refused("attempt_not_approved", `Attempt ${id} has no approved verdict (outcome ${attempt.outcome ?? "unset"})`, { attempt: id, outcome: attempt.outcome });
    }
    await assertNoOpenEscalation(l, attempt);
    const jobArgs = metadataArgs(job.metadata, { merged_sha: mergedSha }, true); // refuses a second merge before any write

    await l.write(["update", job.id, "--if-status", "in_review", ...jobArgs]);
    await l.write(["update", attempt.issue, "--if-status", "in_review", ...attemptSet(attempt, { ended_at: now() })]);
    await closeIssue(l, attempt.issue, "approved", id);
    await l.write(["close", job.id, "--reason", "merged"]);
    const after = (await findAttempt(l, id)).attempt;
    await provenance(l, after, "land", mergedSha, "git-sha", { at: now() });
    logEvent(wf, "info", "job_merged", { attempt: id, job: job.key, mergedSha });
    return after;
  });
}
