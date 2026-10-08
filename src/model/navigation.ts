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
 * H14: selecting a thread under THREADS descends straight to Jobs for that conversation's real
 * plan, keeping whatever lead/plan context (if any) the user was already browsing -- selection is
 * additive, not a parallel navigation path (D108: the real conversation tree and the fixture
 * hierarchy stay separate, so `leadId` here is cosmetic breadcrumb/Escape state only, never used
 * to look the conversation up). `planId` is the conversation id stringified; `parseConversationId`
 * is the single place that string is turned back into a number.
 */
export function selectConversation(state: NavState, conversationId: number): NavState {
  return { level: 'jobs', leadId: state.leadId, planId: String(conversationId) };
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

/**
 * H14: `nav.planId` is overloaded -- it is either a fixture workflow id (e.g. `'w3'`) or a real
 * conversation id stringified. The two are told apart explicitly, never by provenance: a
 * `planId` is treated as a conversation id only when it parses as a positive integer written
 * with no leading zero (fixture ids never look like that). Anything else is fixture-backed.
 * Returns `undefined` for fixture ids (and for an absent planId) so callers can use it directly
 * as the optional conversation id `usePlan`/`getPlan` already accept.
 */
export function parseConversationId(planId: string | undefined): number | undefined {
  if (planId === undefined) return undefined;
  if (!/^[1-9]\d*$/.test(planId)) return undefined;
  return Number(planId);
}
