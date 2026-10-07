import type { Job } from '../model/types';

/**
 * Longest-path layering. A job's wave is 1 + the max wave of its dependencies;
 * a job with no dependencies is wave 0.
 */
export function computeWaves(jobs: Job[]): Map<string, number> {
  const byId = new Map(jobs.map((j) => [j.id, j]));
  const waves = new Map<string, number>();
  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Map<string, number>(jobs.map((j) => [j.id, WHITE]));
  const stack: string[] = [];

  function visit(id: string): number {
    const existing = waves.get(id);
    if (existing !== undefined) return existing;

    color.set(id, GRAY);
    stack.push(id);

    const job = byId.get(id);
    const needs = job?.needs ?? [];
    let maxDepWave = -1;
    for (const dep of needs) {
      const depColor = color.get(dep);
      if (depColor === GRAY) {
        const cycleIds = [...stack.slice(stack.indexOf(dep)), dep];
        throw new Error(
          `Cycle detected in job dependency graph involving: ${cycleIds.join(' -> ')}`,
        );
      }
      const depWave = visit(dep);
      if (depWave > maxDepWave) maxDepWave = depWave;
    }

    stack.pop();
    color.set(id, BLACK);
    const wave = maxDepWave + 1;
    waves.set(id, wave);
    return wave;
  }

  for (const job of jobs) {
    visit(job.id);
  }

  return waves;
}

/** Jobs grouped by wave, each wave's jobs in stable fixture order. */
export function groupByWave(jobs: Job[]): Job[][] {
  const waves = computeWaves(jobs);
  const maxWave = jobs.reduce((max, j) => Math.max(max, waves.get(j.id) ?? 0), 0);
  const groups: Job[][] = Array.from({ length: maxWave + 1 }, () => []);
  for (const job of jobs) {
    const wave = waves.get(job.id) ?? 0;
    groups[wave].push(job);
  }
  return groups;
}

/**
 * True when every job in wave N+1 depends on every job in wave N (D74): the
 * only case where a wave boundary is a real barrier rather than a layout
 * artifact. Longest-path layering (computeWaves) places a job one step past
 * the max of its own deps' waves -- it says nothing about jobs it does not
 * depend on, so a boundary is only a barrier when the full fan-in holds.
 */
export function isHardBoundary(jobs: Job[], wave: number): boolean {
  const groups = groupByWave(jobs);
  const current = groups[wave];
  const next = groups[wave + 1];
  if (!current || !next || current.length === 0 || next.length === 0) return false;

  const currentIds = current.map((j) => j.id);
  return next.every((job) => currentIds.every((id) => job.needs.includes(id)));
}
