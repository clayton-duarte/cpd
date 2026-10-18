import { CpdError, isObject } from "./cpd.ts";
import type { Message } from "./pi.ts";

/** commands.md §15.6: the custom type of the message that lists new Context records. */
export const CONTEXT_TYPE = "cpd-context";

/** At most this many records are listed in one message; the rest are named by a count and a command. */
export const MAX_LISTED = 20;

/**
 * §15.6 "Cursor": the greater of `registered_event` and the greatest `details.event` of the session's `cpd-context`
 * messages whose `details.delivered_to` is this Session (the fork-safe rule of §15.2 step 4: a fork's file starts with
 * its source's messages, whose `delivered_to` is the source's id).
 */
export function contextCursorFrom(registeredEvent: number, entries: readonly unknown[], sessionId: string): number {
  let cursor = registeredEvent;
  for (const e of entries) {
    if (!isObject(e) || e.type !== "custom_message" || e.customType !== CONTEXT_TYPE || !isObject(e.details)) continue;
    if (e.details.delivered_to !== sessionId || !Number.isSafeInteger(e.details.event)) continue;
    cursor = Math.max(cursor, e.details.event as number);
  }
  return cursor;
}

/** A `context-record` Event as `events list` prints it, reduced to what the update needs. */
export type RecordEvent = { event: number; record: string; seq: number };

/** The Event kinds of the Manager's `Plans:` digest (commands.md §15.6, B3-A5). */
export const PLAN_KINDS = ["plan-created", "plan-phase", "pr-opened", "pr-merged", "job-failed", "escalation", "plan-done"] as const;

/** A Plan Event as `events list` prints it, reduced to what the digest needs. */
export type PlanEvent = { event: number; at: string; plan: string; text: string };

/** A Context record as `context show` prints it, reduced to what the update needs. */
export type ContextRecord = { seq: number; id: string; kind: string; at: string; by: string | null; text: string; supersedes?: string; question?: string; plan?: string };

const bad = (what: string, why: string) => new CpdError("bad_output", `cpd ${what}: ${why}`);

/** Parse `cpd events list --kind context-record`: each Event's id, `data.record` and `data.seq`. */
export function parseRecordEvents(out: Record<string, unknown>): RecordEvent[] {
  if (!Array.isArray(out.events)) throw bad("events list", "`events` is not a list");
  return out.events.map((e) => {
    if (!isObject(e) || !Number.isSafeInteger(e.id) || !isObject(e.data) || typeof e.data.record !== "string" || !Number.isSafeInteger(e.data.seq)) {
      throw bad("events list", "a context-record Event lacks id, data.record or data.seq");
    }
    return { event: e.id as number, record: e.data.record, seq: e.data.seq as number };
  });
}

/** Parse `cpd events list --kind <PLAN_KINDS>…`: each Event's id, `at`, `plan` and `text`. */
export function parsePlanEvents(out: Record<string, unknown>): PlanEvent[] {
  if (!Array.isArray(out.events)) throw bad("events list", "`events` is not a list");
  return out.events.map((e) => {
    if (!isObject(e) || !Number.isSafeInteger(e.id) || typeof e.at !== "string" || typeof e.plan !== "string" || typeof e.text !== "string") {
      throw bad("events list", "a Plan Event lacks id, at, plan or text");
    }
    return { event: e.id as number, at: e.at, plan: e.plan, text: e.text };
  });
}

/** Parse `cpd context show`: its `records`, each with `by.session`. */
export function parseRecords(out: Record<string, unknown>): ContextRecord[] {
  if (!Array.isArray(out.records)) throw bad("context show", "`records` is not a list");
  return out.records.map((r) => {
    if (!isObject(r) || !Number.isSafeInteger(r.seq) || typeof r.id !== "string" || typeof r.kind !== "string" || typeof r.at !== "string" || typeof r.text !== "string" || !isObject(r.by)) {
      throw bad("context show", "a record lacks seq, id, kind, at, text or by");
    }
    if (r.by.session !== null && typeof r.by.session !== "string") throw bad("context show", `record ${r.id}: by.session is neither a string nor null`);
    return {
      seq: r.seq as number,
      id: r.id,
      kind: r.kind,
      at: r.at,
      by: r.by.session as string | null,
      text: r.text,
      ...(typeof r.supersedes === "string" ? { supersedes: r.supersedes } : {}),
      ...(typeof r.question === "string" ? { question: r.question } : {}),
      ...(isObject(r.plan) && typeof r.plan.id === "string" ? { plan: r.plan.id } : {}),
    };
  });
}

/** A `brief` is shown short: its first line and its Plan (§15.6); the whole text is `cpd context show --record <id>`. */
const line = (r: ContextRecord): string =>
  r.kind === "brief"
    ? `- ${r.id} brief (${r.at.slice(0, 10)}): ${r.text.trimStart().split("\n")[0]!.trimEnd()}${r.plan !== undefined ? ` Plan ${r.plan}.` : ""}`
    : `- ${r.id} ${r.kind} (${r.at.slice(0, 10)}): ${r.text}${r.question !== undefined ? ` Answers ${r.question}.` : ""}${r.supersedes !== undefined ? ` Supersedes ${r.supersedes}.` : ""}`;

/** The `Plans:` section: at most MAX_LISTED lines, then the command for the rest. */
function plansSection(context: string, plans: readonly PlanEvent[]): string[] {
  const listed = plans.slice(0, MAX_LISTED);
  const lines = ["Plans:", ...listed.map((p) => `- ${p.at} ${p.plan}: ${p.text}`)];
  if (plans.length > listed.length) lines.push(`and ${plans.length - listed.length} more: cpd events list --context ${context} --since ${listed.at(-1)!.event}`);
  return lines;
}

/**
 * §15.6: the message for the records the Event list names, written by other Sessions, oldest first, and, for a
 * Manager (`plans` given), the `Plans:` digest after them. Undefined when neither section has anything left after
 * skipping this Session's own records (the cursor then stays). `details.event` is the highest Event id read of
 * either section, own records included.
 */
export function contextUpdate(context: string, sessionId: string, events: readonly RecordEvent[], records: readonly ContextRecord[], plans?: readonly PlanEvent[]): Message | undefined {
  const wanted = new Set(events.map((e) => e.record));
  const fresh = records.filter((r) => wanted.has(r.id) && r.by !== sessionId).sort((a, b) => a.seq - b.seq);
  const digest = plans ?? [];
  if (fresh.length === 0 && digest.length === 0) return undefined;
  const lines: string[] = [];
  if (fresh.length > 0) {
    const listed = fresh.slice(0, MAX_LISTED);
    lines.push(`Context ${context}: ${fresh.length} new records since your last turn.`, ...listed.map(line));
    const rest = fresh.slice(MAX_LISTED);
    if (rest.length > 0) lines.push(`and ${rest.length} more: cpd context show --context ${context} --since ${rest[0]!.seq - 1}`);
  }
  if (digest.length > 0) lines.push(...plansSection(context, digest));
  return {
    customType: CONTEXT_TYPE,
    content: lines.join("\n"),
    display: true,
    details: { event: Math.max(...events.map((e) => e.event), ...digest.map((p) => p.event)), delivered_to: sessionId, records: fresh.map((r) => r.id) },
  };
}
