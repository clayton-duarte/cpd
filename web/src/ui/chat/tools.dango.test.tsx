// @vitest-environment jsdom
import { act } from "react";
import { expect, it } from "vitest";
import type { ToolItem, ToolStatus, TranscriptItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { ToolGroup } from "./ToolGroup.tsx";
import { ToolTickStrip } from "./ToolTickStrip.tsx";
import { Transcript } from "./Transcript.tsx";

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };

const tool = (id: string, status: ToolStatus, over: Partial<ToolItem> = {}): ToolItem => ({
  kind: "tool",
  id: `tool:${id}`,
  callId: id,
  name: "read",
  args: { path: `/src/${id}.ts` },
  status,
  ...(status === "running" ? {} : { result: { text: "line one\nline two", isError: status === "failed", durationMs: 1500 } }),
  ...over,
});

const click = (el: Element): void => {
  act(() => (el as HTMLElement).click());
};

it("thinking renders a folded pill and expands on click", () => {
  const item: TranscriptItem = { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "thinking", text: "pondering deeply" }] };
  const m = mount(<Transcript {...base} items={[item]} />);
  const d = m.container.querySelector('details[data-kind="thinking"]') as HTMLDetailsElement;
  expect(d.open).toBe(false);
  expect(d.querySelector('summary [data-part="label"]')?.textContent).toBe("Thinking");
  expect(d.querySelector('summary [data-part="caret"]')?.getAttribute("aria-hidden")).toBe("true");
  click(d.querySelector("summary") as Element);
  expect(d.open).toBe(true);
  expect(d.querySelector('[data-part="thinking-body"]')?.textContent).toBe("pondering deeply");
  m.unmount();
});

it("tool row renders a left rail and status glyph", () => {
  const cases: ReadonlyArray<[ToolStatus, string, string]> = [
    ["running", "glyph-active", "active"],
    ["done", "glyph-done", "done"],
    ["failed", "glyph-blocked", "blocked"],
  ];
  for (const [status, cls, tone] of cases) {
    const m = mount(<Transcript {...base} items={[tool("t1", status)]} />);
    const rail = m.container.querySelector('[data-part="tool-rail"]');
    expect(rail).not.toBeNull();
    const glyph = rail?.querySelector('details[data-kind="tool"] > summary [data-part="glyph"]');
    expect(glyph?.getAttribute("aria-hidden")).toBe("true");
    expect(glyph?.classList.contains(cls)).toBe(true);
    expect(glyph?.getAttribute("data-tone")).toBe(tone);
    m.unmount();
  }
});

it("tool group renders a tick strip with one tick per call", () => {
  const statuses: ToolStatus[] = ["done", "failed", "running", "done"];
  const m = mount(<ToolTickStrip statuses={statuses} />);
  const strip = m.container.querySelector('[data-part="tick-strip"]');
  expect(strip?.getAttribute("role")).toBe("img");
  expect(strip?.getAttribute("aria-label")).toBe("4 steps, 1 failed");
  const ticks = Array.from(strip?.querySelectorAll('[data-part="tick"]') ?? []);
  expect(ticks.map((t) => t.getAttribute("data-status"))).toEqual(statuses);
  m.unmount();
  const ok = mount(<ToolTickStrip statuses={["done", "done"]} />);
  expect(ok.container.querySelector('[data-part="tick-strip"]')?.getAttribute("aria-label")).toBe("2 steps");
  ok.unmount();
});

it("bash tool output renders inline", () => {
  const bash = tool("b1", "done", { name: "bash", args: { command: "ls -la /tmp" }, result: { text: "total 8\nfile-a", isError: false } });
  const m = mount(<Transcript {...base} items={[bash]} />);
  const row = m.container.querySelector('[data-part="tool-rail"] details[data-kind="tool"][data-tool="bash"]');
  expect(row).not.toBeNull();
  expect(row?.querySelector('summary [data-part="detail"]')?.textContent).toBe("ls -la /tmp");
  const out = row?.querySelector('pre[data-part="output"]');
  expect(out?.textContent).toBe("total 8\nfile-a");
  m.unmount();
});

it("edit tool renders colored diff lines", () => {
  const edit = tool("e1", "done", { name: "edit", args: { path: "/a.ts" }, diff: "@@ -1 +1 @@\n-old\n+new" });
  const m = mount(<Transcript {...base} items={[edit]} />);
  const diff = m.container.querySelector('details[data-tool="edit"] [data-part="diff"]');
  expect(diff).not.toBeNull();
  const add = diff?.querySelector('[data-diff="add"]');
  const rem = diff?.querySelector('[data-diff="remove"]');
  expect(add?.textContent).toBe("+new");
  expect(add?.querySelector('[data-part="sign"]')?.textContent).toBe("+");
  expect(rem?.textContent).toBe("-old");
  expect(rem?.querySelector('[data-part="sign"]')?.textContent).toBe("-");
  expect(diff?.querySelector('[data-diff="context"]')?.textContent).toBe("@@ -1 +1 @@");
  expect(m.container.querySelector('[data-part="delta"] [data-part="added"]')?.textContent).toBe("+1");
  m.unmount();
});

it("tool group collapses and expands", () => {
  const tools = [tool("g1", "done"), tool("g2", "failed"), tool("g3", "done")];
  const m = mount(<ToolGroup tools={tools} mode="grouped" />);
  const d = m.container.querySelector('details[data-kind="tool-group"]') as HTMLDetailsElement;
  expect(d.getAttribute("data-tool-count")).toBe("3");
  expect(d.getAttribute("data-status")).toBe("failed");
  expect(d.open).toBe(false);
  const header = d.querySelector('summary[data-part="group-header"]') as HTMLElement;
  expect(header.querySelector('[data-part="group-count"]')?.textContent).toBe("3 tool calls");
  expect(d.querySelectorAll('[data-part="group-rows"] details[data-kind="tool"]').length).toBe(3);
  click(header);
  expect(d.open).toBe(true);
  click(header);
  expect(d.open).toBe(false);
  m.unmount();
});
