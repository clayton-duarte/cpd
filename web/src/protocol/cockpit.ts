// Cockpit read models and actions (web/CONTRACT.md section 5). Types, plus the action parser (parseCockpitAction).
import { WebError, type SessionRef, type SessionState } from "./domain.ts";

export type Urgency = "red" | "yellow" | "magenta" | "blue" | "cyan" | "white"; // seen live: red, magenta; the rest from docs/commands.md:3910 (unverified, O15)

export interface PlanCounts { draft: number; queued: number; running: number; merged: number; failed: number; cancelled: number; merging?: number }

export interface SessionDot {
  readonly state: SessionState;
  readonly attention: Urgency | null; // the most urgent open inbox item that fills this row's dot; always null while state === "working"
}

export interface SessionNode {
  readonly ref: SessionRef;
  readonly contextId: string;
  readonly planId: string | null; // `plan` of the row
  readonly jobKey: string | null; // third segment of `key` for a Worker; null otherwise
  readonly archived: boolean; // archived_at !== null
  readonly dot: SessionDot;
  readonly createdAt?: string;
}
export interface PlanWorkflowSummary {
  readonly key: string; readonly status: string; readonly repo: string;
  readonly pr: { readonly number: number; readonly url: string; readonly state: string | null; readonly checks: string | null; readonly mergeable: string | null } | null;
}
export interface PlanNode {
  readonly id: string;
  readonly contextId: string;
  readonly title: string;
  readonly phase: string; // grounding | plan | dispatch | deploy | done ... (opaque string)
  readonly archived: boolean;
  readonly counts: PlanCounts;
  readonly leadSessionId: string | null;
  readonly sessions: readonly SessionNode[]; // the Plan's sessions: the Lead first, then Workers in `session list` order
  readonly createdAt?: string;
  readonly workflows?: readonly PlanWorkflowSummary[];
}
export interface ContextNode {
  readonly id: string;
  readonly title: string;
  readonly archived: boolean;
  readonly records: number;
  readonly openQuestions: number;
  readonly sessions: readonly SessionNode[]; // sessions of the Context with plan === null (Manager, forks)
  readonly plans: readonly PlanNode[];
  readonly createdAt?: string;
}
export interface ContextTreeModel {
  readonly hostname?: string;
  readonly contexts: readonly ContextNode[];
  readonly cursor: number; // min of the events_cursor of the four commands
}

export interface AttemptView {
  readonly id: string; readonly number: number; readonly tier: string;
  readonly status: string; readonly outcome: string | null; readonly cause: string | null;
  readonly workerSession: string | null; // worker_session
}
export interface JobView {
  readonly key: string; readonly title: string; readonly kind: string; readonly status: string; // queued|running|merged|failed|cancelled|...
  readonly tier: string; readonly needs: readonly string[];
  readonly workflow: string | null; // key of the Workflow whose `jobs` lists this Job; explore Jobs: null
  readonly attempts: readonly AttemptView[];
}
export interface WorkflowView {
  readonly key: string; readonly title: string; readonly repo: string; readonly base: string;
  readonly status: string; // open|merged|cancelled|waiting|...
  readonly needs: readonly string[];
  readonly jobKeys: readonly string[]; // `jobs` of the Workflow row (keys), in source order
  readonly pr: { readonly number: number; readonly url: string; readonly state: string | null; readonly checks: string | null; readonly mergeable: string | null; readonly posted?: boolean } | null; // from `pr` + `pull_request` (state, checks, mergeable)
}
export interface PlanViewModel {
  readonly planId: string; readonly contextId: string; readonly title: string; readonly phase: string;
  readonly counts: PlanCounts;
  readonly review: { readonly id: string; readonly status: string; readonly current: boolean } | null;
  readonly openEscalations: number; // escalations.length (open only)
  readonly workflows: readonly WorkflowView[];
  readonly jobs: readonly JobView[]; // all Jobs of the Plan in `seq` order
  readonly cursor: number; // events_cursor
  readonly reviewedPr?: { readonly repo: string; readonly number: number; readonly url: string; readonly title: string; readonly state: string | null } | null; // `reviewed_pr` of plan show; null for a Plan with none
}

export interface InboxRowAction { readonly label: string; readonly action: CockpitAction }
export interface CurationRecord {
  readonly id: string;
  readonly text: string;
  readonly detail: string;
}

export interface InboxRow {
  readonly id: number;
  readonly kind: string; // seen: pr-ready, dialog, unread, curate-retire; documented: plan-review, loop-guard, drafts-parked, pr-closed, review-requested, deploy, engine-down (scout-cockpit §1.3)
  readonly label: string; // may contain "\n"
  readonly urgency: Urgency;
  readonly contextId: string | null; readonly planId: string | null; readonly jobKey: string | null;
  readonly sessionId: string | null; // `session`, the chat target when set
  readonly url: string | null;
  readonly subject?: string; // the item's subject, e.g. `pr:<owner>/<repo>#<n>`
  readonly workflow?: string | null; // the Workflow key for PR kinds, from the item's data; null when none
  readonly openedAt: string; readonly ageSeconds: number;
  readonly actions: readonly InboxRowAction[];
  readonly curation?: readonly CurationRecord[];
}
export interface InboxModel {
  readonly openCount: number; // items.length (the list holds open, visible items only)
  readonly engineRunning: boolean;
  readonly hidden: { readonly claimed: number; readonly dismissed: number; readonly snoozed: number };
  readonly rows: readonly InboxRow[]; // source order (rank order of cpd)
  readonly cursor: number;
}

export type SnoozeDuration = "15m" | "1h" | "tomorrow";
export type CockpitAction =
  | { kind: "inbox.snooze"; itemId: number; duration: SnoozeDuration }
  | { kind: "inbox.dismiss"; itemId: number }
  | { kind: "inbox.claim"; itemId: number }
  | { kind: "inbox.approve"; itemId: number }
  | { kind: "pr.post"; planId: string; workflow?: string; pull?: string }
  | { kind: "pr.repost"; planId: string; workflow?: string; pull?: string }
  | { kind: "pr.merge"; planId?: string; workflow?: string; pull?: string }
  | { kind: "engine.start" }
  | { kind: "error.report"; target: "cpd" | "host"; code: string; message: string }
  | { kind: "context.retire"; contextId: string; approve: string; reject: string }
  | { kind: "session.retier"; sessionId: string; tier: string }
  | { kind: "plan.reject"; planId: string; review: string }
  | { kind: "plan.new"; planId: string; contextId: string; title: string }
  | { kind: "context.rename"; contextId: string; title: string }
  | { kind: "plan.rename"; planId: string; title: string }
  | { kind: "session.rename"; sessionId: string; name: string }
  | { kind: "session.fork"; sourceId: string; name: string }
  | { kind: "session.close"; sessionId: string }
  | { kind: "context.archive"; contextId: string }
  | { kind: "plan.archive"; planId: string }
  | { kind: "session.archive"; sessionId: string }
  | { kind: "context.unarchive"; contextId: string }
  | { kind: "plan.unarchive"; planId: string }
  | { kind: "session.unarchive"; sessionId: string }
  | { kind: "context.delete"; contextId: string }
  | { kind: "plan.delete"; planId: string };
export type ActionOutput = Readonly<Record<string, unknown>>; // the cpd reply object, without `ok`

const DURATIONS: readonly string[] = ["15m", "1h", "tomorrow"];

function field(o: Record<string, unknown>, name: string, kind: string): void {
  const v = o[name];
  const ok = name === "itemId" ? Number.isInteger(v) && (v as number) >= 0
    : name === "duration" ? typeof v === "string" && DURATIONS.includes(v)
    : typeof v === "string" && v !== "";
  if (!ok) throw new WebError("invalid_message", `action ${kind}: bad field ${name}`, { field: name });
}

export function parseCockpitAction(raw: unknown): CockpitAction {
  if (typeof raw !== "object" || raw === null) throw new WebError("unknown_action", "action is not an object");
  const o = raw as Record<string, unknown>;
  const kind = o["kind"];
  const fields: Record<string, readonly string[]> = {
    "inbox.snooze": ["itemId", "duration"],
    "inbox.dismiss": ["itemId"],
    "inbox.claim": ["itemId"],
    "inbox.approve": ["itemId"],
    "pr.post": ["planId"],
    "pr.repost": ["planId"],
    "pr.merge": [],
    "engine.start": [],
    "error.report": ["code", "message"],
    "context.retire": ["contextId"],
    "session.retier": ["sessionId", "tier"],
    "plan.reject": ["planId", "review"],
    "plan.new": ["planId", "contextId", "title"],
    "context.rename": ["contextId", "title"],
    "plan.rename": ["planId", "title"],
    "session.rename": ["sessionId", "name"],
    "session.fork": ["sourceId", "name"],
    "session.close": ["sessionId"],
    "context.archive": ["contextId"],
    "plan.archive": ["planId"],
    "session.archive": ["sessionId"],
    "context.unarchive": ["contextId"],
    "plan.unarchive": ["planId"],
    "session.unarchive": ["sessionId"],
    "context.delete": ["contextId"],
    "plan.delete": ["planId"],
  };
  if (typeof kind !== "string" || !Object.hasOwn(fields, kind)) throw new WebError("unknown_action", `unknown action kind ${String(kind)}`);
  for (const f of fields[kind]!) field(o, f, kind);
  if (kind === "error.report" && o["target"] !== "cpd" && o["target"] !== "host") {
    throw new WebError("invalid_message", `action ${kind}: bad field target`, { field: "target" });
  }
  return o as unknown as CockpitAction;
}

export interface StatusModel {
  readonly runner: string | null;
  readonly preview: string | null;
  readonly incident: { readonly startedAt: string } | null;
  readonly workers: { readonly queued: number; readonly waiting: number; readonly tenants: number; readonly limit: number } | null;
  readonly version: string | null;
  readonly hostAlert: boolean;
  readonly mode: "production" | "preview";
  readonly drift: { readonly count: number; readonly oldestAt: string | null; readonly stale: boolean } | null;
}
/** Dollar totals keyed by Plan id and Context id. */
export type CostModel = Readonly<Record<string, number>>;
export interface LogRow { readonly id: number; readonly at: string; readonly kind: string; readonly text: string; readonly tone: "error" | "lead-error" | "normal" }

/** The Agent Tiers dialog read: the four role Tiers and the four experimental switches. */
export const SETTINGS_SWITCHES = ["durable_workers", "write_curation", "failure_triage", "turn_question_guard"] as const;
export type SettingsSwitch = (typeof SETTINGS_SWITCHES)[number];
export const SETTINGS_ROLES = ["auxiliar", "worker", "lead", "manager"] as const;
export type SettingsRole = (typeof SETTINGS_ROLES)[number];
export interface SettingsModel {
  readonly tiers: Readonly<Record<SettingsRole, string>>;
  readonly experimental: Readonly<Record<SettingsSwitch, boolean>>;
  /** The machine-wide Worker count (config `concurrency`, docs/contract.md §8.2); absent from an older reply. */
  readonly concurrency?: number;
}
/** Only the values the User changed. */
export interface SettingsChanges {
  readonly tiers?: Partial<Record<SettingsRole, string>>;
  readonly experimental?: Partial<Record<SettingsSwitch, boolean>>;
  /** The machine-wide Worker count, when the User changed it (air-d70). */
  readonly concurrency?: number;
}
