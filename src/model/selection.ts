/**
 * Pure selection-movement logic for the Jobs-level canvas (D3).
 * No React, no DOM -- same discipline as navigation.ts / derive.ts.
 */

/**
 * Move the selected job id within an ordered list of job ids.
 *
 * - From `null` (nothing selected), direction 1 or -1 selects the first id.
 * - Moving past either end clamps (no wrap): direction 1 at the last id
 *   stays on the last id; direction -1 at the first id stays on the first id.
 * - An empty list always returns null.
 */
export function nextJob(ids: string[], current: string | null, direction: 1 | -1): string | null {
  if (ids.length === 0) return null;

  if (current === null) {
    return direction === 1 ? ids[0] : ids[0];
  }

  const index = ids.indexOf(current);
  if (index === -1) {
    return ids[0];
  }

  const nextIndex = index + direction;
  if (nextIndex < 0) return ids[0];
  if (nextIndex >= ids.length) return ids[ids.length - 1];
  return ids[nextIndex];
}
