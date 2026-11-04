// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { ContextTreeModel, SessionNode } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { ContextTree } from "./ContextTree.tsx";

function session(id: string, key: string, state: SessionNode["ref"]["state"], title: string | null): SessionNode {
  return {
    ref: { id, key, runner: "pi", role: "manager", state, title },
    contextId: "c1",
    planId: null,
    jobKey: null,
    archived: false,
    dot: { state, attention: null },
  };
}

const counts = { draft: 0, queued: 0, running: 1, merged: 13, failed: 0, cancelled: 0 };
const tree: ContextTreeModel = {
  cursor: 1,
  contexts: [
    {
      id: "c1",
      title: "Context One",
      archived: false,
      records: 0,
      openQuestions: 0,
      sessions: [session("s-mgr", "c1", "waiting", "Manager chat")],
      plans: [
        {
          id: "p1",
          contextId: "c1",
          title: "Plan One",
          phase: "dispatch",
          archived: false,
          counts,
          leadSessionId: "s-lead",
          sessions: [{ ...session("s-lead", "c1:p1", "working", null), planId: "p1" }],
        },
      ],
    },
  ],
};

function props(over: Partial<Parameters<typeof ContextTree>[0]> = {}) {
  return {
    tree,
    selectedSessionId: null,
    selectedPlanId: null,
    onSelectSession: vi.fn(),
    onSelectPlan: vi.fn(),
    ...over,
  };
}

function click(el: Element | null): void {
  if (el === null) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

it("renders contexts, plans and sessions as a tree", () => {
  const m = mount(<ContextTree {...props()} />);
  expect(m.container.querySelector('nav[aria-label="Context tree"]')).not.toBeNull();
  const ctx = m.container.querySelector('[role="treeitem"][data-context="c1"]');
  expect(ctx?.textContent).toContain("Context One");
  const plan = ctx?.querySelector('[role="treeitem"][data-plan="p1"]');
  expect(plan?.querySelector('button[data-select="plan"]')?.textContent).toContain("Plan One");
  expect(plan?.textContent).toContain("merged 13 · running 1");
  expect(ctx?.querySelector('button[data-session="s-mgr"]')?.textContent).toContain("c1 — Manager chat");
  expect(plan?.querySelector('button[data-session="s-lead"]')?.textContent).toContain("c1:p1");
  m.unmount();
});

it("shows a state dot per session", () => {
  const m = mount(<ContextTree {...props()} />);
  const dots = m.container.querySelectorAll('[role="img"][data-dot]');
  expect(dots).toHaveLength(2);
  const mgr = m.container.querySelector('button[data-session="s-mgr"]')?.parentElement?.querySelector("[data-dot]");
  expect(mgr?.getAttribute("data-dot")).toBe("waiting");
  expect(mgr?.getAttribute("data-attention")).toBe("none");
  expect(mgr?.getAttribute("aria-label")).toBe("waiting");
  m.unmount();
});

it("selecting a session calls onSelectSession", () => {
  const p = props();
  const m = mount(<ContextTree {...p} />);
  click(m.container.querySelector('button[data-session="s-lead"]'));
  expect(p.onSelectSession).toHaveBeenCalledExactlyOnceWith("s-lead");
  expect(p.onSelectPlan).not.toHaveBeenCalled();
  m.unmount();
});

it("selecting a plan calls onSelectPlan", () => {
  const p = props();
  const m = mount(<ContextTree {...p} />);
  click(m.container.querySelector('button[data-select="plan"]'));
  expect(p.onSelectPlan).toHaveBeenCalledExactlyOnceWith("p1");
  expect(p.onSelectSession).not.toHaveBeenCalled();
  m.unmount();
});
