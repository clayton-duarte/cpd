// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { CpdFlowCanvas } from "./FlowCanvas.tsx";
import { FLOW_STEPS, forwardEdgeIds, loopEdgeIds } from "./flow-steps.ts";

let observers: { cb: () => void; targets: Element[] }[] = [];
let scale = 1;
const protoKeys = ["offsetTop", "offsetHeight", "offsetLeft", "offsetWidth"] as const;
const saved = new Map<string, PropertyDescriptor | undefined>();

beforeEach(() => {
  observers = [];
  scale = 1;
  vi.stubGlobal("ResizeObserver", class {
    rec: { cb: () => void; targets: Element[] };
    constructor(cb: () => void) { this.rec = { cb, targets: [] }; observers.push(this.rec); }
    observe(el: Element): void { this.rec.targets.push(el); }
    unobserve(): void {}
    disconnect(): void {}
  });
  for (const k of protoKeys) saved.set(k, Object.getOwnPropertyDescriptor(HTMLElement.prototype, k));
  const index = (el: HTMLElement) => Number(el.closest("li")?.getAttribute("data-step") ?? "0");
  Object.defineProperty(HTMLElement.prototype, "offsetTop", { configurable: true, get(this: HTMLElement) { return index(this) * 50 * scale; } });
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get() { return 40; } });
  Object.defineProperty(HTMLElement.prototype, "offsetLeft", { configurable: true, get() { return 0; } });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get() { return 200; } });
});
afterEach(() => {
  for (const k of protoKeys) {
    const d = saved.get(k);
    if (d) Object.defineProperty(HTMLElement.prototype, k, d);
    else delete (HTMLElement.prototype as unknown as Record<string, unknown>)[k];
  }
  vi.unstubAllGlobals();
});

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });
const show = () => { const m = mount(<CpdFlowCanvas />); mounted = m; return m.container; };
const click = (el: Element | null) => {
  if (el === null) throw new Error("element not found");
  act(() => { el.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
};
const card = (c: HTMLElement, id: string) => c.querySelector(`li[data-step="${id}"]`) as HTMLElement;
const text = (el: Element | null) => el?.textContent ?? null;

const palette: Record<string, string[]> = {
  context: ["border-sakura/50", "bg-sakura/12", "text-sakura"],
  manager: ["border-sakura-bright/60", "bg-sakura-bright/15", "text-sakura-bright"],
  plan: ["border-shiro/50", "bg-shiro/12", "text-shiro"],
  lead: ["border-shiro-bright/60", "bg-shiro-bright/15", "text-shiro-bright"],
  dispatch: ["border-matcha/50", "bg-matcha/12", "text-matcha"],
  worker: ["border-matcha-bright/60", "bg-matcha-bright/15", "text-matcha-bright"],
};

it("canvas renders 20 step cards numbered 01 to 20 in order", () => {
  const c = show();
  expect(c.querySelector('[data-part="flow-canvas"]')).not.toBeNull();
  const cards = [...c.querySelectorAll("li[data-step]")];
  expect(cards.map((l) => l.getAttribute("data-step"))).toEqual(FLOW_STEPS.map((s) => s.id));
  expect(cards).toHaveLength(20);
  for (const l of cards) {
    expect(l.className).toContain("rounded-[6px]");
    expect(l.className).toContain("border");
    expect(l.className).toContain("bg-card");
    const t = l.querySelector('[data-part="step-toggle"]');
    expect(t?.getAttribute("aria-label")).toBe(`Toggle step ${l.getAttribute("data-step")}`);
    expect(text(t)).toContain(l.getAttribute("data-step"));
  }
});

it("each card shows its name and an artifact badge only when the step has one", () => {
  const c = show();
  for (const s of FLOW_STEPS) {
    const l = card(c, s.id);
    expect(text(l.querySelector('[data-part="step-toggle"]'))).toContain(s.name);
    const a = l.querySelector('[data-part="artifact"]');
    if (s.artifact === null) expect(a).toBeNull();
    else expect(text(a)).toBe(s.artifact);
  }
});

it("phase and agent badges use the pinned Dango palette classes", () => {
  const c = show();
  for (const s of FLOW_STEPS) {
    const l = card(c, s.id);
    const p = l.querySelector('[data-part="phase"]');
    const a = l.querySelector('[data-part="agent"]');
    expect(text(p)).toBe(s.phase);
    expect(text(a)).toBe(s.agent);
    for (const k of palette[s.phase] ?? ["missing"]) expect(p?.className).toContain(k);
    for (const k of palette[s.agent] ?? ["missing"]) expect(a?.className).toContain(k);
  }
});

it("loop badge reads toStep · label on the 8 loop steps only", () => {
  const c = show();
  expect(c.querySelectorAll('[data-part="loop"]')).toHaveLength(8);
  for (const s of FLOW_STEPS) {
    const b = card(c, s.id).querySelector('[data-part="loop"]');
    if (s.loop === null) expect(b).toBeNull();
    else expect(text(b)).toBe(`${s.loop.to} · ${s.loop.label}`);
  }
});

it("description and loop condition stay collapsed until the card is toggled", () => {
  const c = show();
  expect(c.querySelector('[data-part="step-description"]')).toBeNull();
  const loopStep = FLOW_STEPS.find((s) => s.loop !== null)!;
  const l = card(c, loopStep.id);
  click(l.querySelector('[data-part="step-toggle"]'));
  expect(text(l.querySelector('[data-part="step-description"]'))).toBe(loopStep.description);
  expect(text(l.querySelector('[data-part="loop-condition"]'))).toContain(loopStep.loop!.condition);
  click(l.querySelector('[data-part="step-toggle"]'));
  expect(l.querySelector('[data-part="step-description"]')).toBeNull();
  const plain = FLOW_STEPS.find((s) => s.loop === null)!;
  const pl = card(c, plain.id);
  click(pl.querySelector('[data-part="step-toggle"]'));
  expect(text(pl.querySelector('[data-part="step-description"]'))).toBe(plain.description);
  expect(pl.querySelector('[data-part="loop-condition"]')).toBeNull();
});

it("Expand all and Collapse all toggle every card", () => {
  const c = show();
  const btn = () => c.querySelector('[data-part="expand-all"]');
  expect(text(btn())).toBe("Expand all");
  click(btn());
  expect(c.querySelectorAll('[data-part="step-description"]')).toHaveLength(20);
  expect(text(btn())).toBe("Collapse all");
  click(btn());
  expect(c.querySelectorAll('[data-part="step-description"]')).toHaveLength(0);
  expect(text(btn())).toBe("Expand all");
  click(card(c, "01").querySelector('[data-part="step-toggle"]'));
  expect(text(btn())).toBe("Expand all");
});

const paths = (c: HTMLElement) => [...c.querySelectorAll('svg[data-part="dag-edges"] path[data-edge]')];

it("svg dag-edges holds 19 forward and 8 loop edges with the pinned ids", () => {
  const c = show();
  const ids = paths(c).map((p) => p.getAttribute("data-edge"));
  expect(ids).toEqual([...forwardEdgeIds(), ...loopEdgeIds()]);
  expect(forwardEdgeIds()).toHaveLength(19);
  expect(loopEdgeIds()).toHaveLength(8);
  expect(ids[0]).toBe("01>02");
  expect(ids).toContain("loop:02>02");
});

it("loop edges are dashed and forward edges carry an arrowhead", () => {
  const c = show();
  const fwd = new Set(forwardEdgeIds());
  expect(paths(c).length).toBeGreaterThan(0);
  for (const p of paths(c)) {
    if (fwd.has(p.getAttribute("data-edge") ?? "")) {
      expect(p.hasAttribute("stroke-dasharray")).toBe(false);
      const m = p.getAttribute("marker-end") ?? "";
      expect(m.startsWith("url(#")).toBe(true);
      const id = m.slice(5, -1);
      expect(c.querySelector(`svg[data-part="dag-edges"] marker[id="${id}"]`)).not.toBeNull();
    } else {
      expect(p.hasAttribute("stroke-dasharray")).toBe(true);
    }
  }
});

it("a ResizeObserver callback re-measures and redraws the edges", () => {
  const c = show();
  const first = paths(c).find((p) => p.getAttribute("data-edge") === "01>02")?.getAttribute("d");
  expect(first).toBe("M 100 40 L 100 100");
  expect(observers.length).toBeGreaterThan(0);
  scale = 2;
  act(() => { for (const o of observers) o.cb(); });
  const second = paths(c).find((p) => p.getAttribute("data-edge") === "01>02")?.getAttribute("d");
  expect(second).toBe("M 100 40 L 100 200");
});
