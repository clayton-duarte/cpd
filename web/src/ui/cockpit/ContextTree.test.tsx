// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ContextNode, ContextTreeModel, SessionNode } from "../../protocol/cockpit.ts";
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

function fakeStorage(initial: Record<string, string> = {}): Storage {
  const m = new Map(Object.entries(initial));
  return {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k: string) => m.get(k) ?? null,
    key: (i: number) => Array.from(m.keys())[i] ?? null,
    removeItem: (k: string) => void m.delete(k),
    setItem: (k: string, v: string) => void m.set(k, String(v)),
  };
}

let store: Storage;
beforeEach(() => {
  store = fakeStorage();
  vi.stubGlobal("localStorage", store);
});
afterEach(() => vi.unstubAllGlobals());

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

it("the Archived group is closed by default and folds by section archive", () => {
  const archivedPlan = { ...tree.contexts[0]!.plans[0]!, id: "p9", archived: true, sessions: [] };
  const archivedCtx = { ...tree.contexts[0]!, id: "c2", archived: true, sessions: [], plans: [archivedPlan] };
  const t: ContextTreeModel = { cursor: 1, contexts: [...tree.contexts, archivedCtx] };
  const m = mount(<ContextTree {...props({ tree: t })} />);
  const group = m.container.querySelector("[data-archived]");
  expect(group).not.toBeNull();
  expect(group?.tagName).not.toBe("DETAILS");
  expect(m.container.querySelector("nav, details, summary")).toBeNull();
  expect(group?.closest('[data-part="tree-pinned-bottom"]')).not.toBeNull();
  expect(group?.querySelector('[data-context="c2"]')).toBeNull();
  expect(group?.textContent).toContain("Archive");
  click(header(m.container, "Archive"));
  expect(store.getItem("cpd-ui:collapse:section:archive")).toBe("1");
  expect(m.container.querySelector('[data-archived] [data-context="c2"]')).not.toBeNull();
  m.unmount();
});

// pro-d44 three layers.
const mgr = (id: string, dot: SessionNode["dot"]): SessionNode =>
  ({ ...session(`m-${id}`, id, dot.state, null), contextId: id, dot });
function ctx(id: string, createdAt: string, dot: SessionNode["dot"] = { state: "idle", attention: null }, over: Partial<ContextNode> = {}): ContextNode {
  return { id, title: id, archived: false, records: 0, openQuestions: 0, sessions: [mgr(id, dot)], plans: [], createdAt, ...over } as ContextNode;
}
const layered: ContextTreeModel = {
  cursor: 1,
  contexts: [
    ctx("codereview", "2026-01-01"),
    ctx("old", "2026-02-01"),
    ctx("host", "2026-01-02"),
    ctx("new", "2026-03-01"),
    ctx("hot", "2026-01-05", { state: "waiting", attention: "red" }),
    ctx("cpd", "2026-01-03"),
    ctx("gone", "2026-01-06", { state: "idle", attention: null }, { archived: true }),
  ],
};
const ids = (root: ParentNode | null) => Array.from(root?.querySelectorAll("[data-context]") ?? []).map((e) => e.getAttribute("data-context"));
const header = (c: ParentNode, name: string): HTMLElement | null =>
  Array.from(c.querySelectorAll<HTMLElement>('[data-slot="sidebar-group-label"] button')).find((b) => (b.textContent ?? "").includes(name)) ?? null;

it("the tree has three layers: Contexts, a separator and a pinned bottom with Standing and Archive", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const c = m.container;
  const bottom = q(c, '[data-part="tree-pinned-bottom"]');
  expect(bottom, "pinned bottom").not.toBeNull();
  for (const cls of ["mt-auto", "max-h-[50%]", "shrink-0"]) expect(bottom?.className).toContain(cls);
  expect(bottom?.querySelector('[data-slot="scroll-area"]'), "bottom scroll area").not.toBeNull();
  expect(bottom?.querySelector("[data-standing]"), "standing").not.toBeNull();
  expect(bottom?.querySelector("[data-archived]"), "archive").not.toBeNull();
  expect(bottom?.querySelectorAll('[data-slot="separator"]').length, "separator inside bottom").toBeGreaterThanOrEqual(1);
  const top = q(c, '[data-part="tree-header"]')?.closest('[data-slot="scroll-area"]') ?? null;
  expect(top, "Contexts inside a scroll area").not.toBeNull();
  expect(bottom?.contains(top)).toBe(false);
  expect(c.querySelectorAll('[data-slot="separator"]').length, "separators").toBeGreaterThanOrEqual(2);
  m.unmount();
});

it("the Contexts section lists live Contexts by dot tier then newest first", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const top = q(m.container, '[data-part="tree-header"]')?.closest('[data-slot="scroll-area"]') ?? null;
  expect(ids(top)).toEqual(["hot", "new", "old"]);
  m.unmount();
});

it("the Standing section lists cpd host codereview in that order", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  expect(ids(q(m.container, "[data-standing]"))).toEqual(["cpd", "host", "codereview"]);
  m.unmount();
});

it("each section header shows a minus icon when open and a plus icon when closed", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const icon = (name: string) => header(m.container, name)?.querySelector("svg")?.getAttribute("class") ?? "";
  for (const name of ["Contexts", "Standing"]) {
    expect(icon(name), `${name} open`).toContain("lucide-minus");
    click(header(m.container, name));
    expect(icon(name), `${name} closed`).toContain("lucide-plus");
  }
  expect(icon("Archive"), "Archive closed by default").toContain("lucide-plus");
  click(header(m.container, "Archive"));
  expect(icon("Archive"), "Archive open").toContain("lucide-minus");
  m.unmount();
});

it("the Archive section is closed by default and opens from its header", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const archive = () => q(m.container, "[data-archived]");
  expect(archive()).not.toBeNull();
  expect(archive()?.querySelector('[data-context="gone"]')).toBeNull();
  expect(header(m.container, "Archive")?.getAttribute("aria-expanded")).toBe("false");
  expect(archive()?.closest('[data-part="tree-pinned-bottom"]')).not.toBeNull();
  click(header(m.container, "Archive"));
  expect(archive()?.querySelector('[data-context="gone"]')).not.toBeNull();
  click(header(m.container, "Archive"));
  expect(archive()?.querySelector('[data-context="gone"]')).toBeNull();
  m.unmount();
});

it("a Plan row is current only when its Lead session is selected", () => {
  const m = mount(<ContextTree {...props({ selectedSessionId: "s-lead", selectedPlanId: null })} />);
  expect(q(m.container, '[data-plan="p1"] button[data-select="plan"]')?.getAttribute("aria-current")).toBe("true");
  m.unmount();
  const other = mount(<ContextTree {...props({ selectedSessionId: "s-cfork", selectedPlanId: "p1" })} />);
  expect(q(other.container, '[data-plan="p1"] button[data-select="plan"]')?.hasAttribute("aria-current")).toBe(false);
  other.unmount();
});

it("selecting a Manager does not mark its Plans current", () => {
  const m = mount(<ContextTree {...props({ selectedSessionId: "s-mgr", selectedPlanId: "p1" })} />);
  expect(q(m.container, 'button[data-select="context"]')?.getAttribute("aria-current")).toBe("true");
  expect(m.container.querySelectorAll('button[data-select="plan"][aria-current]')).toHaveLength(0);
  m.unmount();
});

const withArchivedPlans = (): ContextTreeModel => {
  const p = { ...tree.contexts[0]!.plans[0]!, id: "p9", archived: true, sessions: [] };
  return { cursor: 1, contexts: [...tree.contexts, { ...tree.contexts[0]!, id: "c2", archived: true, sessions: [], plans: [p] }] };
};

it("section and context open state persists in localStorage under cpd-ui collapse keys", () => {
  const m = mount(<ContextTree {...props({ tree: withArchivedPlans() })} />);
  const key = (k: string) => store.getItem(`cpd-ui:collapse:${k}`);
  click(header(m.container, "Contexts"));
  expect(key("section:contexts")).toBe("0");
  click(header(m.container, "Contexts"));
  expect(key("section:contexts")).toBe("1");
  click(header(m.container, "Standing"));
  expect(key("section:standing")).toBe("0");
  click(header(m.container, "Archive"));
  expect(key("section:archive")).toBe("1");
  click(q(m.container, '[data-context="c1"] [data-slot="sidebar-menu-action"]'));
  expect(key("ctx:c1")).toBe("0");
  click(q(m.container, '[data-archived] [data-context="c2"] [data-slot="sidebar-menu-action"]'));
  expect(key("ctx:arch:c2")).toBe("0");
  m.unmount();
});

it("a closed section stays closed after a remount", () => {
  const first = mount(<ContextTree {...props({ tree: layered })} />);
  click(header(first.container, "Standing"));
  click(header(first.container, "Archive"));
  first.unmount();
  const again = mount(<ContextTree {...props({ tree: layered })} />);
  expect(q(again.container, '[data-standing] [data-context="cpd"]')).toBeNull();
  expect(q(again.container, '[data-archived] [data-context="gone"]')).not.toBeNull();
  again.unmount();
  const preset = fakeStorage({ "cpd-ui:collapse:section:contexts": "0" });
  vi.stubGlobal("localStorage", preset);
  const closed = mount(<ContextTree {...props({ tree: layered })} />);
  expect(q(closed.container, '[data-context="hot"]')).toBeNull();
  closed.unmount();
});
