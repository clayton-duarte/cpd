import type { ContextNode, ContextTreeModel, PlanNode, SessionDot, SessionNode } from "../protocol/cockpit.ts";

/** Standing Contexts in TUI order (PermanentContext::ALL), never sorted. The one list the tree and navigation share. */
export const PERMANENT_IDS: readonly string[] = ["cpd", "host", "codereview"];
export const isPermanent = (id: string): boolean => PERMANENT_IDS.includes(id);

export const STOPPED: SessionDot = { state: "stopped", attention: null };
export const createdOf = (n: object): string => (n as { createdAt?: string }).createdAt ?? "";
export const leadOf = (p: PlanNode): SessionNode | undefined => p.sessions.find((s) => s.ref.id === p.leadSessionId);
export const managerOf = (c: ContextNode): SessionNode | undefined =>
  c.sessions.find((s) => s.ref.role === "manager" && !s.archived);

/** TUI Dot::tier: red 0, yellow 1, magenta 2, cyan 3, everything else (and working) 4. */
function dotTier(dot: SessionDot): number {
  if (dot.state === "working") return 4;
  switch (dot.attention) {
    case "red": return 0;
    case "yellow": return 1;
    case "magenta": return 2;
    case "cyan": return 3;
    default: return 4;
  }
}

/** (tier, newest first); the stable sort keeps input order on ties. */
export function compareOrderKey(aDot: SessionDot, aCreated: string, bDot: SessionDot, bCreated: string): number {
  const tierDiff = dotTier(aDot) - dotTier(bDot);
  if (tierDiff !== 0) return tierDiff;
  if (aCreated > bCreated) return -1;
  if (aCreated < bCreated) return 1;
  return 0;
}

/** Live Plans of a Context in the order the tree shows them. */
export const livePlans = (c: ContextNode): PlanNode[] =>
  c.plans
    .filter((p) => !p.archived)
    .sort((a, b) => compareOrderKey(leadOf(a)?.dot ?? STOPPED, createdOf(a), leadOf(b)?.dot ?? STOPPED, createdOf(b)));

/** Live non-standing Contexts in the order the tree shows them. */
export const liveProjectContexts = (tree: ContextTreeModel): ContextNode[] =>
  tree.contexts
    .filter((c) => !c.archived && !isPermanent(c.id))
    .sort((a, b) => compareOrderKey(managerOf(a)?.dot ?? STOPPED, createdOf(a), managerOf(b)?.dot ?? STOPPED, createdOf(b)));

export interface Selection { readonly sessionId: string; readonly planId: string | null }

interface Found { readonly context: ContextNode; readonly plan: PlanNode | null; readonly node: SessionNode }

function find(tree: ContextTreeModel | null, sessionId: string): Found | null {
  if (tree === null) return null;
  for (const context of tree.contexts) {
    for (const node of context.sessions) if (node.ref.id === sessionId) return { context, plan: null, node };
    for (const plan of context.plans) {
      for (const node of plan.sessions) if (node.ref.id === sessionId) return { context, plan, node };
    }
  }
  return null;
}

const isArchived = (f: Found): boolean => f.node.archived || f.context.archived || (f.plan?.archived ?? false);

/** True when the session sits in the tree and it, its Plan or its Context is archived. */
export const sessionArchived = (tree: ContextTreeModel | null, sessionId: string | null): boolean => {
  if (sessionId === null) return false;
  const f = find(tree, sessionId);
  return f !== null && isArchived(f);
};

const managerSelection = (c: ContextNode): Selection | null => {
  const m = managerOf(c);
  return m === undefined ? null : { sessionId: m.ref.id, planId: null };
};
const leadSelection = (p: PlanNode): Selection | null => {
  const l = leadOf(p);
  return l === undefined ? null : { sessionId: l.ref.id, planId: p.id };
};
const firstPlanLead = (c: ContextNode): Selection | null => {
  for (const p of livePlans(c)) {
    const s = leadSelection(p);
    if (s !== null) return s;
  }
  return null;
};

/**
 * The new selection when the selected session goes from live to archived between two trees; null when
 * nothing changes (still live, already archived, or not in the tree). An archived row opened on purpose
 * stays: only a live-to-archived transition navigates.
 */
export function archiveNavigation(prev: ContextTreeModel | null, next: ContextTreeModel, selectedSessionId: string | null): Selection | null {
  if (selectedSessionId === null) return null;
  const before = find(prev, selectedSessionId);
  const after = find(next, selectedSessionId);
  if (before === null || after === null || isArchived(before) || !isArchived(after)) return null;
  const context = next.contexts.find((c) => c.id === after.context.id);
  if (context !== undefined && !context.archived) {
    const inContext = firstPlanLead(context) ?? managerSelection(context);
    if (inContext !== null) return inContext;
  }
  for (const c of liveProjectContexts(next)) {
    if (c.id === after.context.id) continue;
    const s = managerSelection(c) ?? firstPlanLead(c);
    if (s !== null) return s;
  }
  for (const id of ["cpd", "host"]) {
    const c = next.contexts.find((x) => x.id === id && !x.archived);
    const s = c === undefined ? null : managerSelection(c);
    if (s !== null) return s;
  }
  return null;
}
