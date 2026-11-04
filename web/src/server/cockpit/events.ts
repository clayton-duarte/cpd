import type { ActionOutput, CockpitAction } from "../../protocol/cockpit.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";
import type { CockpitPane, CockpitPaneData } from "../../protocol/wire.ts";
import type { ContextTreeModel, CostModel, InboxModel, LogRow, PlanViewModel, SettingsChanges, SettingsModel, StatusModel } from "../../protocol/cockpit.ts";
import type { WireError } from "../../protocol/domain.ts";
import { WebError, toWireError } from "../../protocol/domain.ts";
import { followArgs, type Cpd, type FollowBoundary, type FollowProcess } from "./cli.ts";
import { runAction } from "./actions.ts";
import type { PaneUpdate, ReadModel } from "./readmodel.ts";

export interface CpdEvent { readonly id: number; readonly kind: string; readonly plan: string | null; readonly context: string | null; readonly session: string | null; readonly job: string | null; readonly attempt: string | null; readonly at: string; readonly text: string; readonly data: Readonly<Record<string, unknown>> }
function malformed(line: string, field: string): never {
  throw new WebError("cpd_malformed_json", `event line: missing or mistyped field ${field}`, { field, command: line.slice(0, 200) });
}
export function parseEventLine(line: string): CpdEvent {
  let v: unknown;
  try { v = JSON.parse(line); } catch { return malformed(line, "json"); }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return malformed(line, "json");
  const o = v as Record<string, unknown>;
  if (typeof o["id"] !== "number") return malformed(line, "id");
  if (typeof o["kind"] !== "string") return malformed(line, "kind");
  const s = (f: string): string | null => (typeof o[f] === "string" ? (o[f] as string) : null);
  return { id: o["id"], kind: o["kind"], plan: s("plan"), context: s("context"), session: s("session"), job: s("job"), attempt: s("attempt"), at: s("at") ?? "", text: s("text") ?? "", data: (typeof o["data"] === "object" && o["data"] !== null && !Array.isArray(o["data"]) ? o["data"] : {}) as Record<string, unknown> };
}

const TREE_KINDS = new Set("context-created context-grounded context-record context-renamed context-archived context-curated context-owner context-deleted plan-created plan-phase plan-done plan-renamed plan-archived plan-review plan-outlined plan-deleted job-added job-addendum job-blocked job-cancel-requested job-edited job-merged job-failed job-retried job-cancelled attempt-started attempt-retry attempt-status attempt-ended attempt-aborted attempt-nudge all-merged workflow-added workflow-base-changed workflow-described workflow-edited workflow-cancelled workflow-started pr-added pr-base-changed pr-cancelled pr-described pr-edited pr-feedback pr-opened pr-started pr-merged escalation escalation-answered escalation-expired review-opened review-closed session-registered session-state session-moved session-renamed session-archived session-asleep session-message session-resumed session-retiered inbox-opened inbox-closed inbox-claimed inbox-snoozed inbox-dismissed watch-started watch-stopped incident-started incident-ended incident-plans error".split(" "));
const INBOX_KINDS = new Set("inbox-opened inbox-closed inbox-claimed inbox-snoozed inbox-dismissed session-state session-message session-resumed plan-created plan-review job-failed job-blocked pr-opened pr-merged pr-feedback watch-started watch-stopped incident-started incident-ended session-registered session-moved".split(" "));

export function panesFor(event: CpdEvent, selectedPlan: { id: string; contextId: string } | null): ReadonlySet<CockpitPane> {
  const panes = new Set<CockpitPane>();
  if (TREE_KINDS.has(event.kind)) panes.add("tree");
  if (INBOX_KINDS.has(event.kind)) panes.add("inbox");
  const known = TREE_KINDS.has(event.kind) || INBOX_KINDS.has(event.kind);
  if (known && selectedPlan !== null && (event.plan === selectedPlan.id || event.context === selectedPlan.contextId || event.kind === "session-moved")) panes.add("plan");
  return panes;
}

export const COALESCE_MS = 250;
export const SAFETY_REFRESH_MS = 30_000;
export const FOLLOW_RESTART_BASE_MS = 1_000;

export type CockpitUpdate = ({ kind: "pane" } & CockpitPaneData) | { kind: "error"; error: WireError };
export interface CockpitSnapshot { tree: ContextTreeModel | null; plan: PlanViewModel | null; inbox: InboxModel | null; status?: StatusModel; costs?: CostModel }

/** The machine status, costs, logs and settings reads; absent in tests of the panes alone. */
export interface Extras {
  status(): Promise<StatusModel>;
  costs(): Promise<CostModel>;
  logs(): Promise<LogRow[]>;
  settings(): Promise<SettingsModel>;
  saveSettings(changes: SettingsChanges): Promise<void>;
}

export interface CockpitService {
  start(): Promise<void>;
  stop(): void;
  snapshot(): CockpitSnapshot;
  selectedPlanId(): string | null;
  selectPlan(planId: string | null): Promise<void>;
  subscribe(listener: (u: CockpitUpdate) => void): Disposable;
  runAction(action: CockpitAction): Promise<ActionOutput>;
  readLogs(): Promise<LogRow[]>;
  readSettings(): Promise<SettingsModel>;
  saveSettings(changes: SettingsChanges): Promise<void>;
}
const RESTART_CAP_MS = 30_000;
const CURSOR_AHEAD = 3;

export function createCockpitService(deps: { readModel: ReadModel; cpd: Cpd; follow: FollowBoundary; scheduler: Scheduler; extras?: Extras }): CockpitService {
  const { readModel, cpd, follow, scheduler, extras } = deps;
  const listeners = new Set<(u: CockpitUpdate) => void>();
  let snap: CockpitSnapshot = { tree: null, plan: null, inbox: null };
  let planId: string | null = null;
  let started = false;
  let stopped = false;
  let lastCursor = 0;
  let restarts = 0;
  let proc: FollowProcess | null = null;
  let procSubs: Disposable[] = [];
  let pending = new Set<CockpitPane>();
  let coalesce: Disposable | null = null;
  let restart: Disposable | null = null;
  let safety: Disposable | null = null;

  const emit = (u: CockpitUpdate): void => {
    if (stopped) return;
    for (const l of [...listeners]) l(u);
  };
  const emitError = (e: unknown): void => emit({ kind: "error", error: toWireError(e) });
  const apply = (u: PaneUpdate): void => {
    if (stopped) return;
    if (u.tree !== undefined) { snap = { ...snap, tree: u.tree }; emit({ kind: "pane", pane: "tree", data: u.tree }); }
    if (u.plan !== undefined) { snap = { ...snap, plan: u.plan }; emit({ kind: "pane", pane: "plan", data: u.plan }); }
    if (u.inbox !== undefined) { snap = { ...snap, inbox: u.inbox }; emit({ kind: "pane", pane: "inbox", data: u.inbox }); }
  };
  let running = false;
  let queued: Set<CockpitPane> | null = null;
  const refresh = async (panes: ReadonlySet<CockpitPane>): Promise<void> => {
    if (running) { queued = new Set([...(queued ?? []), ...panes]); return; }
    running = true;
    try {
      let next: ReadonlySet<CockpitPane> | null = panes;
      while (next !== null && !stopped) {
        try { apply(await readModel.loadPanes(next, planId)); } catch (e) { emitError(e); }
        next = queued;
        queued = null;
      }
    } finally { running = false; }
  };
  const livePanes = (): Set<CockpitPane> => new Set<CockpitPane>(planId !== null ? ["tree", "inbox", "plan"] : ["tree", "inbox"]);
  const refreshExtras = async (): Promise<void> => {
    if (extras === undefined) return;
    try {
      const status = await extras.status();
      if (stopped) return;
      snap = { ...snap, status };
      emit({ kind: "pane", pane: "status", data: status });
    } catch (e) { emitError(e); }
    try {
      const costs = await extras.costs();
      if (stopped) return;
      snap = { ...snap, costs };
      emit({ kind: "pane", pane: "costs", data: costs });
    } catch (e) { emitError(e); }
  };
  const need = (): Extras => {
    if (extras === undefined) throw new WebError("internal", "settings and logs are not available");
    return extras;
  };
  const selected = (): { id: string; contextId: string } | null =>
    planId !== null && snap.plan !== null ? { id: planId, contextId: snap.plan.contextId } : planId !== null ? { id: planId, contextId: "" } : null;

  const scheduleSafety = (): void => {
    safety = scheduler.after(SAFETY_REFRESH_MS, () => {
      safety = null;
      if (stopped) return;
      void refresh(livePanes());
      void refreshExtras();
      scheduleSafety();
    });
  };
  const onLine = (line: string): void => {
    let event: CpdEvent;
    try { event = parseEventLine(line); } catch (e) { emitError(e); return; }
    restarts = 0;
    lastCursor = Math.max(lastCursor, event.id);
    const panes = panesFor(event, selected());
    if (panes.size === 0) return;
    for (const p of panes) pending.add(p);
    if (coalesce === null) {
      coalesce = scheduler.after(COALESCE_MS, () => {
        coalesce = null;
        const batch = pending;
        pending = new Set();
        void refresh(batch);
      });
    }
  };
  const startFollow = (): void => {
    const p = follow.start(followArgs(lastCursor));
    proc = p;
    procSubs = [p.onLine(onLine), p.onExit((info) => onExit(p, info.code))];
  };
  const onExit = (p: FollowProcess, code: number | null): void => {
    if (stopped || proc !== p) return;
    for (const s of procSubs) s.dispose();
    procSubs = [];
    proc = null;
    const delay = Math.min(FOLLOW_RESTART_BASE_MS * 2 ** restarts, RESTART_CAP_MS);
    restarts++;
    restart = scheduler.after(delay, async () => {
      restart = null;
      if (stopped) return;
      if (code === CURSOR_AHEAD) {
        try {
          const all = await readModel.loadAll(planId);
          if (stopped) return;
          apply(all);
          lastCursor = all.tree.cursor;
        } catch (e) { emitError(e); }
      }
      if (!stopped) startFollow();
    });
  };

  return {
    async start() {
      if (started) return;
      started = true;
      try {
        const all = await readModel.loadAll(planId);
        snap = all;
        lastCursor = all.tree.cursor;
      } catch (e) { started = false; throw e; }
      if (stopped) return;
      startFollow();
      scheduleSafety();
      void refreshExtras();
    },
    stop() {
      stopped = true;
      for (const s of procSubs) s.dispose();
      procSubs = [];
      proc?.kill();
      proc = null;
      coalesce?.dispose(); coalesce = null;
      restart?.dispose(); restart = null;
      safety?.dispose(); safety = null;
      pending = new Set();
      listeners.clear();
    },
    snapshot: () => snap,
    selectedPlanId: () => planId,
    async selectPlan(id) {
      planId = id;
      await refresh(new Set<CockpitPane>(["plan"]));
    },
    subscribe(listener) {
      listeners.add(listener);
      return { dispose: () => { listeners.delete(listener); } };
    },
    async runAction(action) {
      try { return await runAction(cpd, action); } finally { if (!stopped) void refresh(livePanes()); }
    },
    readLogs: () => need().logs(),
    readSettings: () => need().settings(),
    saveSettings: (changes) => need().saveSettings(changes),
  };
}
