export interface SelectionStorage { getItem(key: string): string | null; setItem(key: string, value: string): void }
export interface PersistedSelection { planId: string; sessionKey: string }
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
