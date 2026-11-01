// Contract §6.18 "The compaction ratio": the live `settings.compaction` getter the submit path consults.
import { reserveTokensOf } from "../lib/compaction-ratio.ts";

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

/**
 * The live `settings.compaction` object: a getter, so every compaction decision derives the reserve at that moment.
 * `reserveTokens` is the derivation over the settings and the model `agent()` names with `windowOf`'s window; the
 * other fields come from the same `settings()` read. A model with no positive window keeps the settings' own value.
 */
export function compactionPolicy(source: CompactionSource): CompactionSettings {
  const settings = source.settings();
  const agent = source.agent();
  const window = agent === undefined ? undefined : source.windowOf(agent.provider, agent.modelId);
  const reserve = reserveTokensOf(settings, agent === undefined ? undefined : { ...agent, ...(window === undefined ? {} : { contextWindow: window }) });
  return reserve === undefined ? { ...settings } : { ...settings, reserveTokens: reserve };
}
