// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ContextNode, ContextTreeModel, SessionNode } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { ContextTree } from "./ContextTree.tsx";

function session(id: string, key: string, state: SessionNode["ref"]["state"], planId: string | null): SessionNode {
  return {
    ref: { id, key, runner: "pi", role: planId === null ? "manager" : "lead", state, title: null },
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
      counts: { draft: 0, queued: 2, running: 1, merging: 1, merged: 13, failed: 1, cancelled: 0 },
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

beforeEach(() => {
  const m = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k: string) => m.get(k) ?? null,
    key: (i: number) => Array.from(m.keys())[i] ?? null,
    removeItem: (k: string) => void m.delete(k),
    setItem: (k: string, v: string) => void m.set(k, String(v)),
  });
});
afterEach(() => vi.unstubAllGlobals());

function props(over: Partial<Parameters<typeof ContextTree>[0]> = {}) {
  return { tree, selectedSessionId: null, selectedPlanId: null, onSelectSession: vi.fn(), onSelectPlan: vi.fn(), ...over };
}

// Title kept from the Job; DESIGN.md 3d: no Active/Standing sections or cost (no source field). Pins the glyphs and counters.
it("context tree renders sections glyphs and counters", () => {
  const onNewContext = vi.fn();
  const m = mount(<ContextTree {...props({ onNewContext })} />);
  const c = m.container;
  expect(c.querySelector('[data-part="tree-header"]')?.textContent).toContain("Contexts");
  const add = c.querySelector('button[data-action="new-context"]');
  expect(add?.getAttribute("aria-label")).toBe("New context");
  act(() => add?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(onNewContext).toHaveBeenCalledTimes(1);
  expect(c.textContent).not.toMatch(/\bActive\b/);
  const ctxRow = c.querySelector('[data-context="c1"] > button[data-select="context"]');
  expect(ctxRow?.querySelector('[data-part="context-id"]')?.textContent).toBe("c1");
  const ctxDot = ctxRow?.querySelector('[role="img"][data-dot]');
  expect(ctxDot?.getAttribute("data-dot")).toBe("waiting");
  expect(ctxDot?.getAttribute("data-glyph")).toBe("idle");
  expect(ctxRow?.querySelector('[data-part="aggregate"]')).toBeNull();
  const planRow = c.querySelector('[data-plan="p1"] button[data-select="plan"]');
  const planDot = planRow?.querySelector('[role="img"][data-dot]');
  // pro-d45: p1 has failed jobs and a working Lead: the red fixing loader, a lucide icon with no glyph-* class
  expect(planDot?.getAttribute("data-dot")).toBe("fixing");
  expect(planDot?.getAttribute("data-tone")).toBe("blocked");
  expect(planDot?.tagName.toLowerCase()).toBe("svg");
  expect(planDot?.children.length).toBe(1);
  expect(planDot?.children[0]?.tagName.toLowerCase()).toBe("path");
  expect(planDot?.classList.contains("animate-spin")).toBe(true);
  expect(planDot?.classList.contains("text-status-blocked-bright")).toBe(true);
  expect(planDot?.classList.contains("shrink-0")).toBe(true);
  expect(planDot?.classList.contains("!size-3.5")).toBe(true);
  expect(Array.from(planDot?.classList ?? []).some((c) => c.startsWith("glyph"))).toBe(false);
  const stopped = c.querySelector('[data-plan="p2"] button[data-select="plan"] [role="img"][data-dot]');
  expect(stopped?.getAttribute("data-dot")).toBe("stopped");
  const counts = planRow?.querySelector('[data-part="counters"]');
  expect(counts?.textContent).toBe("2 1 1 1 13");
  expect(Array.from(counts?.children ?? []).map((e) => e.className)).toEqual([
    expect.stringContaining("text-muted-foreground"),
    expect.stringContaining("text-status-active"),
    expect.stringContaining("text-status-landing"),
    expect.stringContaining("text-status-blocked"),
    expect.stringContaining("text-status-done"),
  ]);
  expect(ctxRow?.querySelector('[data-part="counters"]')).toBeNull();
  expect(c.querySelector('[data-plan="p2"] [data-part="counters"]')?.textContent ?? "").toBe("");
  m.unmount();
});

// pro-d44: the Archive section is its own tree section, closed by default, folded by its header.
it("context tree archive section collapses", () => {
  const m = mount(<ContextTree {...props()} />);
  const group = m.container.querySelector("[data-archived]");
  expect(group?.textContent).toContain("Archive");
  expect(group?.closest('[data-part="tree-section"][data-section="archive"]')).not.toBeNull();
  expect(m.container.querySelector("details, summary")).toBeNull();
  expect(m.container.querySelector('[data-context="c2"]')).toBeNull();
  const toggle = group?.querySelector('[data-slot="sidebar-group-label"] button') ?? null;
  expect(toggle?.getAttribute("aria-expanded")).toBe("false");
  act(() => toggle?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(m.container.querySelector('[data-context="c2"]')).not.toBeNull();
  act(() => toggle?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(m.container.querySelector('[data-context="c2"]')).toBeNull();
  m.unmount();
});

// Title kept from the Job; DESIGN.md 3d: the selected hook is aria-current="true".
it("context tree marks the selected plan", () => {
  const m = mount(<ContextTree {...props({ selectedPlanId: "p1", selectedSessionId: "s-mgr" })} />);
  const c = m.container;
  const item = c.querySelector('li[role="treeitem"][data-plan="p1"]');
  expect(item?.querySelector('button[data-select="plan"]')?.hasAttribute("aria-current")).toBe(false);
  expect(item?.querySelector('button[data-select="plan"] [role="img"][data-dot]')).not.toBeNull();
  expect(item?.closest('[data-slot="sidebar-menu-sub"]')).not.toBeNull();
  const other = c.querySelector('li[data-plan="p2"] button[data-select="plan"]');
  expect(other?.hasAttribute("aria-current")).toBe(false);
  expect(c.querySelector('[data-context="c1"] > button[data-select="context"]')?.getAttribute("aria-current")).toBe("true");
  expect(c.querySelector('[data-session="s-lead"]')).toBeNull();
  expect(c.querySelectorAll('[role="img"][data-dot]')).toHaveLength(3);
  expect(c.querySelector('[data-dot="fixing"]')?.getAttribute("data-tone")).toBe("blocked");
  m.unmount();
});
