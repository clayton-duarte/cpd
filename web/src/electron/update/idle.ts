import type { UpdateTimers } from './types';

export function whenApplicable(deps: { isFocused: () => boolean; systemIdleSeconds: () => number; timers: UpdateTimers; pollMs?: number; idleSeconds?: number }): Promise<void> {
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
