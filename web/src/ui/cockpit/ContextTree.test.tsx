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

const counts = { draft: 1, queued: 2, running: 1, merging: 3, merged: 13, failed: 1, cancelled: 4 };
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

it("a Plan row shows its id and its title as tooltip", () => {
  const m = mount(<ContextTree {...props()} />);
  const plan = q(m.container, '[data-plan="p1"]');
  expect(plan?.querySelector('[data-part="plan-id"]')?.textContent).toBe("p1");
  expect(plan?.querySelector('[data-part="plan-id"]')?.getAttribute("title")).toBe("Plan One");
  m.unmount();
});

it("a Context row shows no job counters", () => {
  const m = mount(<ContextTree {...props()} />);
  const row = q(m.container, '[data-context="c1"] button[data-select="context"]');
  expect(row).not.toBeNull();
  expect(row?.querySelector('[data-part="counters"]')).toBeNull();
  expect(row?.closest("[data-context]")?.querySelectorAll('[data-part="counters"]')).toHaveLength(1);
  m.unmount();
});

it("a Plan row shows non-zero counts in order draft queued running merging failed merged separated by one space", () => {
  const m = mount(<ContextTree {...props()} />);
  const counters = q(m.container, '[data-plan="p1"] [data-part="counters"]');
  expect(counters?.textContent).toBe("1 2 1 3 1 13");
  m.unmount();
});

it("a Plan row omits cancelled and zero counts", () => {
  const p1 = tree.contexts[0]!.plans[0]!;
  const sparse = { ...p1, counts: { draft: 0, queued: 2, running: 0, merging: 0, merged: 5, failed: 0, cancelled: 9 } };
  const t: ContextTreeModel = { cursor: 1, contexts: [{ ...tree.contexts[0]!, plans: [sparse] }] };
  const m = mount(<ContextTree {...props({ tree: t })} />);
  const counters = q(m.container, '[data-plan="p1"] [data-part="counters"]');
  expect(counters?.textContent).toBe("2 5");
  expect(counters?.textContent).not.toContain("9");
  m.unmount();
});

it("plan counter colours follow the status tokens", () => {
  const m = mount(<ContextTree {...props()} />);
  const counters = q(m.container, '[data-plan="p1"] [data-part="counters"]');
  const spans = Array.from(counters?.children ?? []);
  expect(spans.map((e) => e.textContent)).toEqual(["1", "2", "1", "3", "1", "13"]);
  const tokens = ["text-status-decision", "text-muted-foreground", "text-status-active", "text-status-landing", "text-status-blocked", "text-status-done"];
  spans.forEach((e, i) => expect(e.className).toContain(tokens[i]!));
  m.unmount();
});

it("the Manager row has a chevron that expands and collapses its Plans and Forks", () => {
  const m = mount(<ContextTree {...props()} />);
  const item = q(m.container, '[data-context="c1"]');
  const chevron = item?.querySelector<HTMLElement>('[data-slot="sidebar-menu-action"]') ?? null;
  expect(chevron).not.toBeNull();
  const visible = () => q(m.container, '[data-plan="p1"]') !== null && q(m.container, 'button[data-session="s-cfork"]') !== null;
  const before = chevron?.getAttribute("aria-expanded");
  expect(["true", "false"]).toContain(before);
  expect(visible()).toBe(before === "true");
  click(chevron);
  expect(q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-action"]')?.getAttribute("aria-expanded")).toBe(before === "true" ? "false" : "true");
  expect(visible()).toBe(before !== "true");
  click(q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-action"]'));
  expect(visible()).toBe(before === "true");
  m.unmount();
});

it("Plans and forks render in a sub list with a 1rem indent and no left border", () => {
  const m = mount(<ContextTree {...props()} />);
  const chevron = q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-action"]');
  if (chevron?.getAttribute("aria-expanded") === "false") click(chevron);
  const sub = q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-sub"]');
  expect(sub).not.toBeNull();
  expect(sub?.querySelector('[data-plan="p1"]')).not.toBeNull();
  expect(sub?.querySelector('button[data-session="s-cfork"]')).not.toBeNull();
  expect(sub?.className).toContain("pl-4");
  expect(sub?.className).not.toContain("border-l");
  m.unmount();
});

it("clicking a Manager Plan or Fork row selects its session", () => {
  const p = props();
  const m = mount(<ContextTree {...p} />);
  const chevron = q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-action"]');
  if (chevron?.getAttribute("aria-expanded") === "false") click(chevron);
  expect(q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-sub"] [data-plan="p1"]')).not.toBeNull();
  click(q(m.container, '[data-context="c1"] button[data-select="context"]'));
  expect(p.onSelectSession).toHaveBeenLastCalledWith("s-mgr");
  click(q(m.container, '[data-plan="p1"] button[data-select="plan"]'));
  expect(p.onSelectSession).toHaveBeenLastCalledWith("s-lead");
  expect(p.onSelectPlan).toHaveBeenCalledWith("p1");
  click(q(m.container, 'button[data-session="s-cfork"]'));
  expect(p.onSelectSession).toHaveBeenLastCalledWith("s-cfork");
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
  const group = m.container.querySelector("[data-archived]");
  expect(group).not.toBeNull();
  expect(group?.tagName).not.toBe("DETAILS");
  expect(m.container.querySelector("nav, details, summary")).toBeNull();
  expect(group?.querySelector('[data-context="c2"]')).toBeNull();
  expect(group?.textContent).toContain("Archived (2)");
  m.unmount();
});
