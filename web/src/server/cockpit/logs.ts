import { parseEventLine } from "./events.ts";
import { toArgv } from "../cpd-argv.ts";

export interface LogEntry { readonly id: number; readonly at: string; readonly kind: string; readonly text: string; readonly tone: "error" | "lead-error" | "normal" }
export interface LogsDeps { cpd(argv: readonly string[]): Promise<string> }
const KEEP = 500;
const LEAD_CODES = new Set(["workflow_start_failed", "push_failed"]);

export async function readLogs(deps: LogsDeps): Promise<LogEntry[]> {
  const out = await deps.cpd(toArgv({ kind: "events.follow", since: 0, untilIdle: true }));
  const lines = out.split("\n").filter((l) => l.trim() !== "").slice(-KEEP);
  return lines.map((line) => {
    const e = parseEventLine(line);
    const lead = e.kind === "error" && e.plan !== null && (e.data["source"] !== "watch" || LEAD_CODES.has(String(e.data["code"])));
    return { id: e.id, at: e.at, kind: e.kind, text: e.text, tone: e.kind !== "error" ? "normal" : lead ? "lead-error" : "error" };
  });
}
