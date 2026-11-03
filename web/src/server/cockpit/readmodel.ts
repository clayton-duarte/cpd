import type {
  AttemptView, CockpitAction, ContextNode, ContextTreeModel, InboxModel, InboxRow, InboxRowAction, JobView, PlanCounts, PlanNode,
  PlanViewModel, SessionNode, Urgency, WorkflowView,
} from "../../protocol/cockpit.ts";
import { WebError, type SessionRole, type SessionState } from "../../protocol/domain.ts";
import type { CockpitPane } from "../../protocol/wire.ts";
import type { Cpd } from "./cli.ts";

type Reply = Readonly<Record<string, unknown>>;
type Obj = Readonly<Record<string, unknown>>;
type StrOrNull = string | null;

export interface RawContext { id: string; title: string; archived_at: StrOrNull; records: number; open_questions: number }
export interface RawPlan { id: string; context: string; title: string; phase: string; archived_at: StrOrNull; created_at: string; counts: PlanCounts; lead_session: StrOrNull }
export interface RawSession {
  id: string; runner: "pi" | "durable"; role: SessionRole; state: SessionState; context: string; plan: StrOrNull; plan_lead: boolean;
  archived_at: StrOrNull; file: StrOrNull; tier: StrOrNull; title: StrOrNull; cwd: StrOrNull; key: StrOrNull; name: StrOrNull;
}
export interface RawInboxItem {
  id: number | null; kind: string; label: string; urgency: Urgency; opened_at: string; age_seconds: number;
  context: StrOrNull; plan: StrOrNull; job: StrOrNull; session: StrOrNull; url: StrOrNull;
  snoozed_until: StrOrNull; dismissed_at: StrOrNull; claimed_at: StrOrNull;
}
export interface RawPlanShow { plan: { id: string; context: string; title: string; phase: string; archived_at: StrOrNull; lead_session: StrOrNull }; counts: PlanCounts; jobs: Obj[]; workflows: Obj[]; review: Obj | null; escalations: unknown[]; cursor: number }
export interface ParsedInbox {
  rows: RawInboxItem[];
  engineRunning: boolean;
  hidden: { claimed: number; dismissed: number; snoozed: number };
  cursor: number;
}

export type PaneSet = ReadonlySet<CockpitPane>;
export interface PaneUpdate { tree?: ContextTreeModel; plan?: PlanViewModel | null; inbox?: InboxModel }
export interface ReadModel {
  loadAll(planId: string | null): Promise<{ tree: ContextTreeModel; plan: PlanViewModel | null; inbox: InboxModel }>;
  loadPanes(panes: PaneSet, planId: string | null): Promise<PaneUpdate>;
}

// ---- boundary validation -------------------------------------------------

function bad(command: string, field: string): never {
  throw new WebError("cpd_malformed_json", `${command}: missing or mistyped field ${field}`, { command, field });
}
function obj(v: unknown, command: string, field: string): Obj {
  if (typeof v !== "object" || v === null || Array.isArray(v)) return bad(command, field);
  return v as Obj;
}
function str(o: Obj, f: string, c: string): string {
  const v = o[f];
  return typeof v === "string" ? v : bad(c, f);
}
function strN(o: Obj, f: string, c: string): StrOrNull {
  const v = o[f];
  return v === null ? null : typeof v === "string" ? v : bad(c, f);
}
function num(o: Obj, f: string, c: string): number {
  const v = o[f];
  return typeof v === "number" && Number.isFinite(v) ? v : bad(c, f);
}
function bool(o: Obj, f: string, c: string): boolean {
  const v = o[f];
  return typeof v === "boolean" ? v : bad(c, f);
}
function arr(o: Obj, f: string, c: string): unknown[] {
  const v = o[f];
  return Array.isArray(v) ? v : bad(c, f);
}
function oneOf<T extends string>(o: Obj, f: string, c: string, set: readonly T[]): T {
  const v = o[f];
  return typeof v === "string" && (set as readonly string[]).includes(v) ? (v as T) : bad(c, f);
}
function counts(o: Obj, c: string): PlanCounts {
  const k = obj(o["counts"], c, "counts");
  return { draft: num(k, "draft", c), queued: num(k, "queued", c), running: num(k, "running", c), merged: num(k, "merged", c), failed: num(k, "failed", c), cancelled: num(k, "cancelled", c) };
}
const cursorOf = (r: Reply, c: string): number => num(r, "events_cursor", c);

export function parseContextList(reply: Reply): RawContext[] {
  const c = "context list";
  cursorOf(reply, c);
  return arr(reply, "contexts", c).map((v) => {
    const o = obj(v, c, "contexts[]");
    return { id: str(o, "id", c), title: str(o, "title", c), archived_at: strN(o, "archived_at", c), records: num(o, "records", c), open_questions: num(o, "open_questions", c) };
  });
}
export function parsePlanList(reply: Reply): RawPlan[] {
  const c = "plan list";
  cursorOf(reply, c);
  return arr(reply, "plans", c).map((v) => {
    const o = obj(v, c, "plans[]");
    return {
      id: str(o, "id", c), context: str(o, "context", c), title: str(o, "title", c), phase: str(o, "phase", c),
      archived_at: strN(o, "archived_at", c), created_at: str(o, "created_at", c), counts: counts(o, c), lead_session: strN(o, "lead_session", c),
    };
  });
}
export function parseSessionList(reply: Reply): RawSession[] {
  const c = "session list";
  cursorOf(reply, c);
  return arr(reply, "sessions", c).map((v) => {
    const o = obj(v, c, "sessions[]");
    return {
      id: str(o, "id", c), runner: oneOf(o, "runner", c, ["pi", "durable"]), role: oneOf(o, "role", c, ["manager", "lead", "worker"]),
      state: oneOf(o, "state", c, ["working", "waiting", "blocked", "idle", "stopped"]),
      context: str(o, "context", c), plan: strN(o, "plan", c), plan_lead: bool(o, "plan_lead", c), archived_at: strN(o, "archived_at", c),
      file: strN(o, "file", c), tier: strN(o, "tier", c), title: strN(o, "title", c), cwd: strN(o, "cwd", c), key: strN(o, "key", c), name: strN(o, "name", c),
    };
  });
}
export function parseInboxList(reply: Reply): ParsedInbox {
  const c = "inbox list";
  const hidden = obj(reply["hidden"], c, "hidden");
  const rows = arr(reply, "items", c).map((v) => {
    const o = obj(v, c, "items[]");
    const id = o["id"] === null ? null : num(o, "id", c);
    return {
      id, kind: str(o, "kind", c), label: str(o, "label", c), urgency: str(o, "urgency", c) as Urgency, opened_at: str(o, "opened_at", c),
      age_seconds: num(o, "age_seconds", c), context: strN(o, "context", c), plan: strN(o, "plan", c), job: strN(o, "job", c),
      session: strN(o, "session", c), url: strN(o, "url", c), snoozed_until: strN(o, "snoozed_until", c),
      dismissed_at: strN(o, "dismissed_at", c), claimed_at: strN(o, "claimed_at", c),
    };
  });
  return {
    rows, engineRunning: bool(obj(reply["engine"], c, "engine"), "running", c),
    hidden: { claimed: num(hidden, "claimed", c), dismissed: num(hidden, "dismissed", c), snoozed: num(hidden, "snoozed", c) },
    cursor: cursorOf(reply, c),
  };
}
export function parsePlanShow(reply: Reply): RawPlanShow {
  const c = "plan show";
  const p = obj(reply["plan"], c, "plan");
  const review = reply["review"];
  return {
    plan: { id: str(p, "id", c), context: str(p, "context", c), title: str(p, "title", c), phase: str(p, "phase", c), archived_at: strN(p, "archived_at", c), lead_session: strN(p, "lead_session", c) },
    counts: counts(reply, c),
    jobs: arr(reply, "jobs", c).map((j) => obj(j, c, "jobs[]")),
    workflows: arr(reply, "workflows", c).map((w) => obj(w, c, "workflows[]")),
    review: review === null ? null : obj(review, c, "review"),
    escalations: arr(reply, "escalations", c),
    cursor: cursorOf(reply, c),
  };
}

// ---- builders ------------------------------------------------------------

const RANK: readonly string[] = ["red", "yellow", "magenta", "blue", "cyan", "white"];
const LEAD_KINDS = ["plan-review", "job-stuck", "pr-closed"];

function visible(i: RawInboxItem): boolean {
  return i.snoozed_until === null && i.dismissed_at === null && i.claimed_at === null;
}
function linked(s: RawSession, i: RawInboxItem): boolean {
  if (i.session === s.id) return true;
  if (!s.plan_lead || i.plan === null || i.plan !== s.plan) return false;
  return LEAD_KINDS.includes(i.kind) || (i.kind === "pr-ready" && (i.urgency === "red" || i.urgency === "yellow"));
}
function sessionNode(s: RawSession, inbox: RawInboxItem[]): SessionNode {
  const key = s.key !== null && s.key !== "" ? s.key : s.name ?? s.id;
  const parts = s.key === null ? [] : s.key.split(":");
  let attention: Urgency | null = null;
  if (s.state !== "working") {
    for (const i of inbox) {
      if (!visible(i) || !linked(s, i)) continue;
      if (attention === null || RANK.indexOf(i.urgency) < RANK.indexOf(attention)) attention = i.urgency;
    }
  }
  return {
    ref: {
      id: s.id, key, runner: s.runner, role: s.role, state: s.state, title: s.title,
      ...(s.file !== null ? { file: s.file } : {}), ...(s.tier !== null ? { tier: s.tier } : {}), ...(s.cwd !== null ? { cwd: s.cwd } : {}),
    },
    contextId: s.context, planId: s.plan, jobKey: parts.length === 3 ? parts[2]! : null,
    archived: s.archived_at !== null, dot: { state: s.state, attention },
  };
}
const ZERO: PlanCounts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };

export function buildTree(input: { contexts: RawContext[]; plans: RawPlan[]; sessions: RawSession[]; inbox: RawInboxItem[]; cursor: number }): ContextTreeModel {
  const ctxs = new Map<string, { node: Omit<ContextNode, "sessions" | "plans">; sessions: SessionNode[]; plans: Map<string, { node: Omit<PlanNode, "sessions">; sessions: SessionNode[] }> }>();
  const ctx = (id: string) => {
    let c = ctxs.get(id);
    if (c === undefined) {
      c = { node: { id, title: id, archived: false, records: 0, openQuestions: 0 }, sessions: [], plans: new Map() };
      ctxs.set(id, c);
    }
    return c;
  };
  const order = (xs: RawContext[]) => [...xs.filter((c) => c.archived_at === null), ...xs.filter((c) => c.archived_at !== null)];
  for (const c of order(input.contexts)) {
    ctxs.set(c.id, { node: { id: c.id, title: c.title, archived: c.archived_at !== null, records: c.records, openQuestions: c.open_questions }, sessions: [], plans: new Map() });
  }
  const plans = [...input.plans].sort((a, b) => Number(a.archived_at !== null) - Number(b.archived_at !== null) || (a.created_at < b.created_at ? 1 : a.created_at > b.created_at ? -1 : 0));
  for (const p of plans) {
    ctx(p.context).plans.set(p.id, {
      node: { id: p.id, contextId: p.context, title: p.title, phase: p.phase, archived: p.archived_at !== null, counts: p.counts, leadSessionId: p.lead_session },
      sessions: [],
    });
  }
  for (const s of input.sessions) {
    const node = sessionNode(s, input.inbox);
    const c = ctx(s.context);
    if (s.plan === null) { c.sessions.push(node); continue; }
    let p = c.plans.get(s.plan);
    if (p === undefined) {
      p = { node: { id: s.plan, contextId: s.context, title: s.plan, phase: "unknown", archived: false, counts: ZERO, leadSessionId: null }, sessions: [] };
      c.plans.set(s.plan, p);
    }
    if (s.plan_lead) p.sessions.unshift(node); else p.sessions.push(node);
  }
  return {
    contexts: [...ctxs.values()].map((c) => ({ ...c.node, sessions: c.sessions, plans: [...c.plans.values()].map((p) => ({ ...p.node, sessions: p.sessions })) })),
    cursor: input.cursor,
  };
}

function strList(o: Obj, f: string): string[] {
  const v = o[f];
  if (v === undefined) return [];
  if (!Array.isArray(v) || v.some((x) => typeof x !== "string")) return bad("plan show", f);
  return v as string[];
}
export function buildPlanView(raw: RawPlanShow): PlanViewModel {
  const c = "plan show";
  const workflows: WorkflowView[] = raw.workflows.map((w) => {
    const pr = w["pr"];
    const pull = w["pull_request"];
    const prObj = pr === null || pr === undefined ? null : obj(pr, c, "pr");
    const pullObj = pull === null || pull === undefined ? null : obj(pull, c, "pull_request");
    const opt = (o: Obj | null, f: string): StrOrNull => (o !== null && typeof o[f] === "string" ? (o[f] as string) : null);
    return {
      key: str(w, "key", c), title: str(w, "title", c), repo: str(w, "repo", c), base: str(w, "base", c), status: str(w, "status", c),
      needs: strList(w, "needs"), jobKeys: strList(w, "jobs"),
      pr: prObj === null ? null : { number: num(prObj, "number", c), url: typeof prObj["url"] === "string" ? prObj["url"] : "", state: opt(pullObj, "state"), checks: opt(pullObj, "checks"), mergeable: opt(pullObj, "mergeable") },
    };
  });
  const jobs: JobView[] = raw.jobs.map((j) => {
    const key = str(j, "key", c);
    const attempts: AttemptView[] = (j["attempts"] === undefined ? [] : arr(j, "attempts", c)).map((a) => {
      const o = obj(a, c, "attempts[]");
      return { id: str(o, "id", c), number: num(o, "number", c), tier: str(o, "tier", c), status: str(o, "status", c), outcome: strN(o, "outcome", c), cause: strN(o, "cause", c), workerSession: strN(o, "worker_session", c) };
    });
    return {
      key, title: str(j, "title", c), kind: str(j, "kind", c), status: str(j, "status", c), tier: str(j, "tier", c), needs: strList(j, "needs"),
      workflow: workflows.find((w) => w.jobKeys.includes(key))?.key ?? null, attempts,
    };
  });
  const r = raw.review;
  return {
    planId: raw.plan.id, contextId: raw.plan.context, title: raw.plan.title, phase: raw.plan.phase, counts: raw.counts,
    review: r === null ? null : { id: str(r, "id", c), status: str(r, "status", c), current: bool(r, "current", c) },
    openEscalations: raw.escalations.length, workflows, jobs, cursor: raw.cursor,
  };
}

const SNOOZES: readonly [string, "15m" | "1h" | "tomorrow"][] = [["Snooze 15m", "15m"], ["Snooze 1h", "1h"], ["Snooze until tomorrow", "tomorrow"]];
function rowActions(id: number, kind: string): InboxRowAction[] {
  const acts: InboxRowAction[] = [];
  const add = (label: string, action: CockpitAction) => acts.push({ label, action });
  if (kind === "review-requested") add("Claim", { kind: "inbox.claim", itemId: id });
  if (kind === "deploy") add("Approve", { kind: "inbox.approve", itemId: id });
  for (const [label, duration] of SNOOZES) add(label, { kind: "inbox.snooze", itemId: id, duration });
  add("Dismiss", { kind: "inbox.dismiss", itemId: id });
  return acts;
}
export function buildInbox(parsed: ParsedInbox): InboxModel {
  const rows: InboxRow[] = parsed.rows.map((i) => ({
    id: i.id ?? -1, kind: i.kind, label: i.label, urgency: i.urgency, contextId: i.context, planId: i.plan, jobKey: i.job,
    sessionId: i.session, url: i.url, openedAt: i.opened_at, ageSeconds: i.age_seconds,
    actions: i.id === null ? [] : rowActions(i.id, i.kind),
  }));
  return { openCount: rows.length, engineRunning: parsed.engineRunning, hidden: parsed.hidden, rows, cursor: parsed.cursor };
}

export function createReadModel(cpd: Cpd): ReadModel {
  async function loadPanes(panes: PaneSet, planId: string | null): Promise<PaneUpdate> {
    const wantTree = panes.has("tree");
    const wantInbox = panes.has("inbox") || wantTree;
    const wantPlan = panes.has("plan") && planId !== null;
    const [ctxR, planR, sessR, inboxR, showR] = await Promise.all([
      wantTree ? cpd.json(["context", "list"]) : undefined,
      wantTree ? cpd.json(["plan", "list"]) : undefined,
      wantTree ? cpd.json(["session", "list"]) : undefined,
      wantInbox ? cpd.json(["inbox", "list"]) : undefined,
      wantPlan ? cpd.json(["plan", "show", "--plan", planId]) : undefined,
    ]);
    const out: { tree?: ContextTreeModel; plan?: PlanViewModel | null; inbox?: InboxModel } = {};
    const inbox = inboxR === undefined ? undefined : parseInboxList(inboxR);
    if (ctxR !== undefined && planR !== undefined && sessR !== undefined && inbox !== undefined) {
      out.tree = buildTree({
        contexts: parseContextList(ctxR), plans: parsePlanList(planR), sessions: parseSessionList(sessR), inbox: inbox.rows,
        cursor: Math.min(cursorOf(ctxR, "context list"), cursorOf(planR, "plan list"), cursorOf(sessR, "session list"), inbox.cursor),
      });
    }
    if (panes.has("inbox") && inbox !== undefined) out.inbox = buildInbox(inbox);
    if (panes.has("plan")) out.plan = showR === undefined ? null : buildPlanView(parsePlanShow(showR));
    return out;
  }
  return {
    async loadAll(planId) {
      const u = await loadPanes(new Set<CockpitPane>(["tree", "plan", "inbox"]), planId);
      return { tree: u.tree!, plan: u.plan ?? null, inbox: u.inbox! };
    },
    loadPanes,
  };
}
