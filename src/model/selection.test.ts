import { describe, expect, it } from 'vitest';
import { nextJob } from './selection';

describe('nextJob', () => {
  it('from null with direction 1 returns the first id', () => {
    expect(nextJob(['a', 'b', 'c'], null, 1)).toBe('a');
  });

  it('at the last id with direction 1 stays on the last id (no wrap)', () => {
    expect(nextJob(['a', 'b', 'c'], 'c', 1)).toBe('c');
  });

  it('at the first id with direction -1 stays on the first id', () => {
    expect(nextJob(['a', 'b', 'c'], 'a', -1)).toBe('a');
  });

  it('an empty list returns null', () => {
    expect(nextJob([], null, 1)).toBeNull();
    expect(nextJob([], 'a', 1)).toBeNull();
  });

  it('moves forward and backward through the middle', () => {
    expect(nextJob(['a', 'b', 'c'], 'a', 1)).toBe('b');
    expect(nextJob(['a', 'b', 'c'], 'c', -1)).toBe('b');
  });
});
