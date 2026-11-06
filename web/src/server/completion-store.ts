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

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

export function createCompletionStore(deps: { cpd: CpdRunner }): CompletionStore {
  return {
    async get(sessionId) {
      const reply = await deps.cpd(["swr", "badge", "get", sessionId]);
      const badge = isRecord(reply) ? reply["badge"] : null;
      if (!isRecord(badge)) return null;
      const completion = {
        label: badge["label"],
        text: badge["text"],
        score: badge["score"],
        tone: badge["tone"],
        ...(Array.isArray(badge["others"]) ? { others: badge["others"] } : {}),
      } as Completion;
      return { entryId: String(badge["entry_id"]), completion };
    },
    async put(sessionId, entryId, c) {
      const others = c.others !== undefined && c.others.length > 0 ? ["--others", JSON.stringify(c.others)] : [];
      await deps.cpd(["swr", "badge", "put", sessionId, "--entry", entryId, "--label", c.label, "--score", String(c.score), "--text", c.text, "--tone", c.tone, ...others]);
    },
  };
}
