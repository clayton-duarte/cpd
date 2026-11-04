export interface SelectionStorage { getItem(key: string): string | null; setItem(key: string, value: string): void }
export interface PersistedSelection { planId: string; sessionKey: string }
export const SELECTION_KEY = 'cpd.selection';
export function saveSelection(_storage: SelectionStorage, _sel: PersistedSelection): void {
  throw new Error('not implemented');
}
export function loadSelection(_storage: SelectionStorage): PersistedSelection | null {
  throw new Error('not implemented');
}
