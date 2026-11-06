// @vitest-environment jsdom
import { createRef } from "react";
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { TopPaneHeader } from "./TopPaneHeader.tsx";

const header = (c: HTMLElement) => c.querySelector<HTMLElement>("header");

it("TopPaneHeader renders a sticky h-11 header with a bottom border", () => {
  const m = mount(<TopPaneHeader>x</TopPaneHeader>);
  const h = header(m.container);
  expect(h).not.toBeNull();
  for (const c of ["sticky", "top-0", "z-10", "flex", "h-11", "shrink-0", "items-center", "border-b", "border-border", "bg-background", "px-4"]) {
    expect(h!.classList.contains(c)).toBe(true);
  }
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

it("TopPaneHeader reserves room for overlay controls unless told not to", () => {
  const d = mount(<TopPaneHeader>x</TopPaneHeader>);
  expect(header(d.container)?.classList.contains("pr-12")).toBe(true);
  d.unmount();
  const n = mount(<TopPaneHeader hasOverlayControls={false}>x</TopPaneHeader>);
  expect(header(n.container)?.classList.contains("pr-12")).toBe(false);
  n.unmount();
});

it("TopPaneHeader forwards headerRef to the header element", () => {
  const ref = createRef<HTMLElement>();
  const m = mount(<TopPaneHeader headerRef={ref}>x</TopPaneHeader>);
  expect(ref.current).not.toBeNull();
  expect(ref.current).toBe(header(m.container));
  m.unmount();
});
