import type { UpdateTimers } from './types';

/** The View menu signal (web/CONTRACT.md §19.2): every View menu change calls the handler; the return unsubscribes. */
export interface ViewChanges { onViewChange(handler: () => void): () => void }

export function whenApplicable(deps: { isFocused: () => boolean; systemIdleSeconds: () => number; timers: UpdateTimers; viewChanges?: ViewChanges; pollMs?: number; idleSeconds?: number }): Promise<void> {
  const pollMs = deps.pollMs ?? 30000;
  const idleSeconds = deps.idleSeconds ?? 300;
  return new Promise((resolve) => {
    const check = (): void => {
      if (!deps.isFocused() || deps.systemIdleSeconds() >= idleSeconds) resolve();
      else deps.timers.setTimeout(check, pollMs);
    };
    check();
  });
}
