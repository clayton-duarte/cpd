import type { Completion } from "../protocol/domain.ts";

/** The `cpd` boundary: run `cpd <args>` and resolve the parsed JSON line it printed. Rejects on a non-zero exit. */
export type CpdRunner = (args: readonly string[]) => Promise<unknown>;

export interface StoredBadge { readonly entryId: string; readonly completion: Completion }

export interface CompletionStore {
  /** `cpd swr badge get <sessionId>`: the stored badge, or null on a miss. */
  get(sessionId: string): Promise<StoredBadge | null>;
  /** `cpd swr badge put ...`: persist the classification taken at `entryId`. */
  put(sessionId: string, entryId: string, completion: Completion): Promise<void>;
}

export function createCompletionStore(_deps: { cpd: CpdRunner }): CompletionStore {
  return {
    get: () => { throw new Error("not implemented"); },
    put: () => { throw new Error("not implemented"); },
  };
}
