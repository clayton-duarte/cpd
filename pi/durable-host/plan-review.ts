// Contract §6.15 `plan-review.ts`: the plan-review flow (open/decide/decidedElsewhere/closeSuperseded) over the
// dialogs and the held map, porting pi/lib/review.ts and review-chat.ts. The pure questions of pi/lib/plan-review.ts
// (`reviewsFromEntries`, `reviewNumber`) and the `decisionText` are imported, not copied.
import type { EventOf } from "../lib/event.ts";
import { isObject, toCpdError } from "../lib/cpd.ts";
import {
  DECISION_TYPE,
  decisionText,
  ELSEWHERE_CHAT_NOTE,
  ENTRY_TYPE,
  reviewNumber,
  reviewsFromEntries,
  SUMMARY_TYPE,
  type ReviewEntry,
} from "../lib/plan-review.ts";
import type * as held from "./held.ts";
import type { Dialogs } from "./dialogs.ts";

export type PlanReviewPort = {
  plan: string;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  readText(file: string): Promise<string>;
  write(entry: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }): Promise<unknown>;
  entries(): Promise<readonly unknown[]>;
  dialogs: Dialogs;
  held: held.HeldApi;
};

export type PlanReview = {
  open(review: string, via: "event" | "tool" | "start", summary?: string): Promise<void>;
  decide(entry: ReviewEntry, approved: boolean, feedback?: string): Promise<void>;
  decidedElsewhere(entry: ReviewEntry, ev: EventOf<"plan-review">): Promise<void>;
  closeSuperseded(entry: ReviewEntry): void;
};

const APPROVE = "Approve";
const CHANGE = "Change";
const OPTIONS = [APPROVE, CHANGE];
const SUMMARY_FILE = (review: string): string => `summary-${review}.md`;
const FEEDBACK_FILE = (review: string): string => `feedback-${review}.md`;
/** pi's notice when a cancelled select leaves the review pending (review-chat.ts `stillWaiting`). */
const stillWaiting = (review: string): string => `Plan review ${review} is still waiting: /plan-review reopens it`;

/** The open dialog of each review, module-wide (one host process), as review-chat.ts's per-run map: its select's abort controller. */
const openDialogs = new Map<string, AbortController>();

const entryData = (entry: ReviewEntry): { plan: string; review: string; surface: "chat"; status: ReviewEntry["status"] } => ({
  plan: entry.plan,
  review: entry.review,
  surface: "chat",
  status: entry.status,
});

export function createPlanReview(port: PlanReviewPort): PlanReview {
  const deciding = new Set<string>();

  /** Closes the dialog of `review`: its select resolves `undefined` (superseded) and its loop returns silently. */
  const abortDialog = (review: string): void => {
    const controller = openDialogs.get(review);
    if (controller) {
      controller.abort();
      openDialogs.delete(review);
    }
  };

  /** Records the User's decision with cpd (`cpd plan decide`), then tells the Lead (review.ts `decide`). A failure other than `review_decided` leaves the entry pending. */
  const decide = async (entry: ReviewEntry, approved: boolean, feedback?: string): Promise<void> => {
    if (entry.status !== "pending" || deciding.has(entry.review)) return;
    deciding.add(entry.review);
    let verdict = approved;
    let text = feedback;
    let current = true;
    try {
      const args = ["plan", "decide", entry.review, approved ? "--approve" : "--reject"];
      if (feedback !== undefined && feedback !== "") args.push("--feedback-file", FEEDBACK_FILE(entry.review));
      args.push("--plan", port.plan);
      try {
        const out = await port.cpd(args);
        current = out.current !== false;
      } catch (e) {
        if (toCpdError(e).code !== "review_decided") throw e;
        // Another caller decided it: re-read the review and continue with that decision; nothing else is retried.
        const shown = (await port.cpd(["plan", "show", "--plan", port.plan])).review;
        if (isObject(shown) && shown.id === entry.review && (shown.status === "approved" || shown.status === "rejected")) {
          verdict = shown.status === "approved";
          text = typeof shown.feedback === "string" ? shown.feedback : undefined;
          current = shown.current !== false;
        } else {
          current = false; // a newer review exists: this decision no longer counts
        }
      }
      await settle(entry, verdict, text, current, false);
    } finally {
      deciding.delete(entry.review);
    }
  };

  /** §15.5 "Deciding": the settled entry, the dialog closed, the Lead told (review.ts `settle`). */
  async function settle(entry: ReviewEntry, verdict: boolean, text: string | undefined, current: boolean, elsewhere: boolean): Promise<void> {
    const done: ReviewEntry = { ...entry, status: verdict ? "approved" : "rejected" };
    abortDialog(entry.review);
    await port.write({ kind: ENTRY_TYPE, data: entryData(done), model: [] });
    await port.write({
      kind: DECISION_TYPE,
      data: {
        plan: entry.plan,
        review: entry.review,
        approved: verdict,
        current,
        ...(text !== undefined ? { feedback: text } : {}),
        ...(elsewhere ? { decided_elsewhere: true } : {}),
      },
      model: [{ role: "user", content: decisionText(entry.review, verdict, text, current, elsewhere ? ELSEWHERE_CHAT_NOTE : undefined) }],
    });
  }

  /** The dialog of a pending chat review, detached (review.ts `showDialog`): select; Approve decides, Change opens the editor and decides with the note. A decline leaves the entry pending and sends the `stillWaiting` notice. */
  const runDialog = (entry: ReviewEntry): (() => Promise<void>) => {
    const review = entry.review;
    return async () => {
      const controller = openDialogs.get(review);
      if (controller === undefined) return;
      try {
        while (!controller.signal.aborted) {
          if (openDialogs.get(review) !== controller) return; // closed or replaced (4.4 rule 4): silent
          const choice = await port.dialogs.select("Approve the plan review", OPTIONS, { purpose: "plan_review", signal: controller.signal });
          if (openDialogs.get(review) !== controller) return; // decided elsewhere or superseded meanwhile: silent
          if (choice === undefined) {
            // A genuine decline: the review is still pending and nothing re-opened it (review-chat.ts `stillWaiting`).
            port.dialogs.notice("warning", stillWaiting(review));
            return;
          }
          if (choice === APPROVE) {
            await decide(entry, true, undefined);
            return;
          }
          const note = await port.dialogs.editor("What should change?");
          if (controller.signal.aborted) return; // decided elsewhere while the editor was open: the late note is ignored (review-chat.ts)
          if (note === undefined || note.trim() === "") continue; // an empty note returns to the select
          await decide(entry, false, note);
          return;
        }
      } finally {
        if (openDialogs.get(review) === controller) openDialogs.delete(review);
      }
    };
  };

  /** §15.5 "Latest only": a newer review closes this session's older pending ones (`closeSuperseded`); a reopen of the same review closes the previous select and says it still waits. */
  const supersede = (review: string, reviews: ReadonlyMap<string, ReviewEntry>): void => {
    for (const known of reviews.values()) {
      if (known.status === "pending" && known.review !== review && isFinite(reviewNumber(known.review)) && isFinite(reviewNumber(review)) && reviewNumber(known.review) < reviewNumber(review)) {
        closeSuperseded(known);
      }
    }
    const same = openDialogs.get(review);
    if (same) {
      // A reopen of the same review closes the previous select (4.4 rule 6); its loop returns silently, and the
      // `stillWaiting` notice of the reopened review is open()'s own (the pending entry is in the store).
      same.abort();
      openDialogs.delete(review);
    }
  };

  /** §5.5 "Latest only": a `plan-review` Event with status `superseded` closes the reason and the dialog of a pending entry and records it. Nothing is sent. */
  const closeSuperseded = (entry: ReviewEntry): void => {
    if (entry.status !== "pending") return;
    const done: ReviewEntry = { ...entry, status: "superseded" };
    abortDialog(entry.review);
    void port.write({ kind: ENTRY_TYPE, data: entryData(done), model: [] }).catch(() => {});
  };

  return {
    /** §4.4 rule 6 and review.ts's flow: the summary and the pending entry, then the dialog. A `plan-review` Event reads the summary file through `readText`; the `cpd_plan_review` tool call passes it directly; the start hook reuses a pending entry that survived a restart. */
    async open(review, via, summary) {
      const { reviews } = reviewsFromEntries(await port.entries(), port.plan);
      supersede(review, reviews);
      let content = summary;
      if (content === undefined) content = await port.readText(SUMMARY_FILE(review));
      const pending = reviews.get(review);
      // A review that is already pending is reopened (its earlier select was cancelled/Esc'd, or the host restarted):
      // the entry stays pending and the User is told it still waits (review-chat.ts `stillWaiting`).
      const reuse = via === "start" && pending?.status === "pending";
      if (pending?.status === "pending") port.dialogs.notice("warning", stillWaiting(review));
      const entry: ReviewEntry = { plan: port.plan, review, surface: "chat", status: "pending" };
      await port.write({ kind: SUMMARY_TYPE, data: { plan: port.plan, review, display: true }, model: [{ role: "user", content }] });
      if (!reuse) await port.write({ kind: ENTRY_TYPE, data: entryData(entry), model: [] });
      const controller = new AbortController();
      openDialogs.set(review, controller);
      void runDialog(entry)().catch(() => {});
    },
    decide,
    /** §15.5 "Deciding elsewhere": a `plan-review` Event with status `approved` or `rejected` for a pending entry means another caller decided it; cpd already has the decision, so there is no `plan decide`. A failure leaves the entry pending. */
    async decidedElsewhere(entry, ev) {
      if (entry.status !== "pending" || deciding.has(entry.review)) return;
      deciding.add(entry.review);
      try {
        const verdict = ev.data.status === "approved";
        const shown = (await port.cpd(["plan", "show", "--plan", port.plan])).review;
        const current = isObject(shown) && shown.id === entry.review ? shown.current !== false : false;
        const feedback = typeof ev.data.feedback === "string" ? ev.data.feedback : isObject(shown) && shown.id === entry.review && typeof shown.feedback === "string" ? shown.feedback : undefined;
        await settle(entry, verdict, feedback, current, true);
      } finally {
        deciding.delete(entry.review);
      }
    },
    closeSuperseded,
  };
}
