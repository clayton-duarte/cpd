import { describe, expect, it } from 'vitest';
import { dataForPlan, leadsLevel, plansLevel } from './levels';
import { sampleData } from '../fixtures/sample';

describe('leadsLevel', () => {
  it('produces one entry per session with name, tier, and plan count', () => {
    const leads = leadsLevel(sampleData);
    expect(leads).toHaveLength(sampleData.sessions.length);
    const s2 = leads.find((l) => l.id === 's2')!;
    expect(s2.name).toBe('Grove automation');
    expect(s2.leadTier).toBe('S2');
    expect(s2.planCount).toBe(3);
  });
});

describe('plansLevel', () => {
  it('returns exactly the workflows belonging to the given lead', () => {
    const plans = plansLevel(sampleData, 's2');
    expect(plans.map((p) => p.id).sort()).toEqual(['w3', 'w4', 'w5']);
  });

  it('carries job count and D77 identity fields only when present', () => {
    const plans = plansLevel(sampleData, 's1');
    const w1 = plans.find((p) => p.id === 'w1')!;
    const w2 = plans.find((p) => p.id === 'w2')!;
    expect(w1.jobCount).toBe(14);
    expect(w1.ticket?.key).toBe('ORCH-42');
    expect(w2.ticket).toBeUndefined();
    expect(w2.branch).toBeUndefined();
    expect(w2.pr).toBeUndefined();
  });

  it('a lead with zero plans returns an empty list, not a crash', () => {
    const noPlanData = {
      ...sampleData,
      sessions: [{ id: 'sx', projectId: 'p1', name: 'Empty lead', leadTier: 'L0' as const, workflowIds: [] }],
    };
    expect(plansLevel(noPlanData, 'sx')).toEqual([]);
  });

  it('an unknown lead id returns an empty list', () => {
    expect(plansLevel(sampleData, 'does-not-exist')).toEqual([]);
  });
});

describe('dataForPlan', () => {
  it('scopes jobs to exactly the given plan', () => {
    const scoped = dataForPlan(sampleData, 'w3');
    expect(scoped.jobs.map((j) => j.id).sort()).toEqual(['j17', 'j18', 'j19', 'j20', 'j21']);
    expect(scoped.workflows.map((w) => w.id)).toEqual(['w3']);
  });
});
