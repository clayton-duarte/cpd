// @vitest-environment jsdom
import { expect, it } from "vitest";
import type { FoldItem, ToolItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { FoldRow } from "./Folds.tsx";
import { ToolGroup } from "./ToolGroup.tsx";
import { ToolRow } from "./ToolRow.tsx";

const fold: FoldItem = { kind: "fold", id: "f:1", foldKind: "cpd-context", title: "Ctx", body: "body", defaultOpen: false, at: "2026-01-02T03:04:05Z" };
const tool = (id: string): ToolItem => ({
  kind: "tool",
  id: `tool:${id}`,
  callId: id,
  name: "bash",
  args: { command: "ls" },
  status: "done",
  result: { text: "a\n", isError: false, durationMs: 1500 },
});

const has = (el: Element | null | undefined, tokens: string): void => {
  expect(el, `element for ${tokens}`).not.toBeNull();
  expect(el, `element for ${tokens}`).not.toBeUndefined();
  for (const t of tokens.split(" ")) expect(el?.classList.contains(t), `${t} in "${el?.className}"`).toBe(true);
};
const lacks = (el: Element | null | undefined, tokens: string): void => {
  for (const t of tokens.split(" ")) expect(el?.classList.contains(t), `${t} absent from "${el?.className}"`).toBe(false);
};
const q = (c: HTMLElement, sel: string): Element | null => c.querySelector(sel);
const DEFAULT = "text-sm font-sans";
const MUTED = "font-mono text-xs text-muted-foreground/70";

it("FoldRow renders the phi mark U+03C6 with the display classes inside the size-4 TraceIcon", () => {
  const m = mount(<FoldRow item={fold} />);
  const icon = q(m.container, "[data-slot=trace-line] > [data-slot=trace-icon]");
  has(icon, "size-4");
  const phi = q(m.container, "[data-part=phi]");
  expect(phi?.textContent).toBe("\u03c6");
  has(phi, "font-display font-bold text-[18px] leading-none -translate-y-[1.5px] select-none");
  expect(icon?.contains(phi) || icon === phi, "phi inside TraceIcon").toBe(true);
  m.unmount();
});

it("FoldRow source is text-sm font-sans and its marker text is font-mono text-xs muted", () => {
  const m = mount(<FoldRow item={fold} />);
  has(q(m.container, "[data-slot=trace-line]"), DEFAULT);
  lacks(q(m.container, "[data-slot=trace-line]"), "text-xs");
  has(q(m.container, "[data-part=source]"), DEFAULT);
  lacks(q(m.container, "[data-part=source]"), "font-mono");
  has(q(m.container, "[data-part=marker-text]"), MUTED);
  m.unmount();
});

it("FoldRow renders a middle dot before the time", () => {
  const m = mount(<FoldRow item={fold} />);
  const time = q(m.container, "[data-part=time]");
  expect(time, "time").not.toBeNull();
  const line = q(m.container, "[data-slot=trace-line]");
  const text = line?.textContent ?? "";
  const dot = text.indexOf("\u00b7");
  expect(dot, "middle dot present").toBeGreaterThanOrEqual(0);
  expect(dot, "dot before time").toBeLessThan(text.lastIndexOf(time?.textContent ?? "\u0000"));
  m.unmount();
});

it("ToolRow name is text-sm font-sans and its detail and summary are font-mono text-xs muted", () => {
  const m = mount(<ToolRow tool={tool("c1")} />);
  has(q(m.container, "[data-slot=trace-line]"), DEFAULT);
  lacks(q(m.container, "[data-slot=trace-line]"), "font-mono");
  has(q(m.container, "[data-part=name]"), DEFAULT);
  has(q(m.container, "[data-part=detail]"), MUTED);
  for (const s of m.container.querySelectorAll("[data-part=summary]")) has(s, MUTED);
  has(q(m.container, "[data-part=status]"), MUTED);
  m.unmount();
});

it("ToolGroup name is text-sm font-sans and its description is font-mono text-xs muted", () => {
  const m = mount(<ToolGroup tools={[tool("c1"), tool("c2")]} mode="grouped" />);
  const header = q(m.container, "[data-part=group-header]");
  has(header?.querySelector("[data-slot=trace-line]"), DEFAULT);
  has(q(m.container, "[data-part=name]"), DEFAULT);
  has(q(m.container, "[data-part=summary]"), MUTED);
  has(q(m.container, "[data-part=totals]"), MUTED);
  m.unmount();
});
