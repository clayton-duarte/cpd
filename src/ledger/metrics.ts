import { z } from "zod";
import { usage } from "../lib/errors.ts";
import { logEvent } from "./audit.ts";
import { metadataArgs, withLedger } from "./bd.ts";
import { findAttempt } from "./attempts.ts";

// Contract §14: per-Attempt metrics as flat metadata keys, written once.

const Count = z.number().finite().nonnegative();
/** The numeric part matches `SessionMetrics` (src/metrics/session.ts); extra fields are ignored. */
export const Metrics = z.object({
  turns: Count,
  tokensIn: Count,
  tokensOut: Count,
  cacheRead: Count,
  cacheWrite: Count,
  costUsd: Count,
  durationS: Count,
  model: z.string().nullish(),
  thinkingLevel: z.string().nullish(),
  finalStopReason: z.string().nullish(),
});
export type Metrics = z.input<typeof Metrics>;

export type MetricsPrefix = "metrics" | "review_metrics";

/** The metadata keys for `m`, e.g. `metrics_tokens_in`. Null strings are left out. */
export function metricKeys(m: z.output<typeof Metrics>, prefix: MetricsPrefix): Record<string, number | string> {
  const k = (name: string) => `${prefix}_${name}`;
  return {
    [k("turns")]: m.turns,
    [k("tokens_in")]: m.tokensIn,
    [k("tokens_out")]: m.tokensOut,
    [k("cache_read")]: m.cacheRead,
    [k("cache_write")]: m.cacheWrite,
    [k("cost_usd")]: m.costUsd,
    [k("duration_s")]: m.durationS,
    ...(m.model ? { [k("model")]: m.model } : {}),
    ...(m.thinkingLevel ? { [k("thinking_level")]: m.thinkingLevel } : {}),
    ...(m.finalStopReason ? { [k("stop_reason")]: m.finalStopReason } : {}),
  };
}

/** Write the Attempt's metrics (`metrics_*` for the Worker, `review_metrics_*` for the Reviewer). Write-once: a second call is `metadata_write_once`, and nothing is written. */
export async function recordMetrics(wf: string, attemptId: string, metrics: Metrics, opts: { prefix: MetricsPrefix }): Promise<void> {
  if (opts.prefix !== "metrics" && opts.prefix !== "review_metrics") throw usage("invalid_prefix", `invalid metrics prefix ${JSON.stringify(opts.prefix)}`);
  const parsed = Metrics.safeParse(metrics);
  if (!parsed.success) throw usage("invalid_metrics", "invalid metrics", { issues: parsed.error.issues });
  await withLedger(wf, `metrics ${attemptId}`, async (l) => {
    const { attempt } = await findAttempt(l, attemptId);
    await l.write(["update", attempt.issue, ...metadataArgs(attempt.metadata, metricKeys(parsed.data, opts.prefix), true)]);
    logEvent(wf, "info", "metrics_recorded", { attempt: attemptId, prefix: opts.prefix });
  });
}
