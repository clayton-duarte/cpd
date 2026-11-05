import type { SessionRef } from "../protocol/domain.ts";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { SessionServerMessage } from "../protocol/wire.ts";
import type { CompletionClassifier } from "./completion-classify.ts";

export interface CompletionTracker { observe(m: SessionServerMessage): void; dispose(): void }

export function createCompletionService(_deps: { classifier: CompletionClassifier; planOf: (ref: SessionRef) => Promise<PlanViewModel | null> }): { track(ref: SessionRef, send: (m: SessionServerMessage) => void): CompletionTracker } {
  return { track: () => ({ observe: () => undefined, dispose: () => undefined }) };
}
