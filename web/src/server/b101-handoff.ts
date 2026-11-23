import type { DedupStore, DedupStorage } from "./hub/dedup.ts";

/** A staged candidate has no production receipt store until ownership transfers. */
export interface HandoffCandidate {
  transfer(drainAndDispose: () => Promise<void>): Promise<DedupStore>;
}

export function prepareHandoff(_storage: DedupStorage): HandoffCandidate {
  throw new Error("safe handoff is not implemented");
}
