import { CpdError, isObject } from "./cpd.ts";
import type { Message } from "./pi.ts";

/** commands.md §15.7: the custom type of the message that lists new Context records. */
export const CONTEXT_TYPE = "cpd-context";

/** At most this many records are listed in one message; the rest are named by a count and a command. */
export const MAX_LISTED = 20;

/**
 * §15.7 "Cursor": the greater of `registered_event` and the greatest `details.event` of the session's `cpd-context`
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

/** The Event kinds of the Manager's `Plans:` digest (commands.md §15.7, B3-A5). */
export const PLAN_KINDS = ["plan-created", "plan-phase", "pr-opened", "pr-merged", "job-failed", "escalation", "plan-done"] as const;

/** A Plan Event as `events list` prints it, reduced to what the digest needs. */
export type PlanEvent = { event: number; at: string; plan: string; text: string };

/** A Job of `context pending`, reduced to what the digest needs. */
export type PendingJob = { key: string; status: string; title: string };
/** A PR of `context pending` that is not merged or cancelled. */
export type PendingPr = { key: string; status: string; number: number | null; jobs: PendingJob[] };
/** An active Plan of `context pending`. */
export type PendingPlan = { id: string; phase: string; prs: PendingPr[] };

/** At most this many Jobs are listed under one PR of `Pending work:`; the rest are named by a count. */
export const MAX_PENDING_JOBS = 10;

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

/** Parse `cpd context pending`: each active Plan's id and phase, and its PRs with their Jobs' key, status and title. */
export function parsePending(out: Record<string, unknown>): PendingPlan[] {
  const what = "context pending";
  if (!Array.isArray(out.plans)) throw bad(what, "`plans` is not a list");
  return out.plans.map((p) => {
    if (!isObject(p) || typeof p.id !== "string" || typeof p.phase !== "string" || !Array.isArray(p.prs)) throw bad(what, "a Plan lacks id, phase or prs");
    const plan = p.id;
    return {
      id: plan,
      phase: p.phase,
      prs: p.prs.map((r) => {
        if (!isObject(r) || typeof r.key !== "string" || typeof r.status !== "string" || !Array.isArray(r.jobs)) throw bad(what, `Plan ${plan}: a PR lacks key, status or jobs`);
        if (r.pull_request !== null && !(isObject(r.pull_request) && Number.isSafeInteger(r.pull_request.number))) throw bad(what, `Plan ${plan}, PR ${r.key}: pull_request is neither null nor an object with a number`);
        return {
          key: r.key,
          status: r.status,
          number: isObject(r.pull_request) ? (r.pull_request.number as number) : null,
          jobs: r.jobs.map((j) => {
            if (!isObject(j) || typeof j.key !== "string" || typeof j.status !== "string" || typeof j.title !== "string") throw bad(what, `Plan ${plan}, PR ${r.key}: a Job lacks key, status or title`);
            return { key: j.key, status: j.status, title: j.title };
          }),
        };
      }),
    };
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

/** A `brief` is shown short: its first line and its Plan (§15.7); the whole text is `cpd context show --record <id>`. */
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

/** The `Pending work:` section: one line per PR with its Job counts, then up to MAX_PENDING_JOBS Job lines. Empty when no Plan has a PR. */
export function pendingSection(plans: readonly PendingPlan[]): string[] {
  const lines: string[] = [];
  for (const p of plans) {
    for (const pr of p.prs) {
      const n = (status: string) => pr.jobs.filter((j) => j.status === status).length;
      lines.push(`- ${p.id} (${p.phase}), PR ${pr.key} ${pr.status}${pr.number !== null ? ` #${pr.number}` : ""}: ${n("merged")} merged on its Lead Branch, ${n("queued")} queued, ${n("running")} running, ${n("failed")} failed, ${n("draft")} draft`);
      lines.push(...pr.jobs.slice(0, MAX_PENDING_JOBS).map((j) => `  - ${j.key} ${j.status}: ${j.title}`));
      if (pr.jobs.length > MAX_PENDING_JOBS) lines.push(`  and ${pr.jobs.length - MAX_PENDING_JOBS} more`);
    }
  }
  return lines.length === 0 ? [] : ["Pending work:", ...lines];
}

/**
 * §15.7: the message for the records the Event list names, written by other Sessions, oldest first, and, for a
 * Manager (`plans` given), the `Plans:` digest after them. Undefined when neither section has anything left after
 * skipping this Session's own records (the cursor then stays), unless `pending` (a Manager's fresh `Pending work:`,
 * not cursor-based) has a line. `details.event` is the highest Event id read of the records and `Plans:`, own records
 * included; `pending` does not move it (0 when nothing was read: the cursor never goes down).
 */
export function contextUpdate(context: string, sessionId: string, events: readonly RecordEvent[], records: readonly ContextRecord[], plans?: readonly PlanEvent[], pending?: readonly PendingPlan[]): Message | undefined {
  const wanted = new Set(events.map((e) => e.record));
  const fresh = records.filter((r) => wanted.has(r.id) && r.by !== sessionId).sort((a, b) => a.seq - b.seq);
  const digest = plans ?? [];
  const inFlight = pendingSection(pending ?? []);
  if (fresh.length === 0 && digest.length === 0 && inFlight.length === 0) return undefined;
  const lines: string[] = [];
  if (fresh.length > 0) {
    const listed = fresh.slice(0, MAX_LISTED);
    lines.push(`Context ${context}: ${fresh.length} new records since your last turn.`, ...listed.map(line));
    const rest = fresh.slice(MAX_LISTED);
    if (rest.length > 0) lines.push(`and ${rest.length} more: cpd context show --context ${context} --since ${rest[0]!.seq - 1}`);
  }
  if (digest.length > 0) lines.push(...plansSection(context, digest));
  lines.push(...inFlight);
  return {
    customType: CONTEXT_TYPE,
    content: lines.join("\n"),
    display: true,
    details: { event: Math.max(0, ...events.map((e) => e.event), ...digest.map((p) => p.event)), delivered_to: sessionId, records: fresh.map((r) => r.id) },
  };
}
