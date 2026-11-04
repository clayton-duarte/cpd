// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { ContextNode, ContextTreeModel, SessionNode } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { ContextTree } from "./ContextTree.tsx";

function session(id: string, key: string, state: SessionNode["ref"]["state"], planId: string | null): SessionNode {
  return {
    ref: { id, key, runner: "pi", role: "manager", state, title: null },
    contextId: "c1",
    planId,
    jobKey: null,
    archived: false,
    dot: { state, attention: null },
  };
}

const live: ContextNode = {
  id: "c1",
  title: "Context One",
  archived: false,
  records: 0,
  openQuestions: 0,
  sessions: [session("s-mgr", "c1", "waiting", null)],
  plans: [
    {
      id: "p1",
      contextId: "c1",
      title: "Plan One",
      phase: "dispatch",
      archived: false,
      counts: { draft: 0, queued: 2, running: 1, merged: 13, failed: 1, cancelled: 0 },
      leadSessionId: "s-lead",
      sessions: [session("s-lead", "c1:p1", "working", "p1")],
    },
    {
      id: "p2",
      contextId: "c1",
      title: "Plan Two",
      phase: "done",
      archived: false,
      counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
      leadSessionId: null,
      sessions: [],
    },
  ],
};
const archivedCtx: ContextNode = { ...live, id: "c2", title: "Old Context", archived: true, sessions: [], plans: [] };
const tree: ContextTreeModel = { cursor: 1, contexts: [live, archivedCtx] };

function props(over: Partial<Parameters<typeof ContextTree>[0]> = {}) {
  return { tree, selectedSessionId: null, selectedPlanId: null, onSelectSession: vi.fn(), onSelectPlan: vi.fn(), ...over };
}

// Title kept from the Job; DESIGN.md 3d: no Active/Standing sections or cost (no source field). Pins the glyphs and counters.
it("context tree renders sections, glyphs and job counters", () => {
  const onNewContext = vi.fn();
  const m = mount(<ContextTree {...props({ onNewContext })} />);
  const c = m.container;
  expect(c.querySelector('[data-part="tree-header"]')?.textContent).toContain("Contexts");
  const add = c.querySelector('button[data-action="new-context"]');
  expect(add?.getAttribute("aria-label")).toBe("New context");
  act(() => add?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(onNewContext).toHaveBeenCalledTimes(1);
  expect(c.textContent).not.toMatch(/\bActive\b|\bStanding\b/);
  const ctxRow = c.querySelector('[data-context="c1"] [data-part="context-row"]');
  expect(ctxRow?.textContent).toContain("Context One");
  const ctxAgg = ctxRow?.querySelector('[data-part="aggregate"]');
  expect(ctxAgg?.getAttribute("data-glyph")).toBe("active");
  expect(ctxAgg?.getAttribute("data-tone")).toBe("active");
  expect(ctxAgg?.getAttribute("aria-hidden")).toBe("true");
  expect(ctxAgg?.className).toContain("glyph-active");
  const planRow = c.querySelector('[data-plan="p1"] [data-part="plan-row"]');
  expect(planRow?.querySelector('[data-part="aggregate"]')?.getAttribute("data-glyph")).toBe("active");
  expect(planRow?.querySelector('[data-part="aggregate"]')?.getAttribute("data-tone")).toBe("active");
  const idleRow = c.querySelector('[data-plan="p2"] [data-part="plan-row"] [data-part="aggregate"]');
  expect(idleRow?.getAttribute("data-glyph")).toBe("idle");
  expect(idleRow?.getAttribute("data-tone")).toBe("neutral");
  const counts = planRow?.querySelector('[data-part="counts"]');
  expect(counts?.textContent).toBe("queued 2 · merged 13 · running 1 · failed 1");
  expect(counts?.querySelector('[data-count="running"]')?.textContent).toBe("running 1");
  expect(counts?.querySelector('[data-count="merged"]')?.textContent).toBe("merged 13");
  expect(counts?.querySelector('[data-count="failed"]')?.textContent).toBe("failed 1");
  expect(counts?.querySelector('[data-count="queued"]')?.textContent).toBe("queued 2");
  expect(counts?.querySelector('[data-count="draft"]')).toBeNull();
  const dot = c.querySelector('[data-session="s-lead"]')?.parentElement?.querySelector("[data-dot]");
  expect(dot?.getAttribute("data-glyph")).toBe("active");
  expect(dot?.getAttribute("data-tone")).toBe("active");
  expect(dot?.className).toContain("glyph-active");
  expect(c.querySelector('[data-session="s-mgr"]')?.parentElement?.querySelector("[data-dot]")?.getAttribute("data-glyph")).toBe("idle");
  expect(c.querySelector('[data-session="s-lead"] .font-mono')?.textContent).toBe("c1:p1");
  m.unmount();
});

// Title kept from the Job; DESIGN.md 3d: Archived (n) is a native details, its summary toggles it.
it("context tree archive section collapses", () => {
  const m = mount(<ContextTree {...props()} />);
  const details = m.container.querySelector<HTMLDetailsElement>("nav > details[data-archived]");
  const summary = details?.querySelector("summary");
  expect(summary?.textContent).toBe("Archived (1)");
  expect(details?.open).toBe(false);
  expect(summary?.parentElement?.querySelector('[data-context="c2"]')).not.toBeNull();
  act(() => summary?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(details?.open).toBe(true);
  act(() => summary?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(details?.open).toBe(false);
  expect(m.container.querySelector("[data-part=\"aggregate\"][data-glyph]")).not.toBeNull();
  m.unmount();
});

// Title kept from the Job; DESIGN.md 3d: the selected hook is aria-current="true".
it("context tree marks the selected plan", () => {
  const m = mount(<ContextTree {...props({ selectedPlanId: "p1", selectedSessionId: "s-mgr" })} />);
  const c = m.container;
  const item = c.querySelector('li[role="treeitem"][data-plan="p1"]');
  expect(item?.querySelector('button[data-select="plan"]')?.getAttribute("aria-current")).toBe("true");
  expect(item?.querySelector('[data-part="plan-row"] [data-part="aggregate"]')).not.toBeNull();
  const other = c.querySelector('li[data-plan="p2"] button[data-select="plan"]');
  expect(other?.hasAttribute("aria-current")).toBe(false);
  expect(c.querySelector('[data-session="s-mgr"]')?.getAttribute("aria-current")).toBe("true");
  expect(c.querySelector('li[role="treeitem"][data-context="c1"]')).not.toBeNull();
  expect(c.querySelector('[data-session="s-lead"]')).not.toBeNull();
  expect(c.querySelectorAll('[role="img"][data-dot]')).toHaveLength(2);
  expect(c.querySelector('[data-dot="working"]')?.getAttribute("data-tone")).toBe("active");
  m.unmount();
});
