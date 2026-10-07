import { describe, it, expect } from 'vitest';
import { computeWaves, groupByWave, isHardBoundary } from './waves';
import { sampleData } from '../fixtures/sample';
import type { Job } from '../model/types';

describe('computeWaves', () => {
  it('assigns wave 0 to jobs with no dependencies (j1/j2/j10)', () => {
    const waves = computeWaves(sampleData.jobs);
    expect(waves.get('j1')).toBe(0);
    expect(waves.get('j2')).toBe(0);
    expect(waves.get('j10')).toBe(0);
  });

  it('assigns wave 1 to jobs depending only on wave-0 jobs (j3/j4)', () => {
    const waves = computeWaves(sampleData.jobs);
    expect(waves.get('j3')).toBe(1);
    expect(waves.get('j4')).toBe(1);
  });

  it('assigns a job one past the MAX wave of its dependencies, not the first found', () => {
    const waves = computeWaves(sampleData.jobs);
    // j11 needs j3 (wave1), j4 (wave1), j6 (wave2: j6 needs j2(0) -> wave1... )
    // j6 needs j2 (wave0) => j6 wave1. j11 needs j3(1), j4(1), j6(1) => wave2
    // Actual deepest chain: j7 needs j5(wave1 via j2) -> j7 wave2; j8 needs j3(1)-> wave2
    // j11 needs j3,j4,j6 all wave1 => wave2
    expect(waves.get('j11')).toBe(Math.max(waves.get('j3')!, waves.get('j4')!, waves.get('j6')!) + 1);
  });

  it('handles a diamond dependency correctly', () => {
    const jobs: Job[] = [
      { id: 'a', workflowId: 'w', title: 'a', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: [], artifactCount: 0, steeringPending: false },
      { id: 'b', workflowId: 'w', title: 'b', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['a'], artifactCount: 0, steeringPending: false },
      { id: 'c', workflowId: 'w', title: 'c', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['a'], artifactCount: 0, steeringPending: false },
      { id: 'd', workflowId: 'w', title: 'd', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['b', 'c'], artifactCount: 0, steeringPending: false },
    ];
    const waves = computeWaves(jobs);
    expect(waves.get('a')).toBe(0);
    expect(waves.get('b')).toBe(1);
    expect(waves.get('c')).toBe(1);
    expect(waves.get('d')).toBe(2);
  });

  it('throws a clear error naming the involved ids on a cycle', () => {
    const jobs: Job[] = [
      { id: 'x', workflowId: 'w', title: 'x', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['y'], artifactCount: 0, steeringPending: false },
      { id: 'y', workflowId: 'w', title: 'y', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['x'], artifactCount: 0, steeringPending: false },
    ];
    expect(() => computeWaves(jobs)).toThrow(/x/);
    expect(() => computeWaves(jobs)).toThrow(/y/);
  });
});

describe('groupByWave', () => {
  it('groups jobs by wave, preserving stable fixture order within each wave', () => {
    const groups = groupByWave(sampleData.jobs);
    expect(groups[0].map((j) => j.id)).toEqual(['j1', 'j2', 'j10']);
    expect(groups[1].map((j) => j.id)).toEqual(['j3', 'j4', 'j5', 'j6']);
  });
});

describe('isHardBoundary', () => {
  it('is false for the fixture (wave 1 does not need all of wave 0)', () => {
    expect(isHardBoundary(sampleData.jobs, 0)).toBe(false);
  });

  it('is true for a fan-in where every job in wave 1 needs every job in wave 0', () => {
    const jobs: Job[] = [
      { id: 'a', workflowId: 'w', title: 'a', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: [], artifactCount: 0, steeringPending: false },
      { id: 'b', workflowId: 'w', title: 'b', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: [], artifactCount: 0, steeringPending: false },
      { id: 'c', workflowId: 'w', title: 'c', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['a', 'b'], artifactCount: 0, steeringPending: false },
      { id: 'd', workflowId: 'w', title: 'd', status: 'done', owner: 'neutral', profile: 'builder', attempt: 1, needs: ['a', 'b'], artifactCount: 0, steeringPending: false },
    ];
    expect(isHardBoundary(jobs, 0)).toBe(true);
  });
});
