import { CpdError, isObject } from "./cpd.ts";
import type { Message } from "./pi.ts";
import { proseTime } from "./time.ts";

/** commands.md §15.7a: the custom type of the hidden first message of a by-user Session; its `details.event` counts for the cursor. */
export const FIRST_MESSAGE_TYPE = "cpd-first-message";

/** commands.md §15.7: the custom type of the message that lists new Context records. */
export const CONTEXT_TYPE = "cpd-context";

/** What the display line counts (commands.md §15.7 "Display"): the message's `details` carries these besides `records`; `posts` and `errors` only when there are some. */
export type ContextCounts = { records: readonly unknown[]; plan_events: number; plans_pending: number; posts?: number; errors?: number };

/** The counts of a message's `details` (§15.7 "Display"); undefined when `details` lacks a count (a message of an older adapter). */
export function countsOf(details: unknown): ContextCounts | undefined {
  if (!isObject(details) || !Array.isArray(details.records) || !Number.isSafeInteger(details.plan_events) || !Number.isSafeInteger(details.plans_pending)) return undefined;
  const opt = (k: "posts" | "errors") => (Number.isSafeInteger(details[k]) ? { [k]: details[k] as number } : {});
  return { records: details.records, plan_events: details.plan_events as number, plans_pending: details.plans_pending as number, ...opt("posts"), ...opt("errors") };
}

/** §15.7 "Display": `cpd-context: 6 records · 2 board posts · 17 Plan Events · 10 Plans pending · 3 errors`, with a zero part left out. */
export function summaryLine(details: ContextCounts): string {
  const part = (n: number, one: string, many: string) => (n === 0 ? [] : [`${n} ${n === 1 ? one : many}`]);
  const parts = [
    ...part(details.records.length, "record", "records"),
    ...part(details.posts ?? 0, "board post", "board posts"),
    ...part(details.plan_events, "Plan Event", "Plan Events"),
    ...part(details.plans_pending, "Plan pending", "Plans pending"),
    ...part(details.errors ?? 0, "error", "errors"),
  ];
  return parts.length === 0 ? CONTEXT_TYPE : `${CONTEXT_TYPE}: ${parts.join(" · ")}`;
}

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
    if (!isObject(e) || e.type !== "custom_message" || (e.customType !== CONTEXT_TYPE && e.customType !== FIRST_MESSAGE_TYPE) || !isObject(e.details)) continue;
    if (e.details.delivered_to !== sessionId || !Number.isSafeInteger(e.details.event)) continue;
    cursor = Math.max(cursor, e.details.event as number);
  }
  return cursor;
}

/** A `context-record` Event as `events list` prints it, reduced to what the update needs. */
export type RecordEvent = { event: number; record: string; seq: number };

/** The Event kinds of the Manager's `Plans:` digest (commands.md §15.7, B3-A5). */
export const PLAN_KINDS = ["plan-created", "plan-phase", "pr-opened", "pr-merged", "job-failed", "plan-done"] as const;

/** A Plan Event as `events list` prints it, reduced to what the digest needs. */
export type PlanEvent = { event: number; at: string; plan: string; text: string; kind?: string; data?: Record<string, unknown> };
/** A Plan Event whose `kind` and `data` are known: what the per-Plan counts read. */
export type PlanKindEvent = PlanEvent & { kind: string; data: Record<string, unknown> };
/** An unread board post, as `cpd board take` prints it: `from` is the sending Session, null for the User. */
export type BoardPost = { at: string; from: string | null; fromName: string | null; text: string };
/** A `pr-opened` Event of a Plan: its Workflow key, its pull request number and URL. */
export type PrOpened = { event: number; plan: string; workflow: string; number: number | null; url: string };
/** An `error` Event: its Event id and `data.code`. */
export type ErrorEvent = { event: number; code: string; context: string | null; plan: string | null };

/** A plan-level explore Job of `context pending`, reduced to what the digest needs. */
export type PendingJob = { key: string; kind: string; status: string; tier: string | null; title: string };
/** A Job of a Workflow of `context pending`. */
export type PendingPrJob = { key: string; status: string; title: string };
/** A Workflow of `context pending` that is not merged or cancelled. */
export type PendingPr = { key: string; status: string; number: number | null; jobs: PendingPrJob[] };
/** An active Plan of `context pending`. */
export type PendingPlan = { id: string; phase: string; workflows: PendingPr[]; jobs: PendingJob[] };

/** At most this many Jobs are listed under one Workflow of `Pending work:`; the rest are named by a count. */
export const MAX_PENDING_JOBS = 10;

/** A Context record as `context show` prints it, reduced to what the update needs. */
export type ContextRecord = { seq: number; id: string; kind: string; at: string; by: string | null; text: string; supersedes?: string[]; question?: string; plan?: string };

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

/** Parse `cpd events list --kind <PLAN_KINDS>…`: each Event's id, `at`, `plan`, `text`, `kind` and `data`. */
export function parsePlanEvents(out: Record<string, unknown>): PlanKindEvent[] {
  if (!Array.isArray(out.events)) throw bad("events list", "`events` is not a list");
  return out.events.map((e) => {
    if (!isObject(e) || !Number.isSafeInteger(e.id) || typeof e.at !== "string" || typeof e.plan !== "string" || typeof e.text !== "string" || typeof e.kind !== "string" || !isObject(e.data)) {
      throw bad("events list", "a Plan Event lacks id, at, plan, text, kind or data");
    }
    return { event: e.id as number, at: e.at, plan: e.plan, text: e.text, kind: e.kind, data: e.data };
  });
}

/**
 * Parse `cpd events list --kind pr-opened`: each Event's id, Plan, `data.workflow`, `data.number` and `data.url`.
 * The legacy `data.pr` of older Events is read when `data.workflow` is absent (Events are immutable, K1).
 */
export function parsePrOpened(out: Record<string, unknown>): PrOpened[] {
  if (!Array.isArray(out.events)) throw bad("events list", "`events` is not a list");
  return out.events.map((e) => {
    if (!isObject(e) || !Number.isSafeInteger(e.id) || typeof e.plan !== "string" || !isObject(e.data)) {
      throw bad("events list", "a pr-opened Event lacks id, plan, data.workflow (or the legacy data.pr) or data.url");
    }
    const workflow = e.data.workflow ?? e.data.pr;
    if (typeof workflow !== "string" || typeof e.data.url !== "string") {
      throw bad("events list", "a pr-opened Event lacks id, plan, data.workflow (or the legacy data.pr) or data.url");
    }
    return { event: e.id as number, plan: e.plan, workflow, number: Number.isSafeInteger(e.data.number) ? (e.data.number as number) : null, url: e.data.url };
  });
}

/** Parse `cpd events list --kind error`: each Event's id and `data.code`. */
export function parseErrorEvents(out: Record<string, unknown>): ErrorEvent[] {
  if (!Array.isArray(out.events)) throw bad("events list", "`events` is not a list");
  return out.events.map((e) => {
    if (!isObject(e) || !Number.isSafeInteger(e.id) || !isObject(e.data) || typeof e.data.code !== "string") throw bad("events list", "an error Event lacks id or data.code");
    return { event: e.id as number, code: e.data.code, context: typeof e.context === "string" ? e.context : null, plan: typeof e.plan === "string" ? e.plan : null };
  });
}

/** Parse `cpd board take`: each taken post's `at`, `from_session` and `text`. */
export function parseTaken(out: Record<string, unknown>): BoardPost[] {
  if (!Array.isArray(out.taken)) throw bad("board take", "`taken` is not a list");
  return out.taken.map((p) => {
    if (!isObject(p) || typeof p.at !== "string" || typeof p.text !== "string" || (p.from_session !== null && typeof p.from_session !== "string") || (p.from_name !== null && typeof p.from_name !== "string")) throw bad("board take", "a post lacks at, from_session, from_name or text");
    return { at: p.at, from: p.from_session, fromName: p.from_name, text: p.text };
  });
}

/** Parse `cpd context pending`: each active Plan's id and phase, its plan-level explore Jobs and its Workflows with their Jobs' key, status and title. */
export function parsePending(out: Record<string, unknown>): PendingPlan[] {
  const what = "context pending";
  if (!Array.isArray(out.plans)) throw bad(what, "`plans` is not a list");
  return out.plans.map((p) => {
    if (!isObject(p) || typeof p.id !== "string" || typeof p.phase !== "string" || !Array.isArray(p.workflows) || !Array.isArray(p.jobs)) throw bad(what, "a Plan lacks id, phase, workflows or jobs");
    const plan = p.id;
    return {
      id: plan,
      phase: p.phase,
      jobs: p.jobs.map((j) => {
        if (!isObject(j) || typeof j.key !== "string" || typeof j.kind !== "string" || typeof j.status !== "string" || typeof j.title !== "string") throw bad(what, `Plan ${plan}: a Job lacks key, kind, status or title`);
        return { key: j.key, kind: j.kind, status: j.status, tier: typeof j.tier === "string" ? j.tier : null, title: j.title };
      }),
      workflows: p.workflows.map((r) => {
        if (!isObject(r) || typeof r.key !== "string" || typeof r.status !== "string" || !Array.isArray(r.jobs)) throw bad(what, `Plan ${plan}: a Workflow lacks key, status or jobs`);
        if (r.pull_request !== null && !(isObject(r.pull_request) && Number.isSafeInteger(r.pull_request.number))) throw bad(what, `Plan ${plan}, Workflow ${r.key}: pull_request is neither null nor an object with a number`);
        return {
          key: r.key,
          status: r.status,
          number: isObject(r.pull_request) ? (r.pull_request.number as number) : null,
          jobs: r.jobs.map((j) => {
            if (!isObject(j) || typeof j.key !== "string" || typeof j.status !== "string" || typeof j.title !== "string") throw bad(what, `Plan ${plan}, Workflow ${r.key}: a Job lacks key, status or title`);
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
      by: r.by.session,
      text: r.text,
      ...(isStrings(r.supersedes) && r.supersedes.length > 0 ? { supersedes: r.supersedes } : {}),
      ...(typeof r.question === "string" ? { question: r.question } : {}),
      ...(isObject(r.plan) && typeof r.plan.id === "string" ? { plan: r.plan.id } : {}),
    };
  });
}

/** A `brief` is shown short: its first line and its Plan (§15.7); the whole text is `cpd context show --record <id>`. */
const line = (r: ContextRecord): string =>
  r.kind === "brief"
    ? `- ${r.id} brief (${r.at.slice(0, 10)} UTC): ${r.text.trimStart().split("\n")[0]!.trimEnd()}${r.plan !== undefined ? ` Plan ${r.plan}.` : ""}`
    : `- ${r.id} ${r.kind} (${r.at.slice(0, 10)} UTC): ${r.text}${r.question !== undefined ? ` Answers ${r.question}.` : ""}${r.supersedes !== undefined ? ` Supersedes ${r.supersedes.join(", ")}.` : ""}`;

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const sender = (p: BoardPost) => (p.from === null ? "Board post from the User" : `Board post from ${p.fromName ?? p.from} (${p.from})`);

/** The `Board:` section: each post in full with its sender. Empty when there is none. */
function boardSection(posts: readonly BoardPost[]): string[] {
  return posts.length === 0 ? [] : ["Board:", ...posts.map((p) => `- ${sender(p)} (${proseTime(p.at)}): ${p.text}`)];
}

/** The Event kinds of a Plan's line, in their fixed order, with how each is counted. */
const PLAN_COUNTS: readonly { kind: string; say: (events: PlanKindEvent[]) => string }[] = [
  { kind: "plan-created", say: () => "created" },
  { kind: "plan-phase", say: (e) => plural(e.length, "phase change", "phase changes") },
  {
    kind: "pr-opened",
    say: (e) => {
      const numbers = e.flatMap((x) => (Number.isSafeInteger(x.data.number) ? [`#${x.data.number}`] : []));
      return `${plural(e.length, "pull request", "pull requests")} opened${numbers.length > 0 ? ` (${numbers.join(", ")})` : ""}`;
    },
  },
  { kind: "pr-merged", say: (e) => `${plural(e.length, "pull request", "pull requests")} merged` },
  { kind: "job-failed", say: (e) => `${plural(e.length, "Job", "Jobs")} failed` },
  { kind: "plan-done", say: () => "done" },
];

/** The `Plans:` section: one line per Plan with its Event counts by kind, in the fixed order of PLAN_COUNTS. */
function plansSection(plans: readonly PlanEvent[]): string[] {
  if (plans.length === 0) return [];
  const byPlan = new Map<string, PlanKindEvent[]>();
  const known = plans.filter((e): e is PlanKindEvent => e.kind !== undefined && e.data !== undefined);
  for (const e of known) byPlan.set(e.plan, [...(byPlan.get(e.plan) ?? []), e]);
  return [
    "Plans:",
    ...[...byPlan].map(([plan, events]) => `- ${plan}: ${PLAN_COUNTS.flatMap((c) => { const of = events.filter((e) => e.kind === c.kind); return of.length === 0 ? [] : [c.say(of)]; }).join(", ")}`),
  ];
}

/** The `Pending work:` section: one line per active Plan with plan-level Jobs or a Workflow, the `jobs <counts>` segment first, then one per Workflow. Empty when no Plan has work. */
export function pendingSection(plans: readonly PendingPlan[]): string[] {
  const statuses = ["running", "queued", "failed", "draft", "merged"];
  const counts = (jobs: readonly { status: string }[]) => statuses.flatMap((status) => {
    const n = jobs.filter((j) => j.status === status).length;
    return n === 0 ? [] : [`${n} ${status}`];
  });
  const lines: string[] = [];
  for (const p of plans) {
    const segments = p.jobs.length === 0 ? [] : [`jobs ${counts(p.jobs).join(", ")}`];
    for (const w of p.workflows) {
      const parts = counts(w.jobs);
      segments.push(`Workflow ${w.key}${parts.length > 0 ? ` ${parts.join(", ")}` : `: pull request ${w.status}${w.number !== null ? ` #${w.number}` : ""}`}`);
    }
    if (segments.length === 0) continue;
    lines.push(`- ${p.id} (${p.phase}): ${segments.join("; ")}`);
  }
  return lines.length === 0 ? [] : ["Pending work:", ...lines];
}

/** The `Errors:` line: the count, the codes with their counts (most frequent first, then by first appearance) and the command for the Events. Empty when there is none. */
function errorsLine(errors: readonly ErrorEvent[], cursor: number): string[] {
  if (errors.length === 0) return [];
  const counts = new Map<string, number>();
  for (const e of errors) counts.set(e.code, (counts.get(e.code) ?? 0) + 1);
  const codes = [...counts].sort((a, b) => b[1] - a[1]).map(([code, n]) => `${code} x${n}`);
  return [`Errors: ${errors.length} (${codes.join(", ")}): cpd events list --kind error --since ${cursor}`];
}

/** The first sentence of a record's text (up to the first `.`, `!` or `?` before a space), or its first line when it has none. */
const firstSentence = (text: string): string => {
  const t = text.trim();
  return /^[\s\S]*?[.!?](?=\s|$)/.exec(t)?.[0] ?? t.split("\n")[0]!.trimEnd();
};

/** The part of `header` that names a count, zero parts left out. */
const header = (context: string, parts: readonly (readonly [number, string, string])[]): string =>
  `Context ${context}: ${parts.filter(([n]) => n > 0).map(([n, one, many]) => plural(n, one, many)).join(" · ")}`;

/**
 * §15.7, Leads: the short message. New decision records by other Sessions (id and first sentence), the board posts taken in full, the Plan's Workflows whose pull request opened.
 * Undefined when none. `details.event` is the highest Event id read (records and Workflows, own and non-decision included; 0 when none).
 */
export function leadUpdate(context: string, sessionId: string, events: readonly RecordEvent[], records: readonly ContextRecord[], workflows: readonly PrOpened[], posts: readonly BoardPost[], standing: readonly string[] = []): Message | undefined {
  const wanted = new Set(events.map((e) => e.record));
  const decisions = records.filter((r) => wanted.has(r.id) && r.by !== sessionId && r.kind === "decision").sort((a, b) => a.seq - b.seq);
  if (decisions.length === 0 && workflows.length === 0 && posts.length === 0 && standing.length === 0) return undefined;
  const sections: string[][] = [
    decisions.map((r) => `- ${r.id}: ${firstSentence(r.text)}`),
    boardSection(posts),
    workflows.length === 0 ? [] : ["Workflows:", ...workflows.map((p) => `- Workflow ${p.workflow}: pull request opened ${p.url}`)],
    [...standing],
  ].filter((s) => s.length > 0);
  const counts = [[decisions.length, "decision", "decisions"], [posts.length, "board post", "board posts"], [workflows.length, "pull request opened", "pull requests opened"]] as const;
  const summary = counts.every(([n]) => n === 0) ? CONTEXT_TYPE : `${CONTEXT_TYPE}: ${counts.filter(([n]) => n > 0).map(([n, one, many]) => plural(n, one, many)).join(" · ")}`;
  const onlyStanding = counts.every(([n]) => n === 0);
  return {
    customType: CONTEXT_TYPE,
    content: [...(onlyStanding ? [] : [header(context, counts)]), ...sections.flatMap((s, i) => (i === 0 && (decisions.length > 0 || onlyStanding) ? s : ["", ...s]))].join("\n"),
    display: true,
    details: { event: Math.max(0, ...events.map((e) => e.event), ...workflows.map((p) => p.event)), delivered_to: sessionId, records: decisions.map((r) => r.id), summary },
  };
}

/** What a Manager's message holds besides the records, the Plan Events and the pending work: the board posts taken, the error Events since the cursor (and that cursor, for the command). */
export type DigestParts = { posts?: readonly BoardPost[]; errors?: readonly ErrorEvent[]; cursor?: number };

/**
 * §15.7, Managers: the message for the records the Event list names, written by other Sessions, oldest first, then the `Board:`, `Plans:`, `Pending work:` and `Errors:` sections.
 * Undefined when no section has anything left after skipping this Session's own records (the cursor then stays). `details.event` is the highest Event id read of the records, Plan Events and errors, own records
 * included; `pending` does not move it (0 when nothing was read: the cursor never goes down).
 */
export function contextUpdate(context: string, sessionId: string, events: readonly RecordEvent[], records: readonly ContextRecord[], plans: readonly PlanEvent[] = [], pending: readonly PendingPlan[] = [], parts: DigestParts = {}): Message | undefined {
  const wanted = new Set(events.map((e) => e.record));
  const fresh = records.filter((r) => wanted.has(r.id) && r.by !== sessionId).sort((a, b) => a.seq - b.seq);
  const { posts = [], errors = [], cursor = 0 } = parts;
  const sections: string[][] = [];
  if (fresh.length > 0) {
    const listed = fresh.slice(0, MAX_LISTED);
    const lines = [`Context ${context}: ${fresh.length} new records since your last turn.`, ...listed.map(line)];
    const rest = fresh.slice(MAX_LISTED);
    if (rest.length > 0) lines.push(`and ${rest.length} more: cpd context show --context ${context} --since ${rest[0]!.seq - 1}`);
    sections.push(lines);
  }
  sections.push(boardSection(posts), plansSection(plans), pendingSection(pending), errorsLine(errors, cursor));
  const kept = sections.filter((s) => s.length > 0);
  if (kept.length === 0) return undefined;
  const optional = (k: "posts" | "errors", n: number) => (n > 0 ? { [k]: n } : {});
  return {
    customType: CONTEXT_TYPE,
    content: kept.map((s) => s.join("\n")).join("\n\n"),
    display: true,
    details: {
      event: Math.max(0, ...events.map((e) => e.event), ...plans.map((p) => p.event), ...errors.map((e) => e.event)),
      delivered_to: sessionId,
      records: fresh.map((r) => r.id),
      plan_events: plans.length,
      plans_pending: pending.filter((p) => p.workflows.length > 0 || p.jobs.length > 0).length,
      ...optional("posts", posts.length),
      ...optional("errors", errors.length),
    },
  };
}

/** The Lead's standing lines for the Workflows of `cpd plan show` whose pull request closed without a merge (`closed_unmerged`): one per Workflow, derived each turn, never consumed. */
export function closedPrLines(planShow: unknown): string[] {
  const workflows = isObject(planShow) && Array.isArray(planShow.workflows) ? planShow.workflows : [];
  return workflows.flatMap((p) => {
    if (!isObject(p) || p.closed_unmerged !== true || !isObject(p.pull_request)) return [];
    const { number, url } = p.pull_request;
    return [`Workflow ${p.key}: its pull request #${number} (${url}) is closed without a merge and cpd still lists the Workflow as open. Ask the User, or cpd workflow cancel --workflow ${p.key}.`];
  });
}

const isStrings = (v: unknown): v is string[] => Array.isArray(v) && v.every((s) => typeof s === "string");
