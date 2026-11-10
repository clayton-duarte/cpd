export interface ModelCostOptions {
  json?: boolean;
  root?: string;
  durableRoot?: string;
  bifrostCsv?: string;
  since?: string;
  until?: string;
  diffPrefix?: boolean;
}

export interface SpendGroup {
  turns: number;
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  prompt_tokens: number;
  cache_hit_rate: number | null;
  cost_usd: number;
}

export interface CacheBreakDiff {
  scope: "messages" | "system+messages";
  prefix_changed: boolean;
  index: number;
  char: number;
  tokens_from: number;
  prev_tokens_to: number;
  cur_tokens_to: number;
}

export interface CacheBreak {
  source: string;
  session: string;
  time: string;
  model: string;
  role: string;
  class: "to_zero" | "partial";
  prev_cache_read: number;
  cache_read: number;
  gap_s: number;
  idle_gap: boolean;
  cause: string | null;
  diff?: CacheBreakDiff;
}

export interface ReconciliationRow {
  utc_day: string;
  model: string;
  local_cost_usd: number;
  bifrost_cost_usd: number;
  delta_usd: number;
  ratio: number | null;
  local_cache_read: number;
  bifrost_cached_read: number;
}

export interface ModelCostReport {
  window: { since: string | null; until: string | null };
  files: { pi: number; durable: number; bifrost: number };
  skipped_lines: number;
  zero_usage_turns: number;
  totals: SpendGroup;
  by_role: {
    worker: SpendGroup;
    manager: SpendGroup;
    lead: SpendGroup;
    other: SpendGroup;
  };
  by_model: Record<string, SpendGroup>;
  by_day: Record<string, SpendGroup>;
  cache_breaks: CacheBreak[];
  reconciliation: ReconciliationRow[];
}

function emptySpendGroup(): SpendGroup {
  return {
    turns: 0,
    input: 0,
    output: 0,
    cache_read: 0,
    cache_write: 0,
    prompt_tokens: 0,
    cache_hit_rate: null,
    cost_usd: 0,
  };
}

export function report(opts: ModelCostOptions): ModelCostReport {
  return {
    window: { since: opts.since ?? null, until: opts.until ?? null },
    files: { pi: 0, durable: 0, bifrost: 0 },
    skipped_lines: 0,
    zero_usage_turns: 0,
    totals: emptySpendGroup(),
    by_role: {
      worker: emptySpendGroup(),
      manager: emptySpendGroup(),
      lead: emptySpendGroup(),
      other: emptySpendGroup(),
    },
    by_model: {},
    by_day: {},
    cache_breaks: [],
    reconciliation: [],
  };
}

export async function main(_argv: readonly string[]): Promise<number> {
  return 0;
}
