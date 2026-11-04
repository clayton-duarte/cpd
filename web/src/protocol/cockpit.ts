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
}
export interface ContextNode {
  readonly id: string;
  readonly title: string;
  readonly archived: boolean;
  readonly records: number;
  readonly openQuestions: number;
  readonly sessions: readonly SessionNode[]; // sessions of the Context with plan === null (Manager, forks)
  readonly plans: readonly PlanNode[];
}
export interface ContextTreeModel {
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
  readonly pr: { readonly number: number; readonly url: string; readonly state: string | null; readonly checks: string | null; readonly mergeable: string | null } | null; // from `pr` + `pull_request` (state, checks, mergeable)
}
export interface PlanViewModel {
  readonly planId: string; readonly contextId: string; readonly title: string; readonly phase: string;
  readonly counts: PlanCounts;
  readonly review: { readonly id: string; readonly status: string; readonly current: boolean } | null;
  readonly openEscalations: number; // escalations.length (open only)
  readonly workflows: readonly WorkflowView[];
  readonly jobs: readonly JobView[]; // all Jobs of the Plan in `seq` order
  readonly cursor: number; // events_cursor
}

export interface InboxRowAction { readonly label: string; readonly action: CockpitAction }
export interface InboxRow {
  readonly id: number;
  readonly kind: string; // seen: pr-ready, dialog, unread, curate-retire; documented: plan-review, loop-guard, drafts-parked, pr-closed, review-requested, deploy, engine-down (scout-cockpit §1.3)
  readonly label: string; // may contain "\n"
  readonly urgency: Urgency;
  readonly contextId: string | null; readonly planId: string | null; readonly jobKey: string | null;
  readonly sessionId: string | null; // `session`, the chat target when set
  readonly url: string | null;
  readonly workflow?: string | null; // the Workflow key for PR kinds (optional until the read model fills it)
  readonly openedAt: string; readonly ageSeconds: number;
  readonly actions: readonly InboxRowAction[];
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
  | { kind: "pr.post"; planId: string; workflow: string }
  | { kind: "pr.merge"; planId: string; workflow: string }
  | { kind: "engine.start" }
  | { kind: "session.retier"; sessionId: string; tier: string }
  | { kind: "plan.reject"; planId: string; review: string };
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
    "pr.post": ["planId", "workflow"],
    "pr.merge": ["planId", "workflow"],
    "engine.start": [],
    "session.retier": ["sessionId", "tier"],
    "plan.reject": ["planId", "review"],
  };
  if (typeof kind !== "string" || !Object.hasOwn(fields, kind)) throw new WebError("unknown_action", `unknown action kind ${String(kind)}`);
  for (const f of fields[kind]!) field(o, f, kind);
  return o as unknown as CockpitAction;
}
