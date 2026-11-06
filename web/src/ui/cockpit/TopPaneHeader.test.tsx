// @vitest-environment jsdom
import { createRef } from "react";
import { act } from "react";
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { PlanCollapseContext } from "./Shell.tsx";
import { TopPaneHeader } from "./TopPaneHeader.tsx";

const header = (c: HTMLElement) => c.querySelector<HTMLElement>("header");

it("TopPaneHeader renders an h-11 header with a bottom border and is not sticky", () => {
  const m = mount(<TopPaneHeader>x</TopPaneHeader>);
  const h = header(m.container);
  expect(h).not.toBeNull();
  for (const c of ["flex", "h-11", "shrink-0", "items-center", "border-b", "border-border", "bg-background", "px-4"]) {
    expect(h!.classList.contains(c)).toBe(true);
  }
  for (const c of ["sticky", "top-0", "z-10"]) expect(h!.classList.contains(c)).toBe(false);
  m.unmount();
});

it("TopPaneHeader puts the children first, a flex-1 spacer, then the actions", () => {
  const m = mount(<TopPaneHeader actions={<button data-part="act">go</button>}><span data-part="left">L</span></TopPaneHeader>);
  const kids = Array.from(header(m.container)?.children ?? []);
  expect(kids.map((k) => k.getAttribute("data-part") ?? k.className)).toEqual(["left", "flex-1", "act"]);
  m.unmount();
});

it("TopPaneHeader takes its data-part and defaults to pane-header", () => {
  const d = mount(<TopPaneHeader>x</TopPaneHeader>);
  expect(header(d.container)?.getAttribute("data-part")).toBe("pane-header");
  d.unmount();
  const n = mount(<TopPaneHeader dataPart="tree-header">x</TopPaneHeader>);
  expect(header(n.container)?.getAttribute("data-part")).toBe("tree-header");
  n.unmount();
});

const ctl = (collapsed: boolean, toggle = vi.fn()) => ({ collapsed, toggle });
const withCtx = (c: { collapsed: boolean; toggle: () => void }, el = <TopPaneHeader actions={<button data-part="act">go</button>}><span data-part="left">L</span></TopPaneHeader>) => (
  <PlanCollapseContext.Provider value={c}>{el}</PlanCollapseContext.Provider>
);
const GROUP = 'div[data-part="plan-controls"][role="group"][aria-label="Plan pane controls"]';

it("TopPaneHeader renders the plan collapse button after the actions when a PlanCollapseContext is provided", () => {
  const m = mount(withCtx(ctl(false)));
  const kids = Array.from(header(m.container)?.children ?? []);
  expect(kids.map((k) => k.getAttribute("data-part") ?? k.className)).toEqual(["left", "flex-1", "act", "plan-controls"]);
  const group = kids[3]!;
  expect(group.matches(GROUP)).toBe(true);
  for (const c of ["flex", "shrink-0", "items-center", "gap-0.5"]) expect(group.classList.contains(c)).toBe(true);
  const buttons = Array.from(group.querySelectorAll("button"));
  expect(buttons.map((b) => b.getAttribute("data-action"))).toEqual(["plan-collapse"]);
  for (const c of ["size-[26px]", "rounded-[5px]", "text-fg-strong"]) expect(buttons[0]!.classList.contains(c)).toBe(true);
  m.unmount();
});

it("TopPaneHeader plan collapse button labels and icons follow the collapsed state and its click calls toggle", () => {
  const toggle = vi.fn();
  const open = mount(withCtx(ctl(false, toggle)));
  const b = open.container.querySelector<HTMLElement>('button[data-action="plan-collapse"]');
  expect(b?.getAttribute("aria-label")).toBe("Collapse plan view");
  expect(b?.querySelector("svg.lucide-chevron-up")).not.toBeNull();
  expect(b?.querySelector("svg.lucide-chevron-down")).toBeNull();
  act(() => { b?.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
  expect(toggle).toHaveBeenCalledTimes(1);
  open.unmount();
  const closed = mount(withCtx(ctl(true, toggle)));
  const e = closed.container.querySelector<HTMLElement>('button[data-action="plan-collapse"]');
  expect(e?.getAttribute("aria-label")).toBe("Expand plan view");
  expect(e?.querySelector("svg.lucide-chevron-down")).not.toBeNull();
  expect(e?.querySelector("svg.lucide-chevron-up")).toBeNull();
  closed.unmount();
});

it("TopPaneHeader renders no plan controls without a context or with hasOverlayControls false", () => {
  const bare = mount(<TopPaneHeader>x</TopPaneHeader>);
  expect(bare.container.querySelector('[data-part="plan-controls"]')).toBeNull();
  bare.unmount();
  const off = mount(withCtx(ctl(false), <TopPaneHeader hasOverlayControls={false}>x</TopPaneHeader>));
  expect(off.container.querySelector('[data-part="plan-controls"]')).toBeNull();
  expect(off.container.querySelector("button")).toBeNull();
  off.unmount();
  const on = mount(withCtx(ctl(false), <TopPaneHeader>x</TopPaneHeader>));
  expect(on.container.querySelector('[data-part="plan-controls"]')).not.toBeNull();
  on.unmount();
});

it("TopPaneHeader is never position absolute and has no pr-12", () => {
  const m = mount(withCtx(ctl(false)));
  const h = header(m.container)!;
  for (const el of [h, ...Array.from(h.querySelectorAll("*"))]) {
    expect(el.classList.contains("absolute")).toBe(false);
    expect(el.classList.contains("pr-12")).toBe(false);
  }
  m.unmount();
  const n = mount(<TopPaneHeader>x</TopPaneHeader>);
  expect(header(n.container)!.classList.contains("pr-12")).toBe(false);
  n.unmount();
});

it("TopPaneHeader forwards headerRef to the header element", () => {
  const ref = createRef<HTMLElement>();
  const m = mount(<TopPaneHeader headerRef={ref}>x</TopPaneHeader>);
  expect(ref.current).not.toBeNull();
  expect(ref.current).toBe(header(m.container));
  m.unmount();
});
