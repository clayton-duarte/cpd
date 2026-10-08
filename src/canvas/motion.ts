/**
 * Level-transition motion. Values come from Material Design 3 motion tokens
 * (duration-medium4 = 400ms, duration-medium2 = 300ms) and M3's directional
 * easing: entering uses emphasized-decelerate, exiting uses emphasized-accelerate.
 * Descend and ascend are deliberately asymmetric -- descending builds a mental
 * model of an unknown graph, ascending returns to a remembered one.
 */
export const DESCEND_MS = 400;
export const ASCEND_MS = 300;
export const REDUCED_FADE_MS = 100;

/** M3 emphasized-decelerate: begins at peak velocity, comes to a gentle rest. */
export const EASE_DESCEND = [0.05, 0.7, 0.1, 1] as const;
/** M3 emphasized-accelerate: begins at rest, ends at peak velocity. */
export const EASE_ASCEND = [0.3, 0, 0.8, 0.15] as const;

const NEWTON_ITERATIONS = 8;
const NEWTON_MIN_SLOPE = 1e-6;
const SUBDIVISION_PRECISION = 1e-7;
const SUBDIVISION_MAX_ITERATIONS = 10;

function a(aA1: number, aA2: number): number {
  return 1.0 - 3.0 * aA2 + 3.0 * aA1;
}
function b(aA1: number, aA2: number): number {
  return 3.0 * aA2 - 6.0 * aA1;
}
function c(aA1: number): number {
  return 3.0 * aA1;
}

function calcBezier(aT: number, aA1: number, aA2: number): number {
  return ((a(aA1, aA2) * aT + b(aA1, aA2)) * aT + c(aA1)) * aT;
}

function getSlope(aT: number, aA1: number, aA2: number): number {
  return 3.0 * a(aA1, aA2) * aT * aT + 2.0 * b(aA1, aA2) * aT + c(aA1);
}

/** Compile a cubic-bezier control-point tuple into an easing function (t: 0..1 -> 0..1). */
export function cubicBezier(
  p1x: number,
  p1y: number,
  p2x: number,
  p2y: number,
): (t: number) => number {
  if (p1x === p1y && p2x === p2y) return (t: number) => t;

  function getTForX(aX: number): number {
    let aGuessT = aX;
    for (let i = 0; i < NEWTON_ITERATIONS; i += 1) {
      const currentSlope = getSlope(aGuessT, p1x, p2x);
      if (currentSlope === 0) break;
      const currentX = calcBezier(aGuessT, p1x, p2x) - aX;
      aGuessT -= currentX / currentSlope;
    }

    // Newton converged poorly (flat slope) -- fall back to bisection.
    let lo = 0;
    let hi = 1;
    let guess = aGuessT;
    if (guess < lo || guess > hi || Math.abs(getSlope(guess, p1x, p2x)) < NEWTON_MIN_SLOPE) {
      guess = aX;
      for (let i = 0; i < SUBDIVISION_MAX_ITERATIONS; i += 1) {
        const currentX = calcBezier(guess, p1x, p2x) - aX;
        if (Math.abs(currentX) < SUBDIVISION_PRECISION) break;
        if (currentX > 0) hi = guess;
        else lo = guess;
        guess = (hi + lo) / 2;
      }
    }
    return guess;
  }

  return (t: number) => {
    if (t <= 0) return 0;
    if (t >= 1) return 1;
    return calcBezier(getTForX(t), p1y, p2y);
  };
}

/** True when the user has asked the OS to reduce motion. */
export function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}
