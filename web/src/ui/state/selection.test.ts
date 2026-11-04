import { describe, expect, it } from 'vitest';
import { loadSelection, saveSelection, type SelectionStorage } from './selection';

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
