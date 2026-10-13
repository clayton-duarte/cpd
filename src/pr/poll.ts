import { closeSync, openSync } from "node:fs";
import { join } from "node:path";
import { logEvent } from "../ledger/audit.ts";
import { getPhase, readWorkflow, setPhase } from "../ledger/workflow.ts";
import { refused } from "../lib/errors.ts";
import { ledgerDir } from "../lib/paths.ts";
import { notifyLead } from "../queue/lead-queue.ts";
import { repoSlug, reviewComments, viewPr, type PrView, type ReviewComment } from "./gh.ts";
import { readPrState, writePrState, type PrState } from "./state.ts";

// Contract §11 `pr poll` and §13.

export const BODY_MAX = 2000;
const FAILING = new Set(["FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"]);
const isFailing = (conclusion: string): boolean => FAILING.has(conclusion.toUpperCase());
const trunc = (s: string | null | undefined): string => (s ?? "").slice(0, BODY_MAX);

export type Change =
  | { type: "check"; name: string; conclusion: string }
  | { type: "review"; id: string; author: string | null; state: string; body: string }
  | { type: "comment"; id: string; author: string | null; path: string | null; line: number | null; body: string };

/** Pure: what is new in `view` and `inline` relative to `prev` (null = never polled), and the state to store. */
export function diffPr(prev: PrState | null, number: number, view: PrView, inline: ReviewComment[], now: string): { changes: Change[]; next: PrState } {
  const checks: Record<string, string> = {};
  for (const c of view.statusCheckRollup) {
    const name = c.name ?? c.context;
    if (name === undefined) continue;
    checks[name] = c.conclusion ?? c.state ?? "";
  }
  const changes: Change[] = [];
  for (const [name, conclusion] of Object.entries(checks)) {
    const was = prev?.checks[name];
    if (isFailing(conclusion) && !(was !== undefined && isFailing(was))) changes.push({ type: "check", name, conclusion });
  }
  const seenReviews = new Set(prev?.reviewIds);
  for (const r of view.reviews) {
    if (!seenReviews.has(r.id)) changes.push({ type: "review", id: r.id, author: r.author?.login ?? null, state: r.state, body: trunc(r.body) });
  }
  const seenComments = new Set(prev?.commentIds);
  for (const c of view.comments) {
    if (!seenComments.has(c.id)) changes.push({ type: "comment", id: c.id, author: c.author?.login ?? null, path: null, line: null, body: trunc(c.body) });
  }
  for (const c of inline) {
    if (!seenComments.has(c.id)) {
      changes.push({ type: "comment", id: c.id, author: c.user?.login ?? null, path: c.path ?? null, line: c.line ?? null, body: trunc(c.body) });
    }
  }
  const union = (old: string[] | undefined, ids: string[]) => [...new Set([...(old ?? []), ...ids])];
  return {
    changes,
    next: {
      v: 1,
      number,
      lastPollAt: now,
      checks,
      reviewIds: union(prev?.reviewIds, view.reviews.map((r) => r.id)),
      commentIds: union(prev?.commentIds, [...view.comments.map((c) => c.id), ...inline.map((c) => c.id)]),
    },
  };
}

const EXCERPT_MAX = 300;
const LISTED_MAX = 20;

const excerpt = (s: string): string => {
  const one = s.replace(/\s+/g, " ").trim();
  return one.length > EXCERPT_MAX ? `${one.slice(0, EXCERPT_MAX)}…` : one;
};

function describe(c: Change): string {
  switch (c.type) {
    case "check":
      return `check ${c.name}: ${c.conclusion}`;
    case "review":
      return `review ${c.id} by ${c.author ?? "unknown"} (${c.state}): ${excerpt(c.body)}`;
    case "comment":
      return `comment ${c.id} by ${c.author ?? "unknown"} at ${c.path ?? "(conversation)"}${c.line === null ? "" : `:${c.line}`}: ${excerpt(c.body)}`;
  }
}

/** Every change as one line (the Lead sees only `text`), capped at LISTED_MAX lines; `data` always carries all of them in full. */
export function feedbackText(wf: string, number: number, changes: Change[]): string {
  const shown = changes.slice(0, LISTED_MAX).map((c) => `- ${describe(c)}`);
  const more = changes.length > LISTED_MAX ? [`- …and ${changes.length - LISTED_MAX} more (see data)`] : [];
  return [
    `PR #${number} has new feedback:`,
    ...shown,
    ...more,
    `Turn what needs work into Jobs: cpd job add --wf ${wf} --key <key> --title <title> --tier <T> --spec <file.json>`,
  ].join("\n");
}

export type PollResult = { changes: Change[]; merged: boolean };

/** Contract §11 `pr poll`. Messages are at-least-once: state is written after the message, so a crash in between re-sends. */
export async function pollPr(wf: string): Promise<PollResult> {
  const w = readWorkflow(wf);
  const phase = await getPhase(wf);
  if (phase !== "pr") throw refused("illegal_phase", `pr poll needs phase pr, not ${phase}`, { phase });
  if (w.pr === null) throw refused("no_pr", `Workflow ${wf} has no PR`, { wf });
  const { number } = w.pr;

  const view = await viewPr(w.repo, number);
  if (view.state === "MERGED") {
    await setPhase(wf, "done"); // before the freeze: a frozen Ledger refuses writes
    closeSync(openSync(join(ledgerDir(wf), "MIGRATION-FREEZE"), "a"));
    notifyLead(wf, { kind: "workflow-done", text: `PR #${number} is merged; Workflow ${wf} is done.`, data: { number, url: w.pr.url, mergedAt: view.mergedAt } });
    logEvent(wf, "info", "pr_merged", { number });
    return { changes: [], merged: true };
  }

  const inline = await reviewComments(w.repo, await repoSlug(w.repo), number);
  const { changes, next } = diffPr(readPrState(wf), number, view, inline, new Date().toISOString());
  if (changes.length > 0) {
    notifyLead(wf, {
      kind: "pr-feedback",
      text: feedbackText(wf, number, changes),
      data: {
        number,
        checks: changes.filter((c) => c.type === "check"),
        reviews: changes.filter((c) => c.type === "review"),
        comments: changes.filter((c) => c.type === "comment"),
      },
    });
  }
  await writePrState(wf, next);
  return { changes, merged: false };
}
