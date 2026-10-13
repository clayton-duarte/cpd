import { mkdirSync, readFileSync, readdirSync, renameSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { LeadMessageSchema, type LeadMessage } from "../../src/queue/messages.ts";
import { deliveryOf, errorDelivery, type Delivery } from "./delivery.ts";

const CLAIMED = ".claimed";
const REJECTED = "rejected";

/** `escalation` and `escalation-answered` carry the escalation id the blocked counter keys on. */
const EscalationData = z.looseObject({ id: z.string().min(1) });

export type Parsed = { ok: true; message: LeadMessage } | { ok: false; reason: string };

/** Contract §10 schema plus the `data.id` the two escalation kinds need. */
export function parseMessage(text: string): Parsed {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    return { ok: false, reason: `not valid JSON: ${(e as Error).message}` };
  }
  const parsed = LeadMessageSchema.safeParse(raw);
  if (!parsed.success) return { ok: false, reason: parsed.error.issues.map((i) => `${i.path.join(".") || "(root)"}: ${i.message}`).join("; ") };
  const m = parsed.data;
  if (m.kind === "escalation" || m.kind === "escalation-answered") {
    const d = EscalationData.safeParse(m.data);
    if (!d.success) return { ok: false, reason: `${m.kind} needs data.id (the escalation id)` };
  }
  return { ok: true, message: m };
}

/** Escalation id of an `escalation` / `escalation-answered` message (already validated by `parseMessage`). */
export function escalationIdOf(msg: LeadMessage): string {
  return EscalationData.parse(msg.data).id;
}

/** `*.json` files waiting in the queue, oldest first (names start with the epoch ms). */
export function pendingFiles(queueDir: string): string[] {
  try {
    return readdirSync(queueDir).filter((n) => n.endsWith(".json")).sort();
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw e;
  }
}

/** Claim by rename: exactly one caller gets the path, every other (duplicate watch event, second instance) gets null. */
export function claim(queueDir: string, name: string): string | null {
  const claimed = join(queueDir, name + CLAIMED);
  try {
    renameSync(join(queueDir, name), claimed);
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw e;
  }
  return claimed;
}

export type Sink = {
  /** Valid message: the delivery (null for kinds that produce none) and the message itself. */
  message(msg: LeadMessage, delivery: Delivery | null): void;
  /** Invalid file, already moved to `rejected/`: the error delivery for the Lead. */
  rejected(delivery: Delivery): void;
};

/**
 * Claim one file, validate it, hand it to the sink, then delete it. An invalid file is moved to
 * `rejected/` and reported through `sink.rejected`. Returns false when someone else claimed it.
 */
export function processFile(queueDir: string, name: string, sink: Sink): boolean {
  const claimed = claim(queueDir, name);
  if (claimed === null) return false;
  const parsed = parseMessage(readFileSync(claimed, "utf8"));
  if (!parsed.ok) {
    const dir = join(queueDir, REJECTED);
    mkdirSync(dir, { recursive: true });
    const moved = join(dir, name);
    renameSync(claimed, moved);
    sink.rejected(errorDelivery(`cpd: invalid queue message ${name} was moved to ${moved}: ${parsed.reason}`, { file: moved, reason: parsed.reason }));
    return true;
  }
  sink.message(parsed.message, deliveryOf(parsed.message));
  unlinkSync(claimed);
  return true;
}

/** Process every pending file. Returns how many this call claimed. */
export function drain(queueDir: string, sink: Sink): number {
  let n = 0;
  for (const name of pendingFiles(queueDir)) if (processFile(queueDir, name, sink)) n++;
  return n;
}
