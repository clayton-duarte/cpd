import { readFileSync } from "node:fs";
import { z } from "zod";
import { writeJsonAtomic } from "../ledger/workflow.ts";
import { failure } from "../lib/errors.ts";
import { withLock } from "../lib/lock.ts";
import { ledgerLockPath, prStatePath } from "../lib/paths.ts";

/** Contract §13. */
export const PrState = z.strictObject({
  v: z.literal(1),
  number: z.number().int().positive(),
  lastPollAt: z.string(),
  checks: z.record(z.string(), z.string()),
  reviewIds: z.array(z.string()),
  commentIds: z.array(z.string()),
});
export type PrState = z.infer<typeof PrState>;

/** null when no poll has happened yet. */
export function readPrState(wf: string): PrState | null {
  const path = prStatePath(wf);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw failure("pr_state_unreadable", `cannot read ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("pr_state_invalid", `${path} is not valid JSON`, { path });
  }
  const parsed = PrState.safeParse(raw);
  if (!parsed.success) throw failure("pr_state_invalid", `${path} does not match the schema`, { path, issues: parsed.error.issues });
  return parsed.data;
}

export function writePrState(wf: string, state: PrState): Promise<void> {
  const parsed = PrState.parse(state);
  return withLock(ledgerLockPath(wf), "write pr-state.json", () => writeJsonAtomic(prStatePath(wf), parsed));
}
