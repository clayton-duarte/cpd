import { mkdir, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { CpdError } from "../lib/cpd.ts";

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

type Assistant = {
  stopReason: string;
  errorMessage?: string;
  provider: string;
  model: string;
  usage: { input: number; output: number; cacheRead: number; cacheWrite: number; cost: { total: number } };
};

export function endReport(entries: readonly Entry[]): EndReport {
  const messages = entries.filter((e) => e.kind === "pi.assistant").map((e) => e.model?.[0] as Assistant);
  const last = messages.at(-1);
  if (last === undefined) throw new CpdError("no_assistant_entry", "the conversation has no assistant entry");
  const sum = (f: (m: Assistant) => number) => messages.reduce((n, m) => n + f(m), 0);
  return {
    stop_reason: last.stopReason,
    error_message: last.errorMessage ?? null,
    turns: messages.length,
    tokens_in: sum((m) => m.usage.input),
    tokens_out: sum((m) => m.usage.output),
    cache_read: sum((m) => m.usage.cacheRead),
    cache_write: sum((m) => m.usage.cacheWrite),
    cost_usd: sum((m) => m.usage.cost.total),
    model: `${last.provider}/${last.model}`,
  };
}

/** Writes the end report atomically (contract §6.12): creates the directory, writes a temp file, renames it over `file`. */
export async function writeReport(file: string, report: EndReport): Promise<void> {
  await mkdir(dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  await writeFile(temp, JSON.stringify(report));
  await rename(temp, file);
}
