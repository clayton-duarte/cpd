import type { ContextTreeModel, PlanViewModel } from '../../protocol/cockpit.ts';

export interface SelectionStorage { getItem(key: string): string | null; setItem(key: string, value: string): void }
export interface PersistedSelection { planId: string | null; sessionKey: string }
export const SELECTION_KEY = 'cpd.selection';
export function saveSelection(storage: SelectionStorage, sel: PersistedSelection): void {
  storage.setItem(SELECTION_KEY, JSON.stringify({ planId: sel.planId, sessionKey: sel.sessionKey }));
}
export function loadSelection(storage: SelectionStorage): PersistedSelection | null {
  const raw = storage.getItem(SELECTION_KEY);
  if (raw === null) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof parsed !== 'object' || parsed === null) return null;
  const { planId, sessionKey } = parsed as Record<string, unknown>;
  if ((typeof planId !== 'string' && planId !== null) || typeof sessionKey !== 'string') return null;
  return { planId, sessionKey };
}

/** The Plan id of the selected session when that session is a Lead of a Plan; null for a Manager or no selection. */
export function activeLeadPlanId(tree: ContextTreeModel | null, activeSessionId: string | null): string | null {
  if (tree === null || activeSessionId === null) return null;
  for (const c of tree.contexts) {
    for (const p of c.plans) {
      if (p.sessions.some((s) => s.ref.id === activeSessionId)) return p.id;
    }
  }
  return null;
}
/** The Plan to render: the loaded plan only when it is the active Lead's Plan; null otherwise. */
export function visiblePlan(activeLeadPlanId: string | null, plan: PlanViewModel | null): PlanViewModel | null {
  if (activeLeadPlanId === null || plan === null) return null;
  return plan.planId === activeLeadPlanId ? plan : null;
}
