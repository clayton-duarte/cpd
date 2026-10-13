import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { cpdLogPath } from "../lib/paths.ts";
import { readLedger, type ReadLedger } from "./bd.ts";
import { BdEvents, BdIssues } from "./schema.ts";

/** Append one structured event to `<wf>/logs/cpd.jsonl` (contract §2). */
export function logEvent(wf: string, level: "info" | "warn" | "error", event: string, data: Record<string, unknown> = {}): void {
  const path = cpdLogPath(wf);
  mkdirSync(dirname(path), { recursive: true });
  appendFileSync(path, JSON.stringify({ at: new Date().toISOString(), level, event, ...data }) + "\n");
}

export type TamperEvent = { issue: string; eventType: string; actor: string; at: string; id: string };

/** Contract §8: every `bd history --events` entry of the Workflow whose actor is not `cpd`. Detects, does not prevent. */
export async function listTamper(wf: string, l: ReadLedger = readLedger(wf)): Promise<TamperEvent[]> {
  const out: TamperEvent[] = [];
  for (const issue of await l.read(["list", "--all", "-n", "0"], BdIssues)) {
    for (const e of await l.read(["history", issue.id, "--events"], BdEvents)) {
      if (e.actor !== "cpd") out.push({ issue: issue.id, eventType: e.event_type, actor: e.actor, at: e.created_at, id: e.id });
    }
  }
  return out.sort((a, b) => a.at.localeCompare(b.at));
}
