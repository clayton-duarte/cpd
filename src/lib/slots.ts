import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { failure, refused } from "./errors.ts";
import { pidAlive, withLock } from "./lock.ts";
import { slotPath, slotsDir, slotsLockPath } from "./paths.ts";

const SlotFile = z.strictObject({
  wf: z.string(),
  attempt: z.string(),
  pid: z.number().int().positive(),
  at: z.string(),
});
export type Slot = z.infer<typeof SlotFile>;

/** Read slot `i`; null when free. A corrupt file fails loud (it would otherwise block the slot forever). */
function readSlot(i: number): Slot | null {
  const path = slotPath(i);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw e;
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("slot_corrupt", `slot file is not valid JSON: ${path}`, { path });
  }
  const parsed = SlotFile.safeParse(raw);
  if (!parsed.success) throw failure("slot_corrupt", `slot file does not match the schema: ${path}`, { path, issues: parsed.error.issues });
  return parsed.data;
}

function writeSlot(i: number, slot: Slot): void {
  writeFileSync(slotPath(i), JSON.stringify(slot), { flag: "w" });
}

const inSlotsLock = <T>(purpose: string, fn: () => T): Promise<T> => withLock(slotsLockPath(), purpose, fn);

function assertConcurrency(n: number): void {
  if (!Number.isInteger(n) || n < 1) throw failure("invalid_concurrency", `concurrency must be an integer >= 1, got ${String(n)}`, { concurrency: n });
}

/**
 * Contract §9: take the first free or stale slot below `concurrency`; null when all are held.
 * The slot records the calling process's pid; use `setSlotPid` to hand it to a spawned child.
 */
export function acquireSlot(concurrency: number, who: { wf: string; attempt: string }): Promise<number | null> {
  assertConcurrency(concurrency);
  return inSlotsLock("acquire-slot", () => {
    mkdirSync(slotsDir(), { recursive: true });
    for (let i = 0; i < concurrency; i++) {
      const held = readSlot(i);
      if (held && pidAlive(held.pid)) continue;
      if (held) {
        process.stderr.write(`cpd: reclaiming stale slot ${i} (attempt ${held.attempt}, dead pid ${held.pid})\n`);
      }
      writeSlot(i, { wf: who.wf, attempt: who.attempt, pid: process.pid, at: new Date().toISOString() });
      return i;
    }
    return null;
  });
}

function assertHeld(i: number, attempt: string, op: string): Slot {
  const held = readSlot(i);
  if (!held || held.attempt !== attempt) {
    throw refused("slot_not_held", `cannot ${op} slot ${i}: it is ${held ? `held by ${held.attempt}` : "free"}, not by ${attempt}`, {
      index: i,
      attempt,
      holder: held,
    });
  }
  return held;
}

/** Record `pid` as the holder's process (the detached `attempt run` takes over the slot the dispatcher acquired). */
export function setSlotPid(index: number, attempt: string, pid: number): Promise<void> {
  return inSlotsLock("set-slot-pid", () => {
    const held = assertHeld(index, attempt, "update");
    writeSlot(index, { ...held, pid });
  });
}

/**
 * Release slot `index` if `attempt` holds it; returns false when it was already free (idempotent).
 * Refuses (exit 3, `slot_not_held`) when another attempt holds it.
 */
export function releaseSlot(index: number, attempt: string): Promise<boolean> {
  return inSlotsLock("release-slot", () => {
    const held = readSlot(index);
    if (!held) return false;
    assertHeld(index, attempt, "release");
    unlinkSync(slotPath(index));
    return true;
  });
}
