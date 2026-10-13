import { z } from "zod";
import { failure, refused, usage } from "../lib/errors.ts";
import { parseTier } from "../lib/tiers.ts";
import { logEvent } from "./audit.ts";
import { metadataArgs, readLedger, withLedger, workflowIssue, type ReadLedger } from "./bd.ts";
import { canonicalJson, fingerprintOfIssue, sha256, writeFingerprints } from "./fingerprint.ts";
import { findCycle, loadJobs, rawOf, type Job } from "./jobs.ts";
import { getPhaseIn, setPhaseIn } from "./workflow.ts";

// Contract §11 `plan decision` and the Ledger part of `plan lock`.

/** sha256 of the canonical JSON of all `draft` Jobs (key order, then every Spec field, title and needs). */
export function planFingerprintOf(jobs: Job[]): string {
  const drafts = jobs
    .filter((j) => j.status === "draft")
    .map((j) => ({
      key: j.key,
      title: j.title,
      goal: j.goal,
      tier: j.tier,
      review_tier: j.reviewTier,
      context: j.context,
      touches: j.touches,
      acceptance: j.acceptance,
      out_of_scope: j.outOfScope,
      needs: [...j.needs].sort(),
    }))
    .sort((a, b) => a.key.localeCompare(b.key));
  return sha256(canonicalJson(drafts));
}

export async function planFingerprint(wf: string): Promise<string> {
  return planFingerprintOf((await loadJobs(readLedger(wf))).jobs);
}

// --- decisions ---

export const DECISION_STATES = ["pending", "approved", "rejected"] as const;
export type DecisionState = (typeof DECISION_STATES)[number];

const REVIEW_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
const KEY_PREFIX = "plan_review_";

/** Workflow metadata `plan_review_<id>`. `at` orders reviews (contract §11 does not name it; added so "latest" is defined). */
const PlanReview = z.strictObject({
  fingerprint: z.string(),
  status: z.enum(DECISION_STATES),
  approved: z.literal(true).optional(),
  feedback: z.string().optional(),
  at: z.string(),
});
export type PlanReview = z.infer<typeof PlanReview>;

function parseReviews(metadata: Record<string, unknown>): Map<string, PlanReview> {
  const out = new Map<string, PlanReview>();
  for (const [k, v] of Object.entries(metadata)) {
    if (!k.startsWith(KEY_PREFIX)) continue;
    const parsed = PlanReview.safeParse(v);
    if (!parsed.success) throw failure("ledger_invalid", `workflow metadata ${k} does not have the expected shape`, { key: k, issues: parsed.error.issues });
    out.set(k.slice(KEY_PREFIX.length), parsed.data);
  }
  return out;
}

/**
 * Record a review state. Write-once per state: `pending` then `approved` or `rejected` is allowed, the same
 * state twice, or any change after `approved`/`rejected`, is refused. `approved`/`rejected` bind to the fingerprint
 * seen at `pending` (what the reviewer was shown); without a `pending`, to the current one.
 */
export async function recordPlanDecision(wf: string, reviewId: string, state: DecisionState, feedback?: string): Promise<PlanReview> {
  if (!REVIEW_ID.test(reviewId)) throw usage("invalid_review_id", `invalid review id ${JSON.stringify(reviewId)} (must match ${REVIEW_ID.source})`);
  if (!DECISION_STATES.includes(state)) throw usage("invalid_decision", `invalid decision ${JSON.stringify(state)}`);
  return withLedger(wf, `plan decision ${reviewId}`, async (l) => {
    const phase = await getPhaseIn(l);
    if (phase !== "plan") throw refused("illegal_phase", `plan decisions are only recorded in phase plan, not ${phase}`, { phase });
    const issue = await workflowIssue(l);
    const existing = parseReviews(issue.metadata ?? {}).get(reviewId);
    if (existing && (existing.status === state || existing.status !== "pending")) {
      throw refused("decision_recorded", `review ${reviewId} already has state ${existing.status}`, { reviewId, status: existing.status });
    }
    const fingerprint = existing?.fingerprint ?? planFingerprintOf((await loadJobs(l)).jobs);
    const record: PlanReview = {
      fingerprint,
      status: state,
      ...(state === "approved" ? { approved: true as const } : {}),
      ...(feedback === undefined ? {} : { feedback }),
      at: new Date().toISOString(),
    };
    await l.write(["update", issue.id, ...metadataArgs(issue.metadata, { [KEY_PREFIX + reviewId]: record })]);
    logEvent(wf, "info", "plan_decision", { reviewId, state, fingerprint });
    return record;
  });
}

/** The fingerprint the latest plan review approved, or null when the latest review is not an approval (or there is none). */
export async function latestApprovedFingerprint(wf: string, l: ReadLedger = readLedger(wf)): Promise<string | null> {
  const reviews = [...parseReviews((await workflowIssue(l)).metadata ?? {}).values()].sort((a, b) => a.at.localeCompare(b.at));
  const latest = reviews.at(-1);
  return latest?.status === "approved" ? latest.fingerprint : null;
}

// --- lock ---

/**
 * The Ledger part of `plan lock`: approval for the current fingerprint, valid DAG, fingerprints written,
 * every draft `queued` in one `bd batch`, phase `dispatch`. All checks run before the first write.
 */
export function lockPlan(wf: string): Promise<{ fingerprint: string; jobs: string[] }> {
  return withLedger(wf, "plan lock", async (l) => {
    const phase = await getPhaseIn(l);
    if (phase !== "plan") throw refused("illegal_phase", `plan lock needs phase plan, not ${phase}`, { phase });
    const loaded = await loadJobs(l);
    const drafts = loaded.jobs.filter((j) => j.status === "draft");
    if (drafts.length === 0) throw refused("no_jobs", "the plan has no draft Jobs to lock");

    const fingerprint = planFingerprintOf(loaded.jobs);
    const approved = await latestApprovedFingerprint(wf, l);
    if (approved !== fingerprint) {
      throw refused("plan_not_approved", "the latest plan review is not an approval of the current plan", { current: fingerprint, approved });
    }

    for (const j of drafts) {
      parseTier(j.tier);
      parseTier(j.reviewTier);
      for (const n of j.needs) {
        const dep = loaded.jobs.find((d) => d.key === n);
        if (!dep || dep.status === "cancelled" || dep.status === "failed") {
          throw refused("needs_unavailable", `Job ${j.key} needs ${n}, which is ${dep?.status ?? "missing"}`, { key: j.key, needs: n });
        }
      }
    }
    const cycle = findCycle(new Map(loaded.jobs.map((j) => [j.key, j.needs] as [string, string[]])));
    if (cycle) throw refused("dependency_cycle", `dependency cycle: ${cycle.join(" -> ")}`, { cycle });

    writeFingerprints(
      wf,
      Object.fromEntries(drafts.map((j) => [j.key, fingerprintOfIssue(rawOf(loaded, j), loaded.keys)])),
    );
    await l.write(["batch"], undefined, drafts.map((j) => `update ${j.id} status=queued\n`).join(""));
    await setPhaseIn(l, "dispatch", { dispatch_started_at: new Date().toISOString() });
    logEvent(wf, "info", "plan_locked", { jobs: drafts.map((j) => j.key), fingerprint });
    return { fingerprint, jobs: drafts.map((j) => j.key) };
  });
}
