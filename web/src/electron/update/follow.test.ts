import { describe, expect, it } from 'vitest';
import { fakeTimers } from './fakes';
import { followMerges, parseEventId, type FollowProcess } from './follow';

const BASE = ['events', 'follow', '--kind', 'pr-merged'];

function setup(restartMs?: number) {
  const timers = fakeTimers();
  const spawned: { args: string[]; line(l: string): void; exit(c: number | null): void; killed: boolean }[] = [];
  let merged = 0;
  const errors: Error[] = [];
  const spawn = (args: string[]): FollowProcess => {
    let lineCb: (l: string) => void = () => {};
    let exitCb: (c: number | null) => void = () => {};
    const rec = {
      args,
      killed: false,
      line: (l: string) => lineCb(l),
      exit: (c: number | null) => exitCb(c),
    };
    spawned.push(rec);
    return {
      onLine: (cb) => void (lineCb = cb),
      onExit: (cb) => void (exitCb = cb),
      kill: () => void (rec.killed = true),
    };
  };
  const handle = followMerges({ spawn, timers, onMerged: () => void merged++, onError: (e) => void errors.push(e), restartMs });
  return { handle, timers, spawned, errors, merged: () => merged };
}

function failure(fn: () => unknown): Error {
  try {
    fn();
  } catch (e) {
    expect((e as Error).message).not.toBe('not implemented');
    return e as Error;
  }
  throw new Error('expected a throw');
}

const merged = (id: number) => JSON.stringify({ id, kind: 'pr-merged' });

describe('parseEventId', () => {
  it('parseEventId returns the numeric id of a pr-merged line', () => {
    expect(parseEventId(merged(42))).toBe(42);
  });
  it('parseEventId throws on non-JSON', () => {
    expect(failure(() => parseEventId('not json'))).toBeInstanceOf(Error);
  });
  it('parseEventId throws when the id is missing', () => {
    expect(failure(() => parseEventId(JSON.stringify({ kind: 'pr-merged' })))).toBeInstanceOf(Error);
  });
  it('parseEventId throws on another kind', () => {
    expect(failure(() => parseEventId(JSON.stringify({ id: 1, kind: 'pr-opened' })))).toBeInstanceOf(Error);
  });
});

describe('followMerges', () => {
  it('followMerges spawns without since and calls onMerged once at start', () => {
    const s = setup();
    expect(s.spawned.map((p) => p.args)).toEqual([BASE]);
    expect(s.merged()).toBe(1);
  });
  it('followMerges calls onMerged for each line', () => {
    const s = setup();
    s.spawned[0]?.line(merged(1));
    s.spawned[0]?.line(merged(2));
    expect(s.merged()).toBe(3);
  });
  it('followMerges reports a malformed line and keeps following', () => {
    const s = setup();
    s.spawned[0]?.line('garbage');
    expect(s.errors).toHaveLength(1);
    s.spawned[0]?.line(merged(5));
    expect(s.merged()).toBe(2);
    expect(s.spawned[0]?.killed).toBe(false);
  });
  it('followMerges restarts with since after exit', () => {
    const s = setup();
    s.spawned[0]?.line(merged(7));
    s.spawned[0]?.exit(1);
    expect(s.timers.pending.map((t) => t.ms)).toEqual([5000]);
    s.timers.fire();
    expect(s.spawned[1]?.args).toEqual([...BASE, '--since', '7']);
    expect(s.merged()).toBe(3);
  });
  it('followMerges restarts without since when no event was seen', () => {
    const s = setup(1000);
    s.spawned[0]?.exit(null);
    expect(s.timers.pending.map((t) => t.ms)).toEqual([1000]);
    s.timers.fire();
    expect(s.spawned[1]?.args).toEqual(BASE);
  });
  it('followMerges stop kills the process and cancels the restart', () => {
    const s = setup();
    s.spawned[0]?.exit(0);
    expect(s.timers.pending).toHaveLength(1);
    s.handle.stop();
    expect(s.spawned[0]?.killed).toBe(true);
    expect(s.timers.pending).toHaveLength(0);
    expect(s.spawned).toHaveLength(1);
  });
});
