export function whenApplicable(_deps: { isFocused: () => boolean; systemIdleSeconds: () => number; timers: import('./types').UpdateTimers; pollMs?: number; idleSeconds?: number }): Promise<void> {
  throw new Error('not implemented');
}
