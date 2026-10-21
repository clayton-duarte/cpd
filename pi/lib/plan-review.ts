import { isObject } from "./cpd.ts";

/** The adapter's own session entry (`pi.appendEntry`): it survives `/reload` and resume (commands.md §15.5, "Opening" step 4). */
export const ENTRY_TYPE = "cpd-plan-review";
export const DECISION_TYPE = "cpd-plan-decision";
/** The summary message of a Plan review in chat (§15.5 "Opening" step 4). */
export const SUMMARY_TYPE = "cpd-plan-summary";
export const REVIEW_LABEL = "cpd: plan review";

export type EntryStatus = "pending" | "approved" | "rejected" | "lost";
/** Where the User decides: `chat` (the dialog) or plannotator, which alone has a page id. An entry without `surface` is a plannotator one (written before the chat review existed); it keeps that shape. */
export type ReviewEntry =
  | { plan: string; review: string; surface: "chat"; status: EntryStatus }
  | { plan: string; review: string; surface?: "plannotator"; plannotator_id: string; status: EntryStatus };

/** The plannotator page id of an entry, or undefined for a chat entry. */
export const plannotatorId = (e: ReviewEntry): string | undefined => (e.surface === "chat" ? undefined : e.plannotator_id);

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
    if (!isObject(d) || typeof d.plan !== "string" || typeof d.review !== "string" || typeof d.status !== "string" || !STATUSES.includes(d.status)) {
      invalid += 1;
      continue;
    }
    const status = d.status as EntryStatus;
    let entry: ReviewEntry;
    if (d.surface === "chat") entry = { plan: d.plan, review: d.review, surface: "chat", status };
    else if ((d.surface === undefined || d.surface === "plannotator") && typeof d.plannotator_id === "string") {
      entry = { plan: d.plan, review: d.review, ...(d.surface === "plannotator" ? { surface: "plannotator" as const } : {}), plannotator_id: d.plannotator_id, status };
    } else {
      invalid += 1;
      continue;
    }
    if (d.plan === plan) reviews.set(d.review, entry);
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

/** The last line of a chat entry's "decided elsewhere" message: there is no plannotator page to leave open. */
export const ELSEWHERE_CHAT_NOTE = "This review was decided outside this adapter.";

/** cpd 1's `decisionText`, kept (commands.md §15.5, "Deciding" step 3). `elsewhere` is the note to end with, when the review was decided by another caller. */
export function decisionText(review: string, approved: boolean, feedback: string | undefined, current: boolean, elsewhere: string | undefined = undefined): string {
  const lines = [`Plan review ${review} was ${approved ? "APPROVED" : "REJECTED"}.`];
  if (feedback !== undefined && feedback !== "") lines.push("Feedback from the reviewer:", feedback);
  if (!current) lines.push("The Jobs changed since that review. Call cpd_plan_review again for a new review.");
  else if (approved) lines.push("Next: cpd plan lock");
  else lines.push("Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.");
  if (elsewhere !== undefined) lines.push(elsewhere);
  return lines.join("\n");
}

export const lostText = (review: string): string =>
  `Plan review ${review} is gone (plannotator no longer knows it, for example after a restart), so no decision was recorded. Call cpd_plan_review again.`;
