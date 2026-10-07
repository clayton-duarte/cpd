import { describe, expect, it } from 'vitest';
import { sampleData } from '../fixtures/sample';
import { edgeStyle } from './edges';

const findJob = (id: string) => sampleData.jobs.find((j) => j.id === id)!;

describe('edgeStyle', () => {
  it('is dashed when the target is a draft (j11 -> j13)', () => {
    expect(edgeStyle(findJob('j11'), findJob('j13'))).toBe('dashed');
  });

  it('is solid when neither endpoint is a draft (j1 -> j3)', () => {
    expect(edgeStyle(findJob('j1'), findJob('j3'))).toBe('solid');
  });
});
