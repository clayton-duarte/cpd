// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import { useContext } from "react";
import { mount } from "../../testing/render.tsx";
import { PlanCollapseContext, Shell } from "./Shell.tsx";
import { StatusBar } from "./StatusBar.tsx";

// react-resizable-panels is an external dependency and jsdom has no layout: a plain double
// that records collapse/expand and re-fires every onResize, as the library does.
vi.mock("react-resizable-panels", async () => {
  const React = await import("react");
  interface Handle { collapsed: boolean; collapse(): void; expand(): void; isCollapsed(): boolean }
  const registry = new Map<string, () => void>();
  const fire = () => {
    for (const cb of registry.values()) cb();
  };
  return {
    Group: (p: Record<string, unknown>) => {
      const { children, orientation: _o, ...rest } = p;
      return React.createElement("div", rest, children as never);
    },
    Panel: (p: Record<string, unknown>) => {
      const { children, panelRef, onResize, defaultSize: _d, minSize: _mi, maxSize: _ma, collapsible: _c, collapsedSize: _cs, ...rest } = p;
      const id = String(p.id);
      const handle = React.useRef<Handle | null>(null);
      if (handle.current === null) {
        const h: Handle = {
          collapsed: false,
          collapse() {
            h.collapsed = true;
            fire();
          },
          expand() {
            h.collapsed = false;
            fire();
          },
          isCollapsed: () => h.collapsed,
        };
        handle.current = h;
      }
      if (panelRef !== undefined) (panelRef as { current: Handle | null }).current = handle.current;
      React.useEffect(() => {
        if (onResize !== undefined) registry.set(id, onResize as () => void);
        return () => {
          registry.delete(id);
        };
      });
      return React.createElement("div", rest, children as never);
    },
    Separator: (p: Record<string, unknown>) => {
      const { children, ...rest } = p;
      return React.createElement("div", { role: "separator", tabIndex: 0, ...rest }, children as never);
    },
    usePanelRef: () => React.useRef(null),
    useDefaultLayout: () => ({ defaultLayout: undefined, onLayoutChanged: () => {} }),
  };
});

function click(el: Element | null): void {
  if (el === null) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

// A plan child that renders what TopPaneHeader renders from the context: the collapse button.
function PlanProbe() {
  const ctl = useContext(PlanCollapseContext);
  if (ctl === null) return <i id="p">plan</i>;
  return (
    <div data-part="plan-controls" role="group" aria-label="Plan pane controls">
      <button type="button" data-action="plan-collapse" aria-label={ctl.collapsed ? "Expand plan view" : "Collapse plan view"} onClick={ctl.toggle} />
    </div>
  );
}

function shell() {
  return mount(
    <Shell tree={<i id="t">tree</i>} plan={<PlanProbe />} chat={<i id="c">chat</i>} inbox={<i id="i">inbox</i>} status={<i>s</i>} />,
  );
}

// Title kept from the Job; the bar is the TUI's: five icon buttons, then the session pills until the machine status arrives.
it("status bar shows reset layout, legend, logs and the runner pill", () => {
  const onReset = vi.fn();
  const onCosts = vi.fn();
  const m = mount(
    <StatusBar runner="durable" state="idle" version={{ state: "known", value: "0.21.102" }} costsOn={true} onReset={onReset} onLegend={vi.fn()} onLogs={vi.fn()} onCosts={onCosts} onSettings={vi.fn()} />,
  );
  const c = m.container;
  expect(c.querySelector('[data-part="status-bar"]')).not.toBeNull();
  const reset = c.querySelector('button[aria-label="Reset layout"]');
  click(reset);
  expect(onReset).toHaveBeenCalledTimes(1);
  const cost = c.querySelector('button[aria-label="Costs"]');
  expect(cost?.getAttribute("aria-pressed")).toBe("true");
  click(cost);
  expect(onCosts).toHaveBeenCalledTimes(1);
  expect(c.querySelector('button[aria-label="Legend"]')).not.toBeNull();
  expect(c.querySelector('button[aria-label="Logs"]')).not.toBeNull();
  expect(c.querySelector('[data-part="status-right"] [data-part="runner"]')?.textContent).toBe("Runner: durable");
  const state = c.querySelector('[data-part="state"]');
  expect(state?.textContent).toBe("State: idle");
  expect(state?.getAttribute("data-tone")).toBe("neutral");
  expect(state?.getAttribute("data-glyph")).toBe("idle");
  expect(c.querySelector('[data-part="version"]')?.textContent).toBe("cpd: 0.21.102");
  m.unmount();
  const bare = mount(<StatusBar runner={null} state={null} version={null} />);
  expect(bare.container.querySelector('[data-part="runner"]')?.textContent).toBe("Runner: none");
  expect(bare.container.querySelector('[data-part="status-bar"]')).not.toBeNull();
  bare.unmount();
});

// Title kept from the Job; DESIGN.md 3b: the incident pill is Not wired, so it is absent and so are Legend and Logs.
it("status bar shows the incident pill only when an incident is present", () => {
  const m = mount(<StatusBar runner="pi" state="working" version={null} />);
  const c = m.container;
  expect(c.querySelector('[data-part="status-bar"]')).not.toBeNull();
  expect(c.querySelector('[data-part="incident"]')).toBeNull();
  expect(c.textContent).not.toMatch(/incident/i);
  expect(c.querySelectorAll("button")).toHaveLength(0);
  expect(c.querySelector('[data-part="state"]')?.getAttribute("data-glyph")).toBe("active");
  m.unmount();
});

it("plan pane can be collapsed and expanded", () => {
  const m = shell();
  const c = m.container;
  const center = c.querySelector('[data-column="center"]');
  expect(center?.getAttribute("data-plan-state")).toBe("open");
  expect(c.querySelector('section[aria-label="Plan pane"]')?.getAttribute("data-state")).toBe("open");
  click(c.querySelector('[role="group"][aria-label="Plan pane controls"] button[aria-label="Collapse plan view"]'));
  expect(center?.getAttribute("data-plan-state")).toBe("collapsed");
  const section = c.querySelector('section[aria-label="Plan pane"]');
  expect(section?.getAttribute("data-state")).toBe("collapsed");
  expect(section?.hasAttribute("inert")).toBe(false);
  expect(c.querySelector('main[aria-label="Chat"]')?.getAttribute("data-state")).toBe("open");
  click(c.querySelector('button[aria-label="Expand plan view"]'));
  expect(center?.getAttribute("data-plan-state")).toBe("open");
  expect(c.querySelector('section[aria-label="Plan pane"]')?.hasAttribute("inert")).toBe(false);
  expect(c.querySelector('button[aria-label="Collapse plan view"]')).not.toBeNull();
  m.unmount();
});

it("Shell provides PlanCollapseContext whose toggle collapses and expands the plan pane", () => {
  const seen: Array<boolean | null> = [];
  function Spy() {
    const ctl = useContext(PlanCollapseContext);
    seen.push(ctl === null ? null : ctl.collapsed);
    return <button id="tg" onClick={ctl?.toggle} />;
  }
  const m = mount(<Shell tree={<i>t</i>} plan={<Spy />} chat={<i>c</i>} inbox={<i>i</i>} />);
  const c = m.container;
  const center = c.querySelector('[data-column="center"]');
  expect(seen[seen.length - 1]).toBe(false);
  click(c.querySelector("#tg"));
  expect(seen[seen.length - 1]).toBe(true);
  expect(center?.getAttribute("data-plan-state")).toBe("collapsed");
  click(c.querySelector("#tg"));
  expect(seen[seen.length - 1]).toBe(false);
  expect(center?.getAttribute("data-plan-state")).toBe("open");
  m.unmount();
});

it("Shell renders no plan controls or ScrollArea of its own around the plan", () => {
  const m = mount(<Shell tree={<i>t</i>} plan={<i id="p">plan</i>} chat={<i>c</i>} inbox={<i>i</i>} />);
  const c = m.container;
  expect(c.querySelector('[data-part="plan-controls"]')).toBeNull();
  expect(c.querySelector('button[data-action="plan-collapse"]')).toBeNull();
  const section = c.querySelector('section[aria-label="Plan pane"]');
  expect(section?.querySelector("[data-slot=scroll-area]")).toBeNull();
  expect(section?.querySelector('[data-part="plan-scroll"]')).toBeNull();
  expect(section?.firstElementChild?.id).toBe("p");
  m.unmount();
});

it("the Plan pane is a flex column that is not inert when collapsed", () => {
  const m = shell();
  const c = m.container;
  const section = () => c.querySelector('section[aria-label="Plan pane"]');
  for (const k of ["flex", "h-full", "min-h-0", "flex-col", "overflow-hidden"]) expect(section()?.classList.contains(k)).toBe(true);
  click(c.querySelector('button[data-action="plan-collapse"]'));
  expect(section()?.getAttribute("data-state")).toBe("collapsed");
  expect(section()?.hasAttribute("inert")).toBe(false);
  m.unmount();
});

it("plan pane controls hold only the collapse button", () => {
  const m = shell();
  const group = m.container.querySelector('div[data-part="plan-controls"][role="group"][aria-label="Plan pane controls"]');
  expect(group).not.toBeNull();
  const buttons = [...(group?.querySelectorAll("button") ?? [])];
  expect(buttons.map((b) => b.getAttribute("data-action"))).toEqual(["plan-collapse"]);
  expect(buttons[0]?.getAttribute("aria-label")).toBe("Collapse plan view");
  m.unmount();
});

it("the plan pane has no zoom button in any state", () => {
  const m = shell();
  const c = m.container;
  const none = () => {
    expect(c.querySelector('button[data-action="plan-zoom"]')).toBeNull();
    expect(c.querySelector('button[aria-label="Zoom plan view"]')).toBeNull();
  };
  none();
  click(c.querySelector('button[aria-label="Collapse plan view"]'));
  none();
  click(c.querySelector('button[aria-label="Expand plan view"]'));
  none();
  m.unmount();
});

it("plan split exposes a keyboard operable separator", () => {
  const m = shell();
  const sep = m.container.querySelector('[role="separator"][aria-label="Resize plan view and chat"]');
  expect(sep).not.toBeNull();
  expect(sep?.getAttribute("aria-label")).toBe("Resize plan view and chat");
  expect(sep?.getAttribute("data-part")).toBe("split-handle");
  expect(sep?.getAttribute("tabindex")).toBe("0");
  m.unmount();
});

it("shell keeps its landmarks and data hooks without a cockpit header", () => {
  // A legacy `header` slot is passed through a loose spread: the removed slot must render nothing.
  const legacy = { header: <b data-emblem>C-P-D</b> };
  const m = mount(
    <Shell tree={<i id="t">tree</i>} plan={<i id="p">plan</i>} chat={<i id="c">chat</i>} inbox={<i id="i">inbox</i>} status={<i>s</i>} {...legacy} />,
  );
  const c = m.container;
  const root = c.querySelector("div[data-shell]");
  expect(root).not.toBeNull();
  expect([...(root?.children ?? [])].map((e) => e.getAttribute("aria-label") ?? e.getAttribute("data-part"))).toEqual([
    "columns-group",
    "Status bar",
  ]);
  const center = root?.querySelector('div[data-column="center"]');
  expect(center?.children).toHaveLength(1);
  expect(center?.getAttribute("data-part")).toBe("center-frame");
  expect(center?.querySelector('section[aria-label="Plan pane"] #p')).not.toBeNull();
  expect(center?.querySelector('main[aria-label="Chat"] #c')).not.toBeNull();
  expect(root?.querySelector('aside[aria-label="Context tree column"] #t')).not.toBeNull();
  expect(root?.querySelector('aside[aria-label="Inbox column"] #i')).not.toBeNull();
  expect(root?.querySelector('header[aria-label="Cockpit header"]')).toBeNull();
  expect(root?.querySelector("[data-emblem]")).toBeNull();
  m.unmount();
});
