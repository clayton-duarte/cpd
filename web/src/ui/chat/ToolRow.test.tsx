// @vitest-environment jsdom
import { act } from "react";
import { expect, it } from "vitest";
import type { ToolItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { ToolRow } from "./ToolRow.tsx";
import { Transcript } from "./Transcript.tsx";

const tool = (over: Partial<ToolItem> & Pick<ToolItem, "name" | "args">): ToolItem => ({
  kind: "tool",
  id: `tool:${over.callId ?? "c1"}`,
  callId: "c1",
  status: "done",
  result: { text: "a\nb\nc\n", isError: false, durationMs: 1500 },
  ...over,
});
const part = (c: HTMLElement, p: string) => c.querySelector(`[data-kind="tool"] [data-part="${p}"]`)?.textContent;
const click = (el: Element | null): void => {
  act(() => (el as HTMLElement).click());
};
const rowOf = (c: HTMLElement, i = 0): HTMLElement => c.querySelectorAll<HTMLElement>('[data-kind="tool"]')[i] as HTMLElement;
const trigger = (row: HTMLElement): Element | null => row.querySelector('[data-slot="trace-collapsible-trigger"]');
const openRow = (row: HTMLElement): void => {
  if (row.getAttribute("data-state") !== "open") click(trigger(row));
};
const props = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const CLIP = /(^|\s)(truncate|text-ellipsis|whitespace-nowrap|overflow-hidden|h-\[\d+px\]|max-h-\S+)(\s|$)/;
const clipped = (root: Element): Element[] => [root, ...root.querySelectorAll("*")].filter((e) => CLIP.test(e.getAttribute("class") ?? ""));

it("compact tool rows show the command or path and a Done line count", () => {
  const bash = mount(<ToolRow tool={tool({ name: "bash", args: { command: "ls -la\nsecond line" } })} />);
  expect(part(bash.container, "name")).toBe("bash");
  expect(part(bash.container, "detail")).toBe("ls -la");
  expect(part(bash.container, "status")).toBe("Done in 1.5s (3 lines)");
  bash.unmount();
  const read = mount(<ToolRow tool={tool({ name: "read", args: { path: "src/a.ts" }, result: { text: "only\n", isError: false } })} />);
  expect(part(read.container, "detail")).toBe("src/a.ts");
  expect(part(read.container, "status")).toBe("Done (1 line)");
  read.unmount();
});

it("bash read edit write codemode and mcp tools each render a row", () => {
  const cases: [ToolItem, string, string][] = [
    [tool({ name: "bash", args: { command: "echo hi" } }), "bash", "echo hi"],
    [tool({ name: "read", args: { path: "r.ts" } }), "read", "r.ts"],
    [tool({ name: "edit", args: { path: "e.ts" }, diff: "+x\n-y" }), "edit", "e.ts"],
    [tool({ name: "write", args: { path: "w.ts" } }), "write", "w.ts"],
    [tool({ name: "codemode", args: { query: "find   things" } }), "codemode", "find things"],
    [tool({ name: "mcp__srv__tool", args: { url: "http://x" } }), "srv__tool", "http://x"],
  ];
  for (const [t, name, detail] of cases) {
    const m = mount(<ToolRow tool={t} />);
    const d = rowOf(m.container);
    expect(d.getAttribute("data-tool")).toBe(t.name);
    expect(d.getAttribute("data-status")).toBe("done");
    expect(d.getAttribute("data-state")).toBe("closed");
    expect(part(m.container, "name")).toBe(name);
    expect(part(m.container, "detail")).toBe(detail);
    m.unmount();
  }
  const failed = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "failed", result: { text: "\nboom\nmore", isError: true } })} />);
  expect(part(failed.container, "status")).toBe("Failed: boom");
  failed.unmount();
  const running = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "running", result: undefined })} />);
  expect(part(running.container, "status")).toBe("Running");
  running.unmount();
});

it("a tool call renders one rail with an unclamped wrapping summary", () => {
  const command = `echo ${"word ".repeat(60)}end`;
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command } })} />);
  const row = rowOf(m.container);
  expect(row.getAttribute("data-slot")).toBe("trace");
  openRow(row);
  expect(m.container.querySelectorAll('[data-part="rail"]').length).toBe(1);
  expect(m.container.querySelector('[data-part="tool-rail"]')).toBeNull();
  const head = row.querySelector('[data-part="summary"]') as HTMLElement;
  expect(head).not.toBeNull();
  expect(clipped(head)).toEqual([]);
  expect(head.querySelector('[data-part="detail"]')?.textContent).toBe(command);
  expect(head.querySelector('[data-part="detail"]')?.className).toMatch(/break-words|wrap-break-word|whitespace-pre-wrap|whitespace-normal/);
  m.unmount();
});

it("a long command or path is never truncated", () => {
  const long = `/very/long/${"segment/".repeat(40)}file.ts`;
  const bash = mount(<ToolRow tool={tool({ name: "bash", args: { command: `grep ${"x".repeat(300)}\nsecond` } })} />);
  expect(part(bash.container, "detail")).toBe(`grep ${"x".repeat(300)}`);
  bash.unmount();
  const read = mount(<ToolRow tool={tool({ name: "read", args: { path: long } })} />);
  expect(part(read.container, "detail")).toBe(long);
  read.unmount();
  const other = mount(<ToolRow tool={tool({ name: "codemode", args: { query: "q".repeat(400) } })} />);
  expect(part(other.container, "detail")).toBe("q".repeat(400));
  other.unmount();
  const reason = "r".repeat(300);
  const failed = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "failed", result: { text: reason, isError: true } })} />);
  expect(part(failed.container, "status")).toBe(`Failed: ${reason}`);
  failed.unmount();
});

it("tool output previews 10 rows and the expander shows every row", () => {
  const text = Array.from({ length: 15 }, (_, i) => `line${i + 1}`).join("\n");
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command: "seq" }, result: { text, isError: false } })} />);
  openRow(rowOf(m.container));
  const rows = () => [...m.container.querySelectorAll('[data-part="output"] [data-part="output-row"]')].map((e) => e.textContent);
  expect(rows()).toEqual(Array.from({ length: 10 }, (_, i) => `line${i + 1}`));
  const btn = m.container.querySelector('button[data-action="expand-output"]') as HTMLButtonElement;
  expect(btn.textContent).toBe("Show 5 more lines");
  click(btn);
  expect(rows()).toEqual(Array.from({ length: 15 }, (_, i) => `line${i + 1}`));
  expect(m.container.querySelector('button[data-action="expand-output"]')).toBeNull();
  expect(clipped(m.container.querySelector('[data-part="output"]') as Element)).toEqual([]);
  m.unmount();
});

it("an edit diff is collapsed by default and keeps whitespace without clipping", () => {
  const diff = "@@ -1,9 +1,9 @@\n c1\n c2\n c3\n c4\n c5\n-    old   spaced\n+    new   spaced\n c6\n c7\n c8\n c9";
  const m = mount(<ToolRow tool={tool({ name: "edit", args: { path: "f" }, diff })} />);
  const row = rowOf(m.container);
  expect(row.getAttribute("data-state")).toBe("closed");
  expect(m.container.querySelector("[data-diff]")).toBeNull();
  expect(part(m.container, "status")).toBe("Done in 1.5s (+1 -1)");
  click(trigger(row));
  expect(row.getAttribute("data-state")).toBe("open");
  const box = m.container.querySelector('[data-part="diff"]') as HTMLElement;
  expect(box).not.toBeNull();
  const rows = [...box.querySelectorAll("[data-diff]")];
  expect(rows.filter((e) => e.getAttribute("data-diff") === "context" && !(e.textContent ?? "").startsWith("@@")).map((e) => e.textContent)).toEqual([" c5", " c6"]);
  expect(box.querySelector('[data-diff="remove"]')?.textContent).toBe("-    old   spaced");
  expect(box.querySelector('[data-diff="add"]')?.textContent).toBe("+    new   spaced");
  for (const e of rows) {
    const tokens = (e.getAttribute("class") ?? "").split(/\s+/);
    expect(tokens).toContain("whitespace-pre-wrap");
    expect(tokens).not.toContain("whitespace-pre");
  }
  expect(clipped(box)).toEqual([]);
  m.unmount();
});

it("a tool group has no second rail", () => {
  const t1 = tool({ name: "read", args: { path: "a" }, callId: "c1", id: "tool:c1" });
  const t2 = tool({ name: "read", args: { path: "b" }, callId: "c2", id: "tool:c2" });
  const t3 = tool({ name: "read", args: { path: "c" }, callId: "c3", id: "tool:c3" });
  const m = mount(<Transcript {...props} tools="expanded" items={[t1, t2, t3]} />);
  const group = m.container.querySelector('[data-kind="tool-group"]') as HTMLElement;
  expect(group).not.toBeNull();
  expect(group.getAttribute("data-tool-count")).toBe("3");
  expect(group.querySelector('[data-part="group-count"]')?.textContent).toBe("3 tool calls");
  expect(group.className).not.toContain("shadow-[inset");
  expect(group.getAttribute("data-part")).not.toBe("rail");
  expect(m.container.querySelectorAll('[data-part="rail"]').length).toBeGreaterThan(0);
  expect(m.container.querySelectorAll('[data-part="rail"] [data-part="rail"]').length).toBe(0);
  expect(m.container.querySelector('[data-part="tool-rail"]')).toBeNull();
  expect(group.querySelectorAll('[data-kind="tool"]').length).toBe(3);
  const single = mount(<Transcript {...props} items={[t1]} />);
  expect(single.container.querySelector('[data-kind="tool-group"]')).toBeNull();
  expect(single.container.querySelectorAll('[data-kind="tool"]').length).toBe(1);
  single.unmount();
  m.unmount();
});

it("Tools expanded expands the tool rows", () => {
  const mk = (id: string) => tool({ name: "bash", args: { command: id }, callId: id, id: `tool:${id}`, result: { text: `out-${id}`, isError: false } });
  const items = [mk("c1"), mk("c2"), mk("c3")];
  const m = mount(<Transcript {...props} tools="expanded" items={items} />);
  const rows = [...m.container.querySelectorAll<HTMLElement>('[data-kind="tool"]')];
  expect(rows.length).toBe(3);
  for (const r of rows) expect(r.getAttribute("data-state")).toBe("open");
  expect([...m.container.querySelectorAll('[data-part="output"]')].map((e) => e.textContent)).toEqual(["out-c1", "out-c2", "out-c3"]);
  m.unmount();
  const grouped = mount(<Transcript {...props} items={items} />);
  for (const r of grouped.container.querySelectorAll('[data-kind="tool"]')) expect(r.getAttribute("data-state")).toBe("closed");
  grouped.unmount();
  const single = mount(<Transcript {...props} tools="expanded" items={[mk("c1")]} />);
  expect(rowOf(single.container).getAttribute("data-state")).toBe("open");
  single.unmount();
});
