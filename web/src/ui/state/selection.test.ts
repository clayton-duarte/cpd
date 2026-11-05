import { describe, expect, it } from 'vitest';
import type { ContextTreeModel, PlanViewModel } from '../../protocol/cockpit.ts';
import { activeLeadPlanId, loadSelection, visiblePlan, saveSelection, type SelectionStorage } from './selection';

function memory(initial: Record<string, string> = {}): SelectionStorage & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

describe('selection', () => {
  it('saveSelection writes JSON under cpd.selection', () => {
    const s = memory();
    saveSelection(s, { planId: 'p1', sessionKey: 'k1' });
    expect(JSON.parse(s.data.get('cpd.selection') ?? 'null')).toEqual({ planId: 'p1', sessionKey: 'k1' });
  });
  it('loadSelection returns the saved selection', () => {
    const s = memory({ 'cpd.selection': JSON.stringify({ planId: 'p1', sessionKey: 'k1' }) });
    expect(loadSelection(s)).toEqual({ planId: 'p1', sessionKey: 'k1' });
  });
  it('loadSelection returns null when nothing is saved', () => {
    expect(loadSelection(memory())).toBeNull();
  });
  it('loadSelection returns null on malformed JSON', () => {
    expect(loadSelection(memory({ 'cpd.selection': '{nope' }))).toBeNull();
  });
  it('loadSelection returns null on a wrong shape', () => {
    expect(loadSelection(memory({ 'cpd.selection': JSON.stringify({ planId: 1, sessionKey: null }) }))).toBeNull();
  });
});

const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const sessionNode = (id: string, role: 'lead' | 'manager', planId: string | null) =>
  ({ ref: { id, key: `ctx:${planId ?? 'mgr'}`, runner: 'durable', role, state: 'idle', title: id }, contextId: 'ctx', planId, jobKey: null, archived: false, dot: { state: 'idle', attention: null } }) as const;
const lead = sessionNode('s-lead', 'lead', 'plan-a');
const manager = sessionNode('s-mgr', 'manager', null);
const tree: ContextTreeModel = {
  cursor: 1,
  contexts: [{ id: 'ctx', title: 'Ctx', archived: false, records: 0, openQuestions: 0, sessions: [manager],
    plans: [{ id: 'plan-a', contextId: 'ctx', title: 'A', phase: 'plan', archived: false, counts, leadSessionId: 's-lead', sessions: [lead] }] }],
};
const planModel: PlanViewModel = {
  planId: 'plan-a', contextId: 'ctx', title: 'A', phase: 'plan', counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1,
};

it('loadSelection returns a null planId when nothing is stored', () => {
  expect(loadSelection(memory({ 'cpd.selection': JSON.stringify({ planId: null, sessionKey: 'k1' }) }))).toEqual({ planId: null, sessionKey: 'k1' });
});
it('activeLeadPlanId returns the plan of the selected Lead session', () => {
  expect(activeLeadPlanId(tree, 's-lead')).toBe('plan-a');
});
it('activeLeadPlanId returns null when a Manager session is selected', () => {
  expect(activeLeadPlanId(tree, 's-mgr')).toBeNull();
});
it('activeLeadPlanId returns null when no session is selected', () => {
  expect(activeLeadPlanId(tree, null)).toBeNull();
});
it('visiblePlan is null when no Lead is selected', () => {
  expect(visiblePlan(null, planModel)).toBeNull();
});
