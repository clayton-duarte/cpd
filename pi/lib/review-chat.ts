import type { Host } from "./host.ts";
import { boldPhi } from "./phi.ts";
import { reviewNumber } from "./plan-review.ts";
import type { Run } from "./run.ts";

// The dialog of a Plan review in chat (commands.md §15.5 "The dialog"). What it does with an answer is the caller's (`review.ts`): this module owns the loop and which dialogs are open.

export const APPROVE = "Approve";
export const CHANGE = "Change";
const OPTIONS = [APPROVE, CHANGE];

export const stillWaiting = (review: string): string => `Plan review ${review} is still waiting: /plan-review reopens it`;
export const NONE_WAITING = boldPhi("φ: no plan review is waiting in chat");

/** The open dialogs of each run, by review: one at a time per review; its controller aborts the select. An editor has no signal, so Change's editor is open without being abortable. */
const open = new WeakMap<Run, Map<string, AbortController>>();
const of = (r: Run): Map<string, AbortController> => {
  let m = open.get(r);
  if (!m) open.set(r, (m = new Map()));
  return m;
};

export const dialogOpen = (r: Run, review: string): boolean => of(r).has(review);

/** Closes the select of `review`, if open: it records nothing (§15.5 "Latest only", "Deciding elsewhere"). */
export const closeDialog = (r: Run, review: string): void => of(r).get(review)?.abort();

/** Closes the dialogs of the reviews older than `review`. */
export function closeOlderDialogs(r: Run, review: string): void {
  for (const [known, controller] of of(r)) if (reviewNumber(known) < reviewNumber(review)) controller.abort();
}

/** What the caller does with an answer. */
export type Answers = {
  decide(approved: boolean, feedback: string | undefined): Promise<void>;
};

/**
 * The dialog of a pending chat review: select; Approve and Change decide, Change with an empty note and a failed decision return to the select.
 * Esc records nothing and says how to reopen. An abort (a newer review, a decision elsewhere) or a review that is no longer pending ends it silently, whatever it is waiting for.
 */
export async function runDialog(host: Host, r: Run, review: string, title: string | undefined, answers: Answers): Promise<void> {
  const ui = host.ctx?.ui;
  if (!ui || dialogOpen(r, review)) return;
  const controller = new AbortController();
  of(r).set(review, controller);
  const waiting = () => host.run === r && !controller.signal.aborted && r.reviews.get(review)?.status === "pending";
  try {
    while (waiting()) {
      const choice = await r.facts.ownPrompt(() => ui.select(title === undefined ? `Plan review ${review}` : `Plan review ${review}: ${title}`, OPTIONS, { signal: controller.signal }), controller.signal);
      if (!waiting()) return;
      if (choice === undefined) {
        ui.notify(stillWaiting(review));
        return;
      }
      if (choice === APPROVE) await answers.decide(true, undefined);
      else if (choice === CHANGE) {
        const note = await r.facts.ownPrompt(() => ui.editor("What should change?"), controller.signal);
        if (!waiting()) return; // decided elsewhere while the editor was open: its late note is ignored
        if (note === undefined || note.trim() === "") continue;
        await answers.decide(false, note);
      }
    }
  } finally {
    if (of(r).get(review) === controller) of(r).delete(review);
  }
}
