// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { ContextTreeModel, SessionNode } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { ContextTree } from "./ContextTree.tsx";

function session(
  id: string,
  key: string,
  state: SessionNode["ref"]["state"],
  title: string | null,
  over: Partial<SessionNode> & { role?: SessionNode["ref"]["role"] } = {},
): SessionNode {
  const { role = "manager", ...rest } = over;
  return {
    ref: { id, key, runner: "pi", role, state, title },
    contextId: "c1",
    planId: null,
    jobKey: null,
    archived: false,
    dot: { state, attention: null },
    ...rest,
  };
}

const counts = { draft: 1, queued: 2, running: 1, merged: 13, failed: 1, cancelled: 4 };
const tree: ContextTreeModel = {
  cursor: 1,
  contexts: [
    {
      id: "c1",
      title: "Context One",
      archived: false,
      records: 0,
      openQuestions: 0,
      sessions: [session("s-mgr", "c1", "waiting", "Manager chat"), session("s-cfork", "c1-fork", "idle", null, { role: "lead" })],
      plans: [
        {
          id: "p1",
          contextId: "c1",
          title: "Plan One",
          phase: "dispatch",
          archived: false,
          counts,
          leadSessionId: "s-lead",
          sessions: [
            session("s-lead", "c1:p1", "working", null, { planId: "p1", role: "lead" }),
            session("s-work", "c1:p1:j1", "working", null, { planId: "p1", jobKey: "j1", role: "worker" }),
            session("abcd1234", "c1:p1:f", "idle", null, { planId: "p1", role: "lead" }),
            session("s-old", "c1:p1:old", "idle", null, { planId: "p1", role: "lead", archived: true }),
          ],
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

const q = (c: ParentNode, sel: string) => c.querySelector(sel);

it("the Context row selects the Manager session", () => {
  const p = props();
  const m = mount(<ContextTree {...p} />);
  const btn = q(m.container, '[data-context="c1"] button[data-select="context"]');
  expect(btn?.textContent).toContain("c1");
  expect(btn?.querySelector('[role="img"][data-dot]')?.getAttribute("data-dot")).toBe("waiting");
  click(btn);
  expect(p.onSelectSession).toHaveBeenCalledExactlyOnceWith("s-mgr");
  expect(p.onSelectPlan).not.toHaveBeenCalled();
  m.unmount();
  const sel = mount(<ContextTree {...props({ selectedSessionId: "s-mgr" })} />);
  expect(q(sel.container, 'button[data-select="context"]')?.getAttribute("aria-current")).toBe("true");
  sel.unmount();
});

it("the Manager session is not a separate row", () => {
  const m = mount(<ContextTree {...props()} />);
  expect(q(m.container, 'button[data-session="s-mgr"]')).toBeNull();
  // one dot per row: Context, Plan, Context fork, Plan fork
  expect(m.container.querySelectorAll('[role="img"][data-dot]')).toHaveLength(4);
  m.unmount();
});

it("a Plan is one row that selects the Plan and its Lead session", () => {
  const p = props();
  const m = mount(<ContextTree {...p} />);
  const btn = q(m.container, '[data-plan="p1"] button[data-select="plan"]');
  expect(btn?.querySelector('[role="img"][data-dot]')?.getAttribute("data-dot")).toBe("working");
  expect(q(m.container, 'button[data-session="s-lead"]')).toBeNull();
  click(btn);
  expect(p.onSelectPlan).toHaveBeenCalledExactlyOnceWith("p1");
  expect(p.onSelectSession).toHaveBeenCalledExactlyOnceWith("s-lead");
  m.unmount();
});

it("a Plan row shows its id and the planned active failed done counters", () => {
  const m = mount(<ContextTree {...props()} />);
  const plan = q(m.container, '[data-plan="p1"]');
  expect(plan?.querySelector('[data-part="plan-id"]')?.textContent).toBe("p1");
  expect(plan?.querySelector('[data-part="plan-id"]')?.getAttribute("title")).toBe("Plan One");
  const counters = plan?.querySelector('[data-part="counters"]');
  expect(counters?.textContent).toBe("3 · 1 · 1 · 13");
  expect(counters?.querySelector('[data-count="planned"]')?.textContent).toBe("3");
  expect(counters?.querySelector('[data-count="active"]')?.textContent).toBe("1");
  expect(counters?.querySelector('[data-count="failed"]')?.textContent).toBe("1");
  expect(counters?.querySelector('[data-count="done"]')?.textContent).toBe("13");
  expect(q(m.container, '[data-context="c1"] > button [data-part="counters"]')?.textContent).toBe("3 · 1 · 1 · 13");
  m.unmount();
});

it("only forks of a Plan get their own rows and Workers get none", () => {
  const p = props();
  const m = mount(<ContextTree {...p} />);
  expect(q(m.container, 'button[data-session="s-work"]')).toBeNull();
  const fork = q(m.container, 'button[data-session="abcd1234"]');
  expect(fork?.textContent).toBe("p1·abcd");
  const ctxFork = q(m.container, 'button[data-session="s-cfork"]');
  expect(ctxFork?.textContent).toBe("c1-fork");
  expect(m.container.querySelectorAll("button[data-session]")).toHaveLength(2);
  click(fork);
  expect(p.onSelectSession).toHaveBeenCalledExactlyOnceWith("abcd1234");
  m.unmount();
});

it("archived sessions are not listed", () => {
  const m = mount(<ContextTree {...props()} />);
  expect(q(m.container, 'button[data-session="s-old"]')).toBeNull();
  expect(m.container.textContent).not.toContain("c1:p1:old");
  m.unmount();
});

it("a Context row shows the id and its title as tooltip", () => {
  const m = mount(<ContextTree {...props()} />);
  const id = q(m.container, '[data-context="c1"] [data-part="context-id"]');
  expect(id?.textContent).toBe("c1");
  expect(id?.getAttribute("title")).toBe("Context One");
  m.unmount();
});

it("the Archived group is closed by default and counts its Contexts and Plans", () => {
  const archivedPlan = { ...tree.contexts[0]!.plans[0]!, id: "p9", archived: true, sessions: [] };
  const archivedCtx = { ...tree.contexts[0]!, id: "c2", archived: true, sessions: [], plans: [archivedPlan] };
  const t: ContextTreeModel = { cursor: 1, contexts: [...tree.contexts, archivedCtx] };
  const m = mount(<ContextTree {...props({ tree: t })} />);
  const details = m.container.querySelector<HTMLDetailsElement>("nav > details[data-archived]");
  expect(details?.hasAttribute("open")).toBe(false);
  expect(details?.querySelector("summary")?.textContent).toBe("Archived (2)");
  m.unmount();
});
