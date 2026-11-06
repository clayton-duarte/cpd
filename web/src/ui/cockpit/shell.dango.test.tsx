// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { Header } from "./Header.tsx";
import { Shell } from "./Shell.tsx";
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

function shell() {
  return mount(
    <Shell tree={<i id="t">tree</i>} plan={<i id="p">plan</i>} chat={<i id="c">chat</i>} inbox={<i id="i">inbox</i>} header={<Header />} status={<i>s</i>} />,
  );
}

it("header shows the phi mark, breadcrumb, jump trigger and engine indicator", () => {
  const onOpenSearch = vi.fn();
  const m = mount(<Header location={{ context: "Context One", plan: "Plan One" }} engine={true} onOpenSearch={onOpenSearch} />);
  const c = m.container;
  expect(c.querySelector('[data-part="phi"]')?.textContent).toBe("φ");
  expect(c.querySelector("[data-emblem]")?.textContent).toBe("C-P-D");
  const nav = c.querySelector('nav[aria-label="Location"][data-part="breadcrumb"]');
  expect(nav?.querySelector('[data-part="breadcrumb-context"]')?.textContent).toBe("Context One");
  expect(nav?.querySelector('[data-part="breadcrumb-plan"]')?.textContent).toBe("Plan One");
  const search = c.querySelector('button[data-part="search"]');
  expect(search?.textContent).toContain("Jump to context, plan, job, PR…");
  expect(search?.textContent).toContain("⌘K");
  click(search);
  expect(onOpenSearch).toHaveBeenCalledTimes(1);
  const engine = c.querySelector('[data-part="engine"]');
  expect(engine?.getAttribute("data-state")).toBe("running");
  expect(engine?.textContent).toContain("engine");
  expect(engine?.textContent).toContain("running");
  m.unmount();
  const stopped = mount(<Header engine={false} />);
  expect(stopped.container.querySelector('[data-part="engine"]')?.getAttribute("data-state")).toBe("stopped");
  expect(stopped.container.querySelector('[data-part="engine"]')?.textContent).toContain("not running");
  expect(stopped.container.querySelector('button[data-part="search"]')).toBeNull();
  stopped.unmount();
  const unknown = mount(<Header engine={null} />);
  expect(unknown.container.querySelector('[data-part="engine"]')?.getAttribute("data-state")).toBe("unknown");
  expect(unknown.container.querySelector('[data-part="engine"]')?.textContent).toContain("unknown");
  unknown.unmount();
});

// Title kept from the Job; DESIGN.md 3a: no location means no breadcrumb, a null plan means the context alone.
it("header breadcrumb falls back when no plan is selected", () => {
  const none = mount(<Header location={null} engine={null} />);
  expect(none.container.querySelector('[data-part="phi"]')).not.toBeNull();
  expect(none.container.querySelector('[data-part="breadcrumb"]')).toBeNull();
  none.unmount();
  const noPlan = mount(<Header location={{ context: "Context One", plan: null }} />);
  const nav = noPlan.container.querySelector('nav[data-part="breadcrumb"]');
  expect(nav?.querySelector('[data-part="breadcrumb-context"]')?.textContent).toBe("Context One");
  expect(nav?.querySelector('[data-part="breadcrumb-plan"]')).toBeNull();
  expect(nav?.textContent).not.toContain("/");
  noPlan.unmount();
});

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
  expect(c.querySelector('[data-part="version"]')?.textContent).toBe("0.21.102");
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
  expect(section?.hasAttribute("inert")).toBe(true);
  expect(c.querySelector('main[aria-label="Chat"]')?.getAttribute("data-state")).toBe("open");
  const zoom = c.querySelector<HTMLButtonElement>('button[aria-label="Zoom plan view"]');
  expect(zoom?.disabled).toBe(true);
  click(c.querySelector('button[aria-label="Expand plan view"]'));
  expect(center?.getAttribute("data-plan-state")).toBe("open");
  expect(c.querySelector('section[aria-label="Plan pane"]')?.hasAttribute("inert")).toBe(false);
  expect(c.querySelector('button[aria-label="Collapse plan view"]')).not.toBeNull();
  m.unmount();
});

it("plan pane can be zoomed over the chat", () => {
  const m = shell();
  const c = m.container;
  const center = c.querySelector('[data-column="center"]');
  expect(c.querySelector('button[data-action="plan-zoom"]')?.getAttribute("aria-pressed")).toBe("false");
  click(c.querySelector('button[aria-label="Zoom plan view"]'));
  expect(center?.getAttribute("data-plan-state")).toBe("zoomed");
  expect(c.querySelector('section[aria-label="Plan pane"]')?.getAttribute("data-state")).toBe("zoomed");
  const chat = c.querySelector('main[aria-label="Chat"]');
  expect(chat?.getAttribute("data-state")).toBe("zoomed");
  expect(chat?.hasAttribute("inert")).toBe(true);
  const zoom = c.querySelector('button[aria-label="Zoom plan view"]');
  expect(zoom?.getAttribute("aria-pressed")).toBe("true");
  expect(c.querySelector<HTMLButtonElement>('button[data-action="plan-collapse"]')?.disabled).toBe(true);
  click(zoom);
  expect(center?.getAttribute("data-plan-state")).toBe("open");
  expect(c.querySelector('main[aria-label="Chat"]')?.getAttribute("data-state")).toBe("open");
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

it("shell keeps its landmarks and data hooks", () => {
  const m = shell();
  const c = m.container;
  const root = c.querySelector("div[data-shell]");
  expect(root).not.toBeNull();
  expect([...(root?.children ?? [])].map((e) => e.getAttribute("aria-label") ?? e.getAttribute("data-part"))).toEqual([
    "Cockpit header",
    "columns-group",
    "Status bar",
  ]);
  const center = root?.querySelector('div[data-column="center"]');
  expect(center?.children).toHaveLength(2);
  expect(center?.getAttribute("data-part")).toBe("center-frame");
  expect(center?.querySelector('section[aria-label="Plan pane"] #p')).not.toBeNull();
  expect(center?.querySelector('main[aria-label="Chat"] #c')).not.toBeNull();
  expect(root?.querySelector('aside[aria-label="Context tree column"] #t')).not.toBeNull();
  expect(root?.querySelector('aside[aria-label="Inbox column"] #i')).not.toBeNull();
  expect(root?.querySelector('header[aria-label="Cockpit header"] [data-emblem]')?.textContent).toBe("C-P-D");
  m.unmount();
});
