import { describe, expect, it } from 'vitest';
import { ascend, initialNav, selectLead, selectPlan } from './navigation';

describe('navigation', () => {
  it('starts at the leads level', () => {
    expect(initialNav).toEqual({ level: 'leads' });
  });

  it('selecting a lead descends to plans for that lead', () => {
    const next = selectLead('s2');
    expect(next).toEqual({ level: 'plans', leadId: 's2' });
  });

  it('selecting a plan descends to jobs, keeping the lead', () => {
    const plans = selectLead('s2');
    const next = selectPlan(plans, 'w3');
    expect(next).toEqual({ level: 'jobs', leadId: 's2', planId: 'w3' });
  });

  it('ascend from jobs returns to plans with the same lead selected', () => {
    const jobs = selectPlan(selectLead('s1'), 'w1');
    expect(ascend(jobs)).toEqual({ level: 'plans', leadId: 's1' });
  });

  it('ascend from plans returns to leads', () => {
    const plans = selectLead('s1');
    expect(ascend(plans)).toEqual({ level: 'leads' });
  });

  it('ascend from leads is a no-op', () => {
    expect(ascend(initialNav)).toEqual(initialNav);
  });
});
