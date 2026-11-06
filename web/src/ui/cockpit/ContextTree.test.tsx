// @vitest-environment jsdom
import type { ReactElement } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
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
  expect(btn?.querySelector('[role="img"][data-dot]')?.getAttribute("data-dot")).toBe("fixing");
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
  expect(group?.closest('[data-part="tree-section"][data-section="archive"]')).not.toBeNull();
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
  Array.from(c.querySelectorAll<HTMLElement>('button[data-slot="sidebar-group-label"]')).find((b) => (b.textContent ?? "").includes(name)) ?? null;

const section = (c: ParentNode, name: string): HTMLElement | null => c.querySelector<HTMLElement>(`[data-part="tree-section"][data-section="${name}"]`);

it("every open section is flex-1 and a closed section is shrink-0", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const c = m.container;
  for (const name of ["contexts"]) {
    expect(section(c, name)?.className, `${name} open`).toContain("flex-1");
    expect(section(c, name)?.className).toContain("min-h-0");
    expect(section(c, name)?.getAttribute("data-state")).toBe("open");
  }
  expect(section(c, "archive")?.className, "archive closed").toContain("shrink-0");
  expect(section(c, "archive")?.className).not.toContain("flex-1");
  expect(section(c, "archive")?.getAttribute("data-state")).toBe("closed");
  click(header(c, "Archive"));
  expect(section(c, "archive")?.className, "archive open").toContain("flex-1");
  click(header(c, "Contexts"));
  expect(section(c, "contexts")?.className, "contexts closed").toContain("shrink-0");
  m.unmount();
});

it("no section has a max height of 50 percent", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  expect(m.container.innerHTML).not.toContain("max-h-[50%]");
  expect(q(m.container, '[data-part="tree-pinned-bottom"]')).toBeNull();
  m.unmount();
});

it("each section label is outside the scroll area and only the section body scrolls", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  click(header(m.container, "Archive"));
  for (const name of ["contexts", "archive"]) {
    const sec = section(m.container, name);
    const label = sec?.querySelector('[data-slot="sidebar-group-label"]');
    expect(label, name).not.toBeNull();
    expect(label?.closest('[data-slot="scroll-area"]'), `${name} label in scroll area`).toBeNull();
    const areas = sec?.querySelectorAll('[data-slot="scroll-area"]') ?? [];
    expect(areas, `${name} scroll areas`).toHaveLength(1);
    expect(areas[0]?.closest('[data-slot="collapsible-content"]'), `${name} area in content`).not.toBeNull();
    const group = label?.closest('[data-slot="sidebar-group"]');
    expect(group, name).not.toBeNull();
    expect(group?.contains(areas[0] ?? null), `${name} same group`).toBe(true);
  }
  click(header(m.container, "Archive"));
  expect(section(m.container, "archive")?.querySelectorAll('[data-slot="scroll-area"]')).toHaveLength(0);
  m.unmount();
});

it("sections are direct flex children of the sidebar content separated by shrink-0 separators", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const content = m.container.querySelector('[data-slot="sidebar-content"]');
  expect(content).not.toBeNull();
  const sections = Array.from(m.container.querySelectorAll('[data-part="tree-section"]'));
  for (const sec of sections) expect(sec.parentElement).toBe(content);
  const seps = Array.from(m.container.querySelectorAll('[data-slot="separator"]'));
  for (const sep of seps) {
    expect(sep.parentElement).toBe(content);
    expect(sep.className).toContain("shrink-0");
  }
  expect(section(m.container, "contexts")?.className).toContain("min-h-0 flex-1");
  expect(section(m.container, "archive")?.className).toContain("shrink-0");
  m.unmount();
});

it("sections are separated by separators and Archive renders only when something is archived", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  expect(section(m.container, "archive")).not.toBeNull();
  m.unmount();
  const none: ContextTreeModel = { cursor: 1, contexts: layered.contexts.filter((x) => !x.archived) };
  const n = mount(<ContextTree {...props({ tree: none })} />);
  expect(section(n.container, "archive")).toBeNull();
  n.unmount();
});

it("the Contexts section lists live Contexts by dot tier then newest first", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const top = section(m.container, "contexts");
  expect(ids(top).filter((id) => !["cpd", "host", "codereview"].includes(id))).toEqual(["hot", "new", "old"]);
  m.unmount();
});

it("the tree has no Standing section or Standing header", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  expect(section(m.container, "standing")).toBeNull();
  expect(header(m.container, "Standing")).toBeNull();
  expect(q(m.container, "[data-standing]")).toBeNull();
  const names = Array.from(m.container.querySelectorAll('[data-part="tree-section"]')).map((e) => e.getAttribute("data-section"));
  expect(names).toEqual(["contexts", "archive"]);
  m.unmount();
});

it("standing Contexts are rows of the Contexts section sorted with the others", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  expect(ids(section(m.container, "contexts"))).toEqual(["hot", "new", "old", "cpd", "host", "codereview"]);
  m.unmount();
});

it("a standing Context row shows an aria-hidden phi immediately after its context-id span in a flex wrapper", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const row = q(section(m.container, "contexts") ?? m.container, '[data-context="cpd"]');
  const idSpan = row?.querySelector('span[data-part="context-id"]') ?? null;
  expect(idSpan?.textContent).toBe("cpd");
  const phi = idSpan?.nextElementSibling ?? null;
  expect(phi?.getAttribute("data-part")).toBe("phi");
  expect(phi?.tagName).toBe("SPAN");
  expect(phi?.getAttribute("aria-hidden")).toBe("true");
  expect(phi?.textContent).toBe("φ");
  expect(phi?.className).toContain("font-bold");
  const wrap = idSpan?.parentElement ?? null;
  expect(wrap?.firstElementChild).toBe(idSpan);
  expect(phi?.parentElement).toBe(wrap);
  for (const cls of ["flex", "min-w-0", "flex-1", "items-center"]) expect(wrap?.className.split(/\s+/), cls).toContain(cls);
  expect(wrap?.className).toContain("gap-");
  expect(idSpan?.className.split(/\s+/)).toContain("truncate");
  expect(idSpan?.className.split(/\s+/)).not.toContain("flex-1");
  m.unmount();
});

it("a regular Context row shows no phi", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  for (const id of ["hot", "new", "old"]) {
    const row = q(m.container, `[data-context="${id}"]`);
    expect(row?.querySelector('span[data-part="context-id"]'), id).not.toBeNull();
    expect(row?.querySelector('[data-part="phi"]'), id).toBeNull();
  }
  expect(m.container.querySelectorAll('[data-part="phi"]'), "one phi per standing row").toHaveLength(3);
  m.unmount();
});

it("the Standing section collapse key is never written", () => {
  const preset = fakeStorage({ "cpd-ui:collapse:section:standing": "0" });
  const set = vi.spyOn(preset, "setItem");
  vi.stubGlobal("localStorage", preset);
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  expect(q(section(m.container, "contexts") ?? m.container, '[data-context="cpd"]'), "stale key does not hide cpd").not.toBeNull();
  click(header(m.container, "Contexts"));
  click(header(m.container, "Contexts"));
  click(header(m.container, "Archive"));
  expect(set.mock.calls.map((c) => c[0]).filter((k) => String(k).includes("standing"))).toEqual([]);
  expect(preset.getItem("cpd-ui:collapse:section:standing")).toBe("0");
  m.unmount();
});

it("an archived standing Context is not listed in the Archive section", () => {
  const t: ContextTreeModel = { cursor: 1, contexts: layered.contexts.map((c) => (c.id === "cpd" ? { ...c, archived: true } : c)) };
  const m = mount(<ContextTree {...props({ tree: t })} />);
  click(header(m.container, "Archive"));
  expect(ids(q(m.container, "[data-archived]"))).toEqual(["gone"]);
  expect(ids(section(m.container, "contexts"))).toEqual(["hot", "new", "old", "host", "codereview"]);
  m.unmount();
});

it("each section header shows a minus icon when open and a plus icon when closed", () => {
  const m = mount(<ContextTree {...props({ tree: layered })} />);
  const icon = (name: string) => header(m.container, name)?.querySelector("svg")?.getAttribute("class") ?? "";
  for (const name of ["Contexts"]) {
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
  expect(archive()?.closest('[data-part="tree-section"][data-section="archive"]')).not.toBeNull();
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
  click(header(first.container, "Archive"));
  first.unmount();
  const again = mount(<ContextTree {...props({ tree: layered })} />);
  expect(q(again.container, '[data-archived] [data-context="gone"]')).not.toBeNull();
  again.unmount();
  const preset = fakeStorage({ "cpd-ui:collapse:section:contexts": "0" });
  vi.stubGlobal("localStorage", preset);
  const closed = mount(<ContextTree {...props({ tree: layered })} />);
  expect(q(closed.container, '[data-context="hot"]')).toBeNull();
  closed.unmount();
});

// pro-d45: the dot of a row is a lucide icon; hooks data-dot, data-tone, data-work.
type Dot = SessionNode["dot"];
const dotMgr = (state: Dot["state"], attention: Dot["attention"] = null): SessionNode => session("m", "c1", state, null, { dot: { state, attention } });
const zero = { draft: 0, queued: 0, running: 0, merging: 0, merged: 0, failed: 0, cancelled: 0 };

function dotTree(opts: { dot?: Dot; archived?: boolean; plan?: { phase?: string; counts?: Partial<typeof zero>; dot: Dot } }): ContextTreeModel {
  const ctx: ContextNode = {
    id: "c1", title: "C", archived: opts.archived ?? false, records: 0, openQuestions: 0,
    sessions: [dotMgr(opts.dot?.state ?? "idle", opts.dot?.attention ?? null)],
    plans: opts.plan === undefined ? [] : [{
      id: "p1", contextId: "c1", title: "P", phase: opts.plan.phase ?? "dispatch", archived: false,
      counts: { ...zero, ...opts.plan.counts }, leadSessionId: "l",
      sessions: [session("l", "c1:p1", opts.plan.dot.state, null, { planId: "p1", role: "lead", dot: opts.plan.dot })],
    }],
  };
  return { cursor: 1, contexts: [ctx] };
}
const dotOf = (c: ParentNode, sel: string): Element => {
  const el = c.querySelector(`${sel} [role="img"][data-dot]`);
  if (el === null) throw new Error("dot not found");
  return el;
};
const ctxDot = (c: ParentNode) => dotOf(c, '[data-context="c1"] > button[data-select="context"]');
const planDot = (c: ParentNode) => dotOf(c, '[data-plan="p1"] button[data-select="plan"]');
// lucide-react 1.65 sets no lucide-* class: tell the icons apart by their SVG children.
const shape = (el: Element): string => {
  const kids = Array.from(el.children).map((k) => k.tagName.toLowerCase());
  if (kids.length === 1 && kids[0] === "circle") return "circle";
  if (kids.length === 1 && kids[0] === "path") return "loader-circle";
  if (kids.length > 1 && kids.every((k) => k === "path")) return "circle-dashed";
  return `unknown:${kids.join(",")}`;
};
const icon = (el: Element, name: string): void => {
  expect(el.tagName.toLowerCase()).toBe("svg");
  expect(shape(el)).toBe(name);
  expect(el.classList.contains("!size-3.5")).toBe(true);
  expect(el.classList.contains("shrink-0")).toBe(true);
};
const has = (el: Element, ...cls: string[]): void => {
  for (const c of cls) expect(el.classList.contains(c), c).toBe(true);
};
const lacks = (el: Element, ...cls: string[]): void => {
  for (const c of cls) expect(el.classList.contains(c), c).toBe(false);
};
function dotsOf(model: ContextTreeModel, pick: (c: ParentNode) => Element, check: (el: Element) => void): void {
  const m = mount(<ContextTree {...props({ tree: model })} />);
  check(pick(m.container));
  m.unmount();
}

it("a needs-you row shows a filled circle in the colour of its attention", () => {
  const colours = {
    red: "text-status-blocked-bright",
    yellow: "text-status-landing-bright",
    magenta: "text-status-decision-bright",
    cyan: "text-status-active-bright",
    white: "text-fg-strong",
  } as const;
  for (const [attention, colour] of Object.entries(colours)) {
    dotsOf(dotTree({ dot: { state: "waiting", attention: attention as Dot["attention"] } }), ctxDot, (el) => {
      icon(el, "circle");
      has(el, "fill-current", colour);
      lacks(el, "animate-spin");
      expect(el.getAttribute("data-dot")).toBe("waiting");
    });
  }
});

it("a working Plan row shows a spinning loader coloured by its work state", () => {
  const working: Dot = { state: "working", attention: null };
  const cases: Array<[string, string, Partial<typeof zero>, string, string]> = [
    ["plan", "plan", { draft: 1 }, "planning", "text-status-decision-bright"],
    ["grounding", "grounding", {}, "planning", "text-status-decision-bright"],
    ["done", "done", {}, "planning", "text-status-decision-bright"],
    ["dispatch", "dispatch", { running: 1 }, "dispatch", "text-status-active-bright"],
    ["deploy", "deploy", {}, "review", "text-status-landing-bright"],
    ["merging", "dispatch", { merging: 1 }, "review", "text-status-landing-bright"],
  ];
  for (const [, phase, counts, work, colour] of cases) {
    dotsOf(dotTree({ plan: { phase, counts, dot: working } }), planDot, (el) => {
      icon(el, "loader-circle");
      has(el, "animate-spin", colour);
      expect(el.getAttribute("data-dot")).toBe("working");
      expect(el.getAttribute("data-work")).toBe(work);
    });
  }
});

it("a working Plan with failed jobs shows the fixing loader", () => {
  dotsOf(dotTree({ plan: { counts: { failed: 2 }, dot: { state: "working", attention: null } } }), planDot, (el) => {
    icon(el, "loader-circle");
    has(el, "animate-spin", "text-status-blocked-bright");
    expect(el.getAttribute("data-dot")).toBe("fixing");
    expect(el.getAttribute("data-tone")).toBe("blocked");
  });
  // not working: no fixing loader
  dotsOf(dotTree({ plan: { counts: { failed: 2 }, dot: { state: "idle", attention: null } } }), planDot, (el) => {
    expect(el.getAttribute("data-dot")).toBe("idle");
    lacks(el, "animate-spin");
  });
});

it("a working Manager or Fork row shows a strong-foreground loader", () => {
  dotsOf(dotTree({ dot: { state: "working", attention: null } }), ctxDot, (el) => {
    icon(el, "loader-circle");
    has(el, "animate-spin", "text-fg-strong");
    expect(el.getAttribute("data-dot")).toBe("working");
    expect(el.getAttribute("data-work")).toBe("session");
  });
});

it("a waiting row shows a dashed circle", () => {
  dotsOf(dotTree({ dot: { state: "waiting", attention: null } }), ctxDot, (el) => {
    icon(el, "circle-dashed");
    has(el, "text-status-active-bright");
    lacks(el, "animate-spin", "fill-current");
    expect(el.getAttribute("data-dot")).toBe("waiting");
  });
});

it("an idle row shows a hollow circle", () => {
  dotsOf(dotTree({ dot: { state: "idle", attention: null } }), ctxDot, (el) => {
    icon(el, "circle");
    has(el, "text-fg-strong");
    lacks(el, "fill-current", "animate-spin");
    expect(el.getAttribute("data-dot")).toBe("idle");
  });
});

it("a stopped or archived row shows a dim circle and never spins", () => {
  dotsOf(dotTree({ dot: { state: "stopped", attention: null } }), ctxDot, (el) => {
    icon(el, "circle");
    has(el, "text-fg-dim");
    lacks(el, "fill-current", "animate-spin");
    expect(el.getAttribute("data-dot")).toBe("stopped");
  });
  const archived = (dot: Dot): Element => {
    const m = mount(<ContextTree {...props({ tree: dotTree({ dot, archived: true }) })} />);
    const toggle = m.container.querySelector('[data-archived] button[data-slot="sidebar-group-label"]');
    act(() => toggle?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    const el = ctxDot(m.container).cloneNode(true) as Element;
    m.unmount();
    store.clear();
    return el;
  };
  const spinning = archived({ state: "working", attention: null });
  icon(spinning, "circle");
  has(spinning, "text-fg-dim");
  lacks(spinning, "animate-spin", "fill-current");
  const attentive = archived({ state: "waiting", attention: "red" });
  icon(attentive, "circle");
  has(attentive, "text-fg-dim", "fill-current");
  lacks(attentive, "animate-spin", "text-status-blocked-bright");
});

// ---- context menus (pro-d46) ----
// Radix opens a ContextMenu on a `contextmenu` event; the content is portalled to document.body and items select on click.
const E = "\u2026";
const menuOf = () => document.body.querySelector<HTMLElement>('[data-slot="context-menu-content"]');
const labels = () => Array.from(menuOf()?.querySelectorAll('[role="menuitem"]') ?? []).map((i) => i.textContent);
const item = (label: string) => Array.from(menuOf()?.querySelectorAll('[role="menuitem"]') ?? []).find((i) => i.textContent === label) ?? null;
function rightClick(el: Element | null): void {
  if (el === null) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 80 }));
  });
}
const withMenu = () => ({ ...props(), onAction: vi.fn() });
const ctxRow = (id: string) => `[data-context="${id}"] > button[data-select="context"]`;
const planRow = (id: string) => `[data-plan="${id}"] > button[data-select="plan"]`;

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
});

/** The tree with the Plan's Lead stopped: the Plan, and so the Context, is quiet. */
const quietTree = (): ContextTreeModel => {
  const c = tree.contexts[0]!;
  const p = c.plans[0]!;
  const lead = session("s-lead", "c1:p1", "stopped", null, { planId: "p1", role: "lead" });
  return { cursor: 1, contexts: [{ ...c, plans: [{ ...p, sessions: [lead, ...p.sessions.slice(1)] }] }] };
};
const standingTree = (): ContextTreeModel => ({
  cursor: 1,
  contexts: [...tree.contexts, { id: "cpd", title: "cpd", archived: false, records: 0, openQuestions: 0, sessions: [session("s-cpd", "cpd", "idle", null)], plans: [] }],
});

it("right click on a Context row opens a menu with New Plan, Rename, Copy id, Archive and Delete", () => {
  const p = withMenu();
  const m = mount(<ContextTree {...p} tree={quietTree()} />);
  rightClick(q(m.container, ctxRow("c1")));
  expect(menuOf()?.getAttribute("data-menu-kind"), "menu kind").toBe("context");
  expect(labels()).toEqual([`New Plan${E}`, `Rename${E}`, "Copy id", "Archive", "Delete"]);
  click(item("Archive"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "context.archive", contextId: "c1" });
  m.unmount();
});

it("the Delete item is offered only when the Context or Plan is quiet", () => {
  const busy = mount(<ContextTree {...withMenu()} />);
  rightClick(q(busy.container, ctxRow("c1")));
  expect(labels(), "busy Context").toEqual([`New Plan${E}`, `Rename${E}`, "Copy id", "Archive"]);
  busy.unmount();
  const busyPlan = mount(<ContextTree {...withMenu()} />);
  rightClick(q(busyPlan.container, planRow("p1")));
  expect(labels(), "busy Plan").toEqual([`New session${E}`, `Rename${E}`, "Copy id", "Archive"]);
  busyPlan.unmount();
  const quiet = mount(<ContextTree {...withMenu()} tree={quietTree()} />);
  rightClick(q(quiet.container, planRow("p1")));
  expect(labels().at(-1), "quiet Plan").toBe("Delete");
  quiet.unmount();
  const quietCtx = mount(<ContextTree {...withMenu()} tree={quietTree()} />);
  rightClick(q(quietCtx.container, ctxRow("c1")));
  expect(labels().at(-1), "quiet Context").toBe("Delete");
  quietCtx.unmount();
});

it("a standing Context menu in the Contexts section offers only New Plan and Copy id", () => {
  const m = mount(<ContextTree {...withMenu()} tree={standingTree()} />);
  rightClick(q(m.container, `[data-section="contexts"] ${ctxRow("cpd")}`));
  expect(menuOf()?.getAttribute("data-menu-kind"), "menu kind").toBe("permanent");
  expect(labels()).toEqual([`New Plan${E}`, "Copy id"]);
  m.unmount();
});

it("right click on a Plan row opens a menu with New session, Rename, Copy id, Archive and Delete", () => {
  const p = withMenu();
  const m = mount(<ContextTree {...p} tree={quietTree()} />);
  rightClick(q(m.container, planRow("p1")));
  expect(menuOf()?.getAttribute("data-menu-kind"), "menu kind").toBe("plan");
  expect(labels()).toEqual([`New session${E}`, `Rename${E}`, "Copy id", "Archive", "Delete"]);
  click(item("Archive"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "plan.archive", planId: "p1" });
  m.unmount();
});

it("right click on a Fork row offers Open, Rename, Fork, Copy id, Close and Archive", () => {
  const p = withMenu();
  const m = mount(<ContextTree {...p} />);
  rightClick(q(m.container, 'button[data-session="s-cfork"]'));
  expect(menuOf()?.getAttribute("data-menu-kind"), "menu kind").toBe("fork");
  expect(labels()).toEqual(["Open", `Rename${E}`, `Fork${E}`, "Copy id", "Close", "Archive"]);
  click(item("Close"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "session.close", sessionId: "s-cfork" });
  m.unmount();
  const open = withMenu();
  const again = mount(<ContextTree {...open} />);
  rightClick(q(again.container, 'button[data-session="s-cfork"]'));
  click(item("Open"));
  expect(open.onSelectSession).toHaveBeenCalledExactlyOnceWith("s-cfork");
  expect(open.onAction).not.toHaveBeenCalled();
  again.unmount();
});

it("right click on an archived row offers Rename, Copy id and Unarchive, plus Delete on a Context or Plan", () => {
  vi.stubGlobal("localStorage", fakeStorage({ "cpd-ui:collapse:section:archive": "1" }));
  const p = withMenu();
  const m = mount(<ContextTree {...p} tree={withArchivedPlans()} />);
  rightClick(q(m.container, `[data-archived] ${ctxRow("c2")}`));
  expect(menuOf()?.getAttribute("data-menu-kind"), "Context kind").toBe("archived");
  expect(labels(), "archived Context").toEqual([`Rename${E}`, "Copy id", "Unarchive", "Delete"]);
  click(item("Unarchive"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "context.unarchive", contextId: "c2" });
  m.unmount();
  const plans = withMenu();
  const again = mount(<ContextTree {...plans} tree={withArchivedPlans()} />);
  rightClick(q(again.container, `[data-archived] ${planRow("p9")}`));
  expect(menuOf()?.getAttribute("data-menu-kind"), "Plan kind").toBe("archived");
  expect(labels(), "archived Plan").toEqual([`Rename${E}`, "Copy id", "Unarchive", "Delete"]);
  click(item("Unarchive"));
  expect(plans.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "plan.unarchive", planId: "p9" });
  again.unmount();
});

it("Copy id writes the row id to the clipboard", () => {
  const writeText = vi.fn(async () => undefined);
  vi.stubGlobal("navigator", { userAgent: navigator.userAgent, clipboard: { writeText } });
  const p = withMenu();
  const m = mount(<ContextTree {...p} />);
  rightClick(q(m.container, planRow("p1")));
  click(item("Copy id"));
  expect(writeText).toHaveBeenCalledExactlyOnceWith("p1");
  rightClick(q(m.container, ctxRow("c1")));
  click(item("Copy id"));
  expect(writeText).toHaveBeenLastCalledWith("c1");
  expect(p.onAction).not.toHaveBeenCalled();
  m.unmount();
});

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]');
const field = () => dialog()?.querySelector<HTMLInputElement>("input") ?? null;
function type(el: HTMLInputElement | null, value: string): void {
  if (el === null) throw new Error("input not found");
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const button = (label: string) => Array.from(dialog()?.querySelectorAll("button") ?? []).find((b) => b.textContent === label) ?? null;

it("Rename and New Plan open a dialog with an input and submit the typed slug through onAction", () => {
  const p = withMenu();
  const m = mount(<ContextTree {...p} />);
  rightClick(q(m.container, ctxRow("c1")));
  click(item(`Rename${E}`));
  expect(dialog(), "rename dialog").not.toBeNull();
  expect(field()?.value, "starts from the current title").toBe("Context One");
  type(field(), "Better name");
  click(button("OK"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "context.rename", contextId: "c1", title: "Better name" });
  expect(dialog(), "dialog closes after submit").toBeNull();

  p.onAction.mockClear();
  rightClick(q(m.container, ctxRow("c1")));
  click(item(`New Plan${E}`));
  expect(field()?.value, "New Plan starts empty").toBe("");
  type(field(), "Fix the login page");
  click(button("OK"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "plan.new", planId: "fix-the-login-page", contextId: "c1", title: "Fix the login page" });

  p.onAction.mockClear();
  rightClick(q(m.container, planRow("p1")));
  click(item(`New session${E}`));
  type(field(), "side quest");
  click(button("OK"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "session.fork", sourceId: "s-lead", name: "side quest" });
  m.unmount();
});

it("the prompt dialog refuses an empty or invalid slug", () => {
  const p = withMenu();
  const m = mount(<ContextTree {...p} />);
  rightClick(q(m.container, ctxRow("c1")));
  click(item(`New Plan${E}`));
  click(button("OK"));
  expect(p.onAction, "empty title").not.toHaveBeenCalled();
  expect(dialog(), "stays open on empty").not.toBeNull();
  expect(dialog()?.textContent, "empty error").toContain("Title is required.");
  expect(field()?.getAttribute("aria-invalid"), "invalid flag").toBe("true");
  type(field(), "!!");
  click(button("OK"));
  expect(p.onAction, "title without a usable id").not.toHaveBeenCalled();
  expect(dialog(), "stays open on invalid slug").not.toBeNull();
  expect(field()?.getAttribute("aria-invalid"), "invalid flag for slug").toBe("true");
  click(button("Cancel"));
  expect(dialog(), "cancel closes").toBeNull();
  expect(p.onAction).not.toHaveBeenCalled();
  m.unmount();
});

it("draft: archiving a session deletes only that session draft", () => {
  window.localStorage.clear();
  window.localStorage.setItem("cpd-ui:draft:s-cfork", "keep?");
  window.localStorage.setItem("cpd-ui:draft:s-mgr", "other");
  const p = withMenu();
  const m = mount(<ContextTree {...p} />);
  rightClick(q(m.container, 'button[data-session="s-cfork"]'));
  click(item("Archive"));
  expect(p.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "session.archive", sessionId: "s-cfork" });
  expect(window.localStorage.getItem("cpd-ui:draft:s-cfork")).toBeNull();
  expect(window.localStorage.getItem("cpd-ui:draft:s-mgr")).toBe("other");
  m.unmount();
  window.localStorage.clear();
});

// Manager auto-collapse (pro-d78)
function remountable(el: ReactElement) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => root.render(el));
  return {
    container,
    render: (next: ReactElement) => act(() => root.render(next)),
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}
const toggleOf = (c: ParentNode, id: string, scope = "") => q(c, `${scope} [data-context="${id}"] button[aria-label="Toggle ${id}"]`.trim());
const stateOf = (c: ParentNode, id: string, scope = "") => toggleOf(c, id, scope)?.getAttribute("data-state");
const hasPlan = (c: ParentNode, id: string) => q(c, `[data-context="c1"] [data-plan="${id}"]`) !== null;
const hasFork = (c: ParentNode) => q(c, 'button[data-session="s-cfork"]') !== null;
const treeSel = (id: string | null) => <ContextTree {...props({ selectedSessionId: id })} />;

const twoContexts = (): ContextTreeModel => {
  const c2: ContextNode = {
    ...tree.contexts[0]!,
    id: "c2",
    title: "Context Two",
    sessions: [session("s-mgr2", "c2", "waiting", null, { contextId: "c2" }), session("s-c2fork", "c2-fork", "idle", null, { role: "lead", contextId: "c2" })],
    plans: [],
  };
  return { cursor: 1, contexts: [...tree.contexts, c2] };
};

it("selecting a Manager collapses that Context's tree", () => {
  const m = remountable(treeSel(null));
  expect(stateOf(m.container, "c1")).toBe("open");
  expect(hasPlan(m.container, "p1")).toBe(true);
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  expect(hasPlan(m.container, "p1")).toBe(false);
  expect(hasFork(m.container)).toBe(false);
  expect(q(m.container, '[data-context="c1"] button[data-select="context"]')).not.toBeNull();
  m.unmount();
});

it("selecting a Manager does not rewrite the persisted collapse state", () => {
  const m = remountable(treeSel(null));
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  expect(store.getItem("cpd-ui:collapse:ctx:c1")).toBeNull();
  m.render(treeSel("s-lead"));
  expect(stateOf(m.container, "c1")).toBe("open");
  expect(store.getItem("cpd-ui:collapse:ctx:c1")).not.toBe("0");
  m.unmount();
});

it("navigating from a Manager to a Lead expands the Context's tree again", () => {
  const m = remountable(treeSel(null));
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  m.render(treeSel("s-lead"));
  expect(stateOf(m.container, "c1")).toBe("open");
  expect(hasPlan(m.container, "p1")).toBe(true);
  expect(hasFork(m.container)).toBe(true);
  m.unmount();
});

it("navigating from a Manager to another Context's session expands the tree again", () => {
  const t = twoContexts();
  const el = (id: string | null) => <ContextTree {...props({ tree: t, selectedSessionId: id })} />;
  const m = remountable(el(null));
  m.render(el("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  m.render(el("s-c2fork"));
  expect(stateOf(m.container, "c1")).toBe("open");
  expect(hasPlan(m.container, "p1")).toBe(true);
  m.unmount();
});

it("a Context the User had closed opens when its Manager is deselected", () => {
  vi.stubGlobal("localStorage", fakeStorage({ "cpd-ui:collapse:ctx:c1": "0" }));
  const m = remountable(treeSel(null));
  expect(stateOf(m.container, "c1")).toBe("closed");
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  m.render(treeSel("s-lead"));
  expect(stateOf(m.container, "c1")).toBe("open");
  expect(hasPlan(m.container, "p1")).toBe(true);
  m.unmount();
});

it("the User can reopen a selected Manager's tree by hand until the selection changes", () => {
  const m = remountable(treeSel(null));
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  click(toggleOf(m.container, "c1"));
  expect(stateOf(m.container, "c1")).toBe("open");
  expect(hasPlan(m.container, "p1")).toBe(true);
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("open");
  m.render(treeSel("s-lead"));
  expect(stateOf(m.container, "c1")).toBe("open");
  m.render(treeSel("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  m.unmount();
});

it("selecting a Manager leaves other Contexts' trees open", () => {
  const t = twoContexts();
  const el = (id: string | null) => <ContextTree {...props({ tree: t, selectedSessionId: id })} />;
  const m = remountable(el(null));
  m.render(el("s-mgr"));
  expect(stateOf(m.container, "c1")).toBe("closed");
  expect(stateOf(m.container, "c2")).toBe("open");
  expect(q(m.container, 'button[data-session="s-c2fork"]')).not.toBeNull();
  m.unmount();
});

it("Archive section rows ignore the Manager auto-collapse", () => {
  vi.stubGlobal("localStorage", fakeStorage({ "cpd-ui:collapse:section:archive": "1" }));
  const base = tree.contexts[0]!;
  const t: ContextTreeModel = {
    cursor: 1,
    contexts: [{ ...base, plans: [...base.plans, { ...base.plans[0]!, id: "p9", archived: true, sessions: [] }] }],
  };
  const el = (id: string | null) => <ContextTree {...props({ tree: t, selectedSessionId: id })} />;
  const m = remountable(el(null));
  m.render(el("s-mgr"));
  expect(stateOf(m.container, "c1", "[data-archived]")).toBe("open");
  expect(q(m.container, '[data-archived] [data-plan="p9"]')).not.toBeNull();
  expect(stateOf(m.container, "c1", "[data-part=tree-section][data-section=contexts]")).toBe("closed");
  m.unmount();
});
