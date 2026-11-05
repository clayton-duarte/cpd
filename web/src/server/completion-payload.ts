import type { SessionRef, TranscriptItem } from "../protocol/domain.ts";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { CompletionPayload } from "./completion-classify.ts";

export function ledgerActionsOf(_items: readonly TranscriptItem[]): string[] {
  return [];
}

export function buildPayload(_input: { ref: SessionRef; hasOpenDialog: boolean; plan: PlanViewModel | null; items: readonly TranscriptItem[] }): CompletionPayload {
  throw new Error("not implemented");
}
