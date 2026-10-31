// Contract §6.18 "The compaction ratio": the live `settings.compaction` getter the submit path consults.
// This is a stub: the test Job types the exports, the implementation Job fills the body.

/** The `compaction` block pi reads for a Session (contract §6.18). */
export type CompactionSettings = {
  enabled?: boolean;
  reserveTokens?: number;
  reserveTokensRatio?: number;
  keepRecentTokens?: number;
  backgroundTokens?: number;
  modelOverrides?: Record<string, { reserveTokens?: number; reserveTokensRatio?: number } | undefined>;
};

/** What `compactionPolicy` reads: pi's settings, the cached AgentState model and the models() window (contract §6.18). */
export type CompactionSource = {
  settings(): CompactionSettings;
  agent(): { provider: string; modelId: string } | undefined;
  windowOf(provider: string, modelId: string): number | undefined;
};

/** The live `settings.compaction` object: a getter, so every compaction decision derives the reserve at that moment. */
export function compactionPolicy(source: CompactionSource): CompactionSettings {
  throw new Error("not implemented");
}
