import { isObject } from "./cpd.ts";

/** The adapter's own session entry (`pi.appendEntry`): it survives `/reload` and resume (commands.md §15.5, "Opening" step 4). */
export const ENTRY_TYPE = "cpd-plan-review";
export const DECISION_TYPE = "cpd-plan-decision";
export const REVIEW_LABEL = "cpd: plan review";

export type EntryStatus = "pending" | "approved" | "rejected" | "lost";
export type ReviewEntry = { plan: string; review: string; plannotator_id: string; status: EntryStatus };

const STATUSES: readonly string[] = ["pending", "approved", "rejected", "lost"];

/**
 * The reviews this session opened for `plan`, folded from its entries (oldest first, the later entry of a review wins).
 * Entries of another Plan are skipped: a fork's file starts with its source's entries. `invalid` counts our own entries that do not parse.
 */
export function reviewsFromEntries(entries: readonly unknown[], plan: string): { reviews: Map<string, ReviewEntry>; invalid: number } {
  const reviews = new Map<string, ReviewEntry>();
  let invalid = 0;
  for (const e of entries) {
    if (!isObject(e) || e.type !== "custom" || e.customType !== ENTRY_TYPE) continue;
    const d = e.data;
    if (!isObject(d) || typeof d.plan !== "string" || typeof d.review !== "string" || typeof d.plannotator_id !== "string" || typeof d.status !== "string" || !STATUSES.includes(d.status)) {
      invalid += 1;
      continue;
    }
    if (d.plan !== plan) continue;
    reviews.set(d.review, { plan: d.plan, review: d.review, plannotator_id: d.plannotator_id, status: d.status as EntryStatus });
  }
  return { reviews, invalid };
}

/** The `k` of a review id `r<k>` (contract §4.6); `NaN` for any other id, which then compares as neither older nor newer. */
export const reviewNumber = (review: string): number => (/^r\d+$/.test(review) ? Number(review.slice(1)) : Number.NaN);

/** The highest review id of `reviews`, or undefined when there is none. */
export function highestReview(reviews: Iterable<string>): string | undefined {
  let best: string | undefined;
  for (const r of reviews) if (best === undefined || reviewNumber(r) > reviewNumber(best)) best = r;
  return best;
}

/** plannotator's event API has no request that closes a review (`plannotator-events.ts` handles plan-mode, plan-review, review-status, code-review, annotate, annotate-last, archive), so the page stays open. */
export const ELSEWHERE_NOTE = "This review was decided outside plannotator. plannotator offers no way to close its review page from here, so the page stays open: ignore it or close the tab.";

/** cpd 1's `decisionText`, kept (commands.md §15.5, "Deciding" step 3). */
export function decisionText(review: string, approved: boolean, feedback: string | undefined, current: boolean, elsewhere = false): string {
  const lines = [`Plan review ${review} was ${approved ? "APPROVED" : "REJECTED"}.`];
  if (feedback !== undefined && feedback !== "") lines.push("Feedback from the reviewer:", feedback);
  if (!current) lines.push("The Jobs changed since that review. Call cpd_plan_review again for a new review.");
  else if (approved) lines.push("Next: cpd plan lock");
  else lines.push("Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.");
  if (elsewhere) lines.push(ELSEWHERE_NOTE);
  return lines.join("\n");
}

export const lostText = (review: string): string =>
  `Plan review ${review} is gone (plannotator no longer knows it, for example after a restart), so no decision was recorded. Call cpd_plan_review again.`;
