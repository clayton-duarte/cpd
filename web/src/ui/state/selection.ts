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
  if (typeof planId !== 'string' || typeof sessionKey !== 'string') return null;
  return { planId, sessionKey };
}

/** The Plan id of the selected session when that session is a Lead of a Plan; null for a Manager or no selection. */
export function activeLeadPlanId(_tree: ContextTreeModel | null, _activeSessionId: string | null): string | null {
  throw new Error('not implemented');
}
/** The Plan to render: the loaded plan only when it is the active Lead's Plan; null otherwise. */
export function visiblePlan(_activeLeadPlanId: string | null, _plan: PlanViewModel | null): PlanViewModel | null {
  throw new Error('not implemented');
}
