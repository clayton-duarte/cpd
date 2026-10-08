/**
 * Pure navigation state machine for the three canvas levels (C4/D79).
 * No React, no Mantine, no DOM -- same discipline as derive.ts.
 */
export type Level = 'leads' | 'plans' | 'jobs';

export interface NavState {
  level: Level;
  leadId?: string;
  planId?: string;
}

export const initialNav: NavState = { level: 'leads' };

/** Selecting a lead descends to Plans for that lead. */
export function selectLead(leadId: string): NavState {
  return { level: 'plans', leadId };
}

/** Selecting a plan descends to Jobs for that plan, keeping the current lead. */
export function selectPlan(state: NavState, planId: string): NavState {
  return { level: 'jobs', leadId: state.leadId, planId };
}

/**
 * Ascend one level: Jobs -> Plans (same lead still selected), Plans -> Leads.
 * At Leads, ascending is a no-op (nothing above it).
 */
export function ascend(state: NavState): NavState {
  if (state.level === 'jobs') return { level: 'plans', leadId: state.leadId };
  if (state.level === 'plans') return { level: 'leads' };
  return state;
}
