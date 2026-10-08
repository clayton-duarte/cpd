import { describe, expect, it, vi, afterEach } from 'vitest';
import { cubicBezier, EASE_DESCEND, EASE_ASCEND, DESCEND_MS, ASCEND_MS, prefersReducedMotion } from './motion';

function monotonicNonDecreasing(fn: (t: number) => number): boolean {
  let prev = fn(0);
  for (let i = 1; i <= 100; i += 1) {
    const t = i / 100;
    const value = fn(t);
    if (value < prev - 1e-9) return false;
    prev = value;
  }
  return true;
}

describe('cubicBezier', () => {
  it('endpoints are 0 and 1 for the descend curve', () => {
    const ease = cubicBezier(...EASE_DESCEND);
    expect(ease(0)).toBeCloseTo(0);
    expect(ease(1)).toBeCloseTo(1);
  });

  it('endpoints are 0 and 1 for the ascend curve', () => {
    const ease = cubicBezier(...EASE_ASCEND);
    expect(ease(0)).toBeCloseTo(0);
    expect(ease(1)).toBeCloseTo(1);
  });

  it('descend curve is monotonically non-decreasing', () => {
    expect(monotonicNonDecreasing(cubicBezier(...EASE_DESCEND))).toBe(true);
  });

  it('ascend curve is monotonically non-decreasing', () => {
    expect(monotonicNonDecreasing(cubicBezier(...EASE_ASCEND))).toBe(true);
  });
});

describe('motion tokens', () => {
  it('descend and ascend easing tuples differ', () => {
    expect(EASE_DESCEND).not.toEqual(EASE_ASCEND);
  });

  it('descend takes longer than ascend -- asymmetric by design', () => {
    expect(DESCEND_MS).toBeGreaterThan(ASCEND_MS);
  });
});

describe('prefersReducedMotion', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('returns false when matchMedia is absent', () => {
    const original = window.matchMedia;
    // @ts-expect-error -- simulating jsdom's lack of matchMedia
    delete window.matchMedia;
    expect(prefersReducedMotion()).toBe(false);
    window.matchMedia = original;
  });
});
