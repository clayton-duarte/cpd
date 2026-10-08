import { useReactFlow } from '@xyflow/react';
import { ASCEND_MS, DESCEND_MS, EASE_ASCEND, EASE_DESCEND, cubicBezier, prefersReducedMotion } from './motion';

export type TransitionDirection = 'descend' | 'ascend';

/**
 * Returns a function that moves the React Flow camera between canvas
 * levels. Fire-and-forget: the promise `fitView` returns resolves via d3's
 * 'end' event, which does not fire on an interrupted transition (a second,
 * faster navigation fires 'interrupt' instead) -- awaiting it can hang
 * forever, so it is intentionally not awaited here.
 */
export function useLevelTransition(): (direction: TransitionDirection) => void {
  const { fitView } = useReactFlow();

  return (direction: TransitionDirection) => {
    const reduced = prefersReducedMotion();
    const duration = reduced ? 0 : direction === 'descend' ? DESCEND_MS : ASCEND_MS;
    const ease = direction === 'descend' ? cubicBezier(...EASE_DESCEND) : cubicBezier(...EASE_ASCEND);

    // Fire and forget -- do not await, see note above.
    void fitView({ padding: 0.1, duration, ease });
  };
}
