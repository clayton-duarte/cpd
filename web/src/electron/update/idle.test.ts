import { describe, expect, it } from 'vitest';
import { fakeTimers } from './fakes';
import { whenApplicable } from './idle';

function run(state: { focused: boolean; idle: number }) {
  const timers = fakeTimers();
  let done = false;
  const p = whenApplicable({ isFocused: () => state.focused, systemIdleSeconds: () => state.idle, timers }).then(() => void (done = true));
  return { timers, p, done: () => done };
}

describe('whenApplicable', () => {
  it('whenApplicable resolves at once when the window is unfocused', async () => {
    const r = run({ focused: false, idle: 0 });
    await r.p;
    expect(r.timers.pending).toHaveLength(0);
  });
  it('whenApplicable resolves at once when the system idle time is at least 300 seconds', async () => {
    const r = run({ focused: true, idle: 300 });
    await r.p;
    expect(r.timers.pending).toHaveLength(0);
  });
  it('whenApplicable polls every 30 seconds while focused and active', async () => {
    const r = run({ focused: true, idle: 299 });
    await Promise.resolve();
    expect(r.done()).toBe(false);
    expect(r.timers.pending.map((t) => t.ms)).toEqual([30000]);
    r.timers.fire();
    expect(r.done()).toBe(false);
    expect(r.timers.pending.map((t) => t.ms)).toEqual([30000]);
  });
  it('whenApplicable resolves on the poll where focus is lost', async () => {
    const state = { focused: true, idle: 0 };
    const r = run(state);
    await Promise.resolve();
    expect(r.done()).toBe(false);
    state.focused = false;
    r.timers.fire();
    await r.p;
    expect(r.done()).toBe(true);
  });
});
