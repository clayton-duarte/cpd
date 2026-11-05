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

const stateOf = (el: Element): string | null =>
  (el.matches('[data-slot="trace-collapsible"]') ? el : el.querySelector('[data-slot="trace-collapsible"]'))?.getAttribute("data-state") ?? el.getAttribute("data-state");
/** A run of thoughts and tools is one collapsed chain: open it to reach its steps. */
const openChain = (c: HTMLElement): void => {
  const chain = c.querySelector('[data-kind="chain-of-thought"]') as HTMLElement;
  if (stateOf(chain) !== "open") click(chain.querySelector('[data-slot="trace-collapsible-trigger"]') as Element);
};

it("thinking expands on click inside the chain of thought", () => {
  const item: TranscriptItem = { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "thinking", text: "pondering deeply" }] };
  const m = mount(<Transcript {...base} items={[item]} />);
  openChain(m.container);
  const d = m.container.querySelector('[data-kind="thinking"]') as HTMLElement;
  expect(stateOf(d)).toBe("closed");
  click(d.querySelector('[data-slot="trace-collapsible-trigger"]') as Element);
  expect(stateOf(d)).toBe("open");
  expect(d.querySelector('[data-part="thinking-body"]')?.textContent).toBe("pondering deeply");
  m.unmount();
});

it("tool row renders a trace line with a status glyph", () => {
  const cases: ReadonlyArray<[ToolStatus, string]> = [
    ["running", "active"],
    ["done", "done"],
    ["failed", "blocked"],
  ];
  for (const [status, tone] of cases) {
    const m = mount(<Transcript {...base} items={[tool("t1", status)]} />);
    openChain(m.container);
    const row = m.container.querySelector('[data-kind="tool"]') as HTMLElement;
    const glyph = row.querySelector('[data-slot="trace-line"] [data-part="glyph"]');
    expect(glyph?.getAttribute("data-tone")).toBe(tone);
    expect(row.getAttribute("data-status")).toBe(status);
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

it("bash tool output renders in a code block", () => {
  const bash = tool("b1", "done", { name: "bash", args: { command: "ls -la /tmp" }, result: { text: "total 8\nfile-a", isError: false } });
  const m = mount(<Transcript {...base} items={[bash]} />);
  openChain(m.container);
  const row = m.container.querySelector('[data-kind="tool"][data-tool="bash"]') as HTMLElement;
  expect(row.querySelector('[data-slot="trace-line"] [data-part="detail"]')?.textContent).toBe("ls -la /tmp");
  click(row.querySelector('[data-slot="trace-collapsible-trigger"]') as Element);
  expect(row.querySelector('[data-slot="code-block-content"][data-part="output"]')?.textContent).toBe("total 8\nfile-a");
  m.unmount();
});

it("edit tool renders colored diff lines", () => {
  const edit = tool("e1", "done", { name: "edit", args: { path: "/a.ts" }, diff: "@@ -1 +1 @@\n-old\n+new" });
  const m = mount(<Transcript {...base} items={[edit]} />);
  openChain(m.container);
  click(m.container.querySelector('[data-tool="edit"] [data-slot="trace-collapsible-trigger"]') as Element);
  const diff = m.container.querySelector('[data-tool="edit"] [data-slot="code-block"][data-part="diff"]');
  expect(diff).not.toBeNull();
  const add = diff?.querySelector('[data-diff="add"]');
  const rem = diff?.querySelector('[data-diff="remove"]');
  expect(add?.textContent).toBe("new");
  expect(add?.getAttribute("data-variant")).toBe("added");
  expect(rem?.textContent).toBe("old");
  expect(rem?.getAttribute("data-variant")).toBe("removed");
  expect(diff?.querySelector('[data-diff="context"]')?.textContent).toBe("@@ -1 +1 @@");
  expect(m.container.querySelector('[data-part="delta"] [data-part="added"]')?.textContent).toBe("+1");
  m.unmount();
});

it("tool group collapses and expands", () => {
  const tools = [tool("g1", "done"), tool("g2", "failed"), tool("g3", "done")];
  const m = mount(<ToolGroup tools={tools} mode="grouped" />);
  const d = m.container.querySelector('[data-kind="tool-group"]') as HTMLElement;
  expect(d.getAttribute("data-tool-count")).toBe("3");
  expect(d.getAttribute("data-status")).toBe("failed");
  expect(stateOf(d)).toBe("closed");
  const header = d.querySelector('[data-part="group-header"]') as HTMLElement;
  expect(header.querySelector('[data-part="group-count"]')?.textContent).toBe("3 tool calls");
  click(header);
  expect(stateOf(d)).toBe("open");
  expect(d.querySelectorAll('[data-kind="tool"]').length).toBe(3);
  click(header);
  expect(stateOf(d)).toBe("closed");
  m.unmount();
});
