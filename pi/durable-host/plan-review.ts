import type { EventOf } from "../lib/event.ts";
import type { ReviewEntry } from "../lib/plan-review.ts";
import type * as held from "./held.ts";
import type { Dialogs } from "./dialogs.ts";

// Contract §6.15 `plan-review.ts`: the plan-review flow (open/decide/decidedElsewhere/closeSuperseded) over the
// dialogs and the held map, porting pi/lib/review.ts and review-chat.ts. The types are real; the bodies are P1.6's,
// not this Job.

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

export function createPlanReview(port: PlanReviewPort): PlanReview {
  throw new Error("not implemented");
}