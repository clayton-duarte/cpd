import { z } from "zod";

/** The adapter's own session entry (`pi.appendEntry`): it survives `/reload` and resume, so pending reviews can be recovered. */
export const REVIEW_ENTRY_TYPE = "cpd-plan-review";

export const REVIEW_STATUSES = ["pending", "approved", "rejected", "lost"] as const;
export type ReviewStatus = (typeof REVIEW_STATUSES)[number];

export const ReviewSchema = z.strictObject({
  reviewId: z.string().min(1),
  wf: z.string().min(1),
  status: z.enum(REVIEW_STATUSES),
});
export type Review = z.infer<typeof ReviewSchema>;

/** Bookkeeping of the plan reviews this Lead session asked for. Every transition leaves `pending` exactly once. */
export function createReviews(initial: readonly Review[] = []) {
  const byId = new Map<string, Review>(initial.map((r) => [r.reviewId, r]));
  const settleTo = (reviewId: string, status: ReviewStatus): Review | null => {
    const r = byId.get(reviewId);
    if (!r || r.status !== "pending") return null;
    const next = { ...r, status };
    byId.set(reviewId, next);
    return next;
  };
  return {
    markPending(reviewId: string, wf: string): Review {
      const r = ReviewSchema.parse({ reviewId, wf, status: "pending" });
      byId.set(reviewId, r);
      return r;
    },
    /** The settled review, or null when the id is unknown or already settled (a duplicate result or poll). */
    settle: (reviewId: string, approved: boolean) => settleTo(reviewId, approved ? "approved" : "rejected"),
    lose: (reviewId: string) => settleTo(reviewId, "lost"),
    get: (reviewId: string) => byId.get(reviewId),
    pending: () => [...byId.values()].filter((r) => r.status === "pending"),
  };
}
export type Reviews = ReturnType<typeof createReviews>;

/** Fold the session's entries (oldest first, later wins) into the reviews they record. Our own entries must be valid. */
export function reviewsFromEntries(entries: readonly unknown[]): Review[] {
  const byId = new Map<string, Review>();
  for (const e of entries) {
    const entry = z.looseObject({ type: z.string(), customType: z.string().optional(), data: z.unknown().optional() }).safeParse(e);
    if (!entry.success || entry.data.type !== "custom" || entry.data.customType !== REVIEW_ENTRY_TYPE) continue;
    const r = ReviewSchema.parse(entry.data.data);
    byId.set(r.reviewId, r);
  }
  return [...byId.values()];
}

// --- plannotator's event API (t4; not a stable contract, so parsed at the boundary) ---

const Pending = z.looseObject({ status: z.literal("pending"), reviewId: z.string().min(1) });
const Respond = z.discriminatedUnion("status", [
  z.looseObject({ status: z.literal("handled"), result: z.unknown() }),
  z.looseObject({ status: z.literal("unavailable"), error: z.unknown().optional() }),
  z.looseObject({ status: z.literal("error"), error: z.unknown().optional() }),
]);

/** The `respond` payload of a `plan-review` request: the new review's id. Anything else throws. */
export function parseRequested(response: unknown): string {
  const r = Respond.parse(response);
  if (r.status !== "handled") throw new Error(`plannotator ${r.status}: ${JSON.stringify(r.error ?? null)}`);
  return Pending.parse(r.result).reviewId;
}

export const ResultEvent = z.looseObject({ reviewId: z.string().min(1), approved: z.boolean(), feedback: z.string().optional() });
export type ResultEvent = z.infer<typeof ResultEvent>;

const StatusResult = z.discriminatedUnion("status", [
  z.looseObject({ status: z.literal("pending") }),
  z.looseObject({ status: z.literal("completed"), approved: z.boolean(), feedback: z.string().optional() }),
  z.looseObject({ status: z.literal("missing") }),
]);
export type ReviewStatusResult = z.infer<typeof StatusResult>;

/** The `respond` payload of a `review-status` request. */
export function parseStatus(response: unknown): ReviewStatusResult {
  const r = Respond.parse(response);
  if (r.status !== "handled") throw new Error(`plannotator ${r.status}: ${JSON.stringify(r.error ?? null)}`);
  return StatusResult.parse(r.result);
}

// --- what the Lead is told ---

/** The decision message for the Lead. `failure` is set when recording it with `cpd plan decision` failed. */
export function decisionText(review: Review, feedback: string | undefined, failure?: string): string {
  const verdict = review.status === "approved" ? "APPROVED" : "REJECTED";
  const lines = [`Plan review ${review.reviewId} was ${verdict}.`];
  if (feedback !== undefined && feedback !== "") lines.push("Feedback from the reviewer:", feedback);
  if (failure !== undefined) {
    lines.push(`cpd could not record this decision: ${failure}`);
  } else if (review.status === "approved") {
    lines.push(`Next: cpd plan lock --wf ${review.wf}`);
  } else {
    lines.push("Revise the plan (append, cancel or retry Jobs), then call cpd_plan_review again.");
  }
  return lines.join("\n");
}

export function lostText(review: Review): string {
  return `Plan review ${review.reviewId} is gone (plannotator no longer knows it, for example after a restart), so no decision was recorded. Call cpd_plan_review again.`;
}
