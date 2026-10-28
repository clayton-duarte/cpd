export type EndReport = {
  stop_reason: string;
  error_message: string | null;
  turns: number;
  tokens_in: number;
  tokens_out: number;
  cache_read: number;
  cache_write: number;
  cost_usd: number;
  model: string | null;
};

export type Entry = { kind: string; model?: readonly unknown[] };

export function endReport(_entries: readonly Entry[]): EndReport {
  throw new Error("not implemented");
}
