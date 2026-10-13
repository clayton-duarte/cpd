import { readFileSync } from "node:fs";
import { z } from "zod";
import { failure } from "../lib/errors.ts";

/** Every entry carries a type and an ISO timestamp. */
const Base = z.object({ type: z.string(), timestamp: z.iso.datetime() });
const Header = Base.extend({ type: z.literal("session") });
const ModelChange = Base.extend({ type: z.literal("model_change"), modelId: z.string() });
const ThinkingChange = Base.extend({ type: z.literal("thinking_level_change"), thinkingLevel: z.string() });
const Usage = z.object({
  input: z.number(),
  output: z.number(),
  cacheRead: z.number(),
  cacheWrite: z.number(),
  cost: z.object({ total: z.number() }),
});
const AssistantMessage = Base.extend({
  type: z.literal("message"),
  message: z.object({ role: z.literal("assistant"), usage: Usage, stopReason: z.string() }),
});

export type SessionMetrics = {
  turns: number;
  tokensIn: number;
  tokensOut: number;
  cacheRead: number;
  cacheWrite: number;
  costUsd: number;
  durationS: number;
  model: string | null;
  thinkingLevel: string | null;
  finalStopReason: string | null;
  /** Entries that carry no metrics (debugging aid). */
  skipped: number;
};

function malformed(path: string, line: number, reason: string) {
  return failure("session_malformed", `${path}:${line}: ${reason}`, { path, line });
}

function parseEntry<T>(schema: z.ZodType<T>, value: unknown, path: string, line: number): T {
  const r = schema.safeParse(value);
  if (!r.success) throw malformed(path, line, z.prettifyError(r.error));
  return r.data;
}

/** Parse one pi session JSONL file into per-Attempt metrics (contract §14). */
export function sessionMetrics(path: string): SessionMetrics {
  const lines = readFileSync(path, "utf8").split("\n");
  if (lines.at(-1) === "") lines.pop();
  if (lines.length === 0) throw malformed(path, 1, "empty file, missing session header");

  const m: SessionMetrics = {
    turns: 0, tokensIn: 0, tokensOut: 0, cacheRead: 0, cacheWrite: 0, costUsd: 0,
    durationS: 0, model: null, thinkingLevel: null, finalStopReason: null, skipped: 0,
  };
  let first = 0;
  let last = 0;

  lines.forEach((text, i) => {
    const n = i + 1;
    let json: unknown;
    try {
      json = JSON.parse(text);
    } catch (e) {
      throw malformed(path, n, `invalid JSON: ${(e as Error).message}`);
    }
    const base = parseEntry(Base.loose(), json, path, n);
    const t = Date.parse(base.timestamp);
    if (i === 0) {
      parseEntry(Header.loose(), json, path, n);
      first = t;
    }
    last = t;

    if (base.type === "model_change") {
      m.model = parseEntry(ModelChange.loose(), json, path, n).modelId;
    } else if (base.type === "thinking_level_change") {
      m.thinkingLevel = parseEntry(ThinkingChange.loose(), json, path, n).thinkingLevel;
    } else if (base.type === "message" && (json as { message?: { role?: unknown } }).message?.role === "assistant") {
      const { usage, stopReason } = parseEntry(AssistantMessage.loose(), json, path, n).message;
      m.turns += 1;
      m.tokensIn += usage.input;
      m.tokensOut += usage.output;
      m.cacheRead += usage.cacheRead;
      m.cacheWrite += usage.cacheWrite;
      m.costUsd += usage.cost.total;
      m.finalStopReason = stopReason;
    } else if (i !== 0 || base.type !== "session") {
      m.skipped += 1;
    }
  });

  m.durationS = (last - first) / 1000;
  return m;
}
