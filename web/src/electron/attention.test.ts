import type { InboxModel, InboxRow, Urgency } from "../protocol/cockpit.ts";
import type { CockpitService, CockpitUpdate } from "../server/cockpit/events.ts";
import { countAttention, watchAttention, type AttentionCounts } from "./attention.ts";

function row(id: number, urgency: Urgency): InboxRow {
  return { id, kind: "pr-ready", label: `row ${id}`, urgency, contextId: null, planId: null, jobKey: null, sessionId: null, url: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 0, actions: [] };
}

function inbox(...urgencies: Urgency[]): InboxModel {
  return { openCount: urgencies.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: urgencies.map((u, i) => row(i + 1, u)), cursor: 0 };
}

function fakeCockpit(initial: InboxModel | null) {
  let current = initial;
  const listeners = new Set<(u: CockpitUpdate) => void>();
  let disposed = 0;
  const cockpit: Pick<CockpitService, "snapshot" | "subscribe"> = {
    snapshot: () => ({ tree: null, plan: null, inbox: current }),
    subscribe(listener) {
      listeners.add(listener);
      return { dispose() { disposed++; listeners.delete(listener); } };
    },
  };
  return {
    cockpit,
    update(next: InboxModel | null) {
      current = next;
      for (const l of [...listeners]) l({ kind: "pane" } as unknown as CockpitUpdate);
    },
    disposed: () => disposed,
  };
}

test("countAttention returns zeros for a null inbox", () => {
  expect(countAttention(null)).toEqual({ blocking: 0, decisions: 0 });
});

test("countAttention counts red rows as blocking and magenta rows as decisions", () => {
  expect(countAttention(inbox("red", "magenta", "red", "magenta", "magenta"))).toEqual({ blocking: 2, decisions: 3 });
});

test("countAttention ignores rows of other urgencies", () => {
  expect(countAttention(inbox("yellow", "blue", "cyan", "white"))).toEqual({ blocking: 0, decisions: 0 });
});

test("watchAttention emits the current counts immediately", () => {
  const f = fakeCockpit(inbox("red", "magenta"));
  const seen: AttentionCounts[] = [];
  watchAttention(f.cockpit, (c) => seen.push(c));
  expect(seen).toEqual([{ blocking: 1, decisions: 1 }]);
});

test("watchAttention emits again when an inbox update changes the counts", () => {
  const f = fakeCockpit(null);
  const seen: AttentionCounts[] = [];
  watchAttention(f.cockpit, (c) => seen.push(c));
  f.update(inbox("red"));
  expect(seen).toEqual([{ blocking: 0, decisions: 0 }, { blocking: 1, decisions: 0 }]);
});

test("watchAttention does not emit when an update leaves the counts equal", () => {
  const f = fakeCockpit(inbox("red"));
  const seen: AttentionCounts[] = [];
  watchAttention(f.cockpit, (c) => seen.push(c));
  f.update(inbox("red", "yellow"));
  expect(seen).toEqual([{ blocking: 1, decisions: 0 }]);
});

test("watchAttention stops emitting after dispose", () => {
  const f = fakeCockpit(null);
  const seen: AttentionCounts[] = [];
  const d = watchAttention(f.cockpit, (c) => seen.push(c));
  d.dispose();
  d.dispose();
  f.update(inbox("red"));
  expect(seen).toEqual([{ blocking: 0, decisions: 0 }]);
  expect(f.disposed()).toBeGreaterThanOrEqual(1);
});
