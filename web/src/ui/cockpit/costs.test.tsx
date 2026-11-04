// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ContextTreeModel } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { ContextTree } from "./ContextTree.tsx";

const counts = { draft: 0, queued: 0, running: 1, merging: 0, merged: 0, failed: 0, cancelled: 0 };
const tree: ContextTreeModel = {
  cursor: 1,
  contexts: [
    {
      id: "c1", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [],
      plans: [
        { id: "p1", contextId: "c1", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: null, sessions: [] },
        { id: "p2", contextId: "c1", title: "Plan Two", phase: "dispatch", archived: false, counts, leadSessionId: null, sessions: [] },
      ],
    },
  ],
};
const costs = { contexts: { c1: 0.12 }, plans: { p1: 1234 } };

beforeEach(() => {
  const m = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k), clear: () => m.clear(), key: () => null, length: 0,
  });
});
afterEach(() => vi.unstubAllGlobals());

const render = (costsOn: boolean) =>
  mount(<ContextTree tree={tree} selectedSessionId={null} selectedPlanId={null} onSelectSession={vi.fn()} onSelectPlan={vi.fn()} costsOn={costsOn} costs={costs} />);

it("costs on shows the cost after the Context id and the Plan name", () => {
  const m = render(true);
  const ctx = m.container.querySelector('[data-context="c1"] button[data-select="context"] [data-part="row-cost"]');
  expect(ctx?.textContent).toBe("$0.120");
  expect(ctx?.className).toContain("text-fg-dim");
  const p1 = m.container.querySelector('[data-plan="p1"] [data-part="row-cost"]');
  expect(p1?.textContent).toBe("$1.23k");
  expect(m.container.querySelector('[data-plan="p2"] [data-part="row-cost"]')).toBeNull();
  m.unmount();
});

it("costs off shows no cost on tree rows", () => {
  const m = render(false);
  expect(m.container.querySelector('[data-part="row-cost"]')).toBeNull();
  m.unmount();
});
