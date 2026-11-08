import type { UpdateTimers } from './types';

/** The View menu signal (web/CONTRACT.md §19.2): every View menu change calls the handler; the return unsubscribes. */
export interface ViewChanges { onViewChange(handler: () => void): () => void }

export function whenApplicable(deps: { isFocused: () => boolean; systemIdleSeconds: () => number; timers: UpdateTimers; viewChanges?: ViewChanges; pollMs?: number; idleSeconds?: number }): Promise<void> {
  const pollMs = deps.pollMs ?? 30000;
  const idleSeconds = deps.idleSeconds ?? 300;
  return new Promise((resolve) => {
    let timer: unknown;
    let unsubscribe: (() => void) | undefined;
    let settled = false;
    const finish = (): void => {
      if (settled) return;
      settled = true;
      if (timer !== undefined) deps.timers.clearTimeout(timer);
      unsubscribe?.();
      resolve();
    };
    const check = (): void => {
      // §19.2: a View menu change is an applicability moment even while the window is focused and
      // the system is not idle, so the pending swap runs at that moment.
      if (!deps.isFocused() || deps.systemIdleSeconds() >= idleSeconds) finish();
      else timer = deps.timers.setTimeout(check, pollMs);
    };
    unsubscribe = deps.viewChanges?.onViewChange(finish);
    check();
  });
}
