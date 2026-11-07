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
  expect(part(failed.container, "status")).toBe("Failed");
  failed.unmount();
  const running = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "running", result: undefined })} />);
  expect(part(running.container, "status")).toBe("Running");
  running.unmount();
});

it("a tool row is a trace collapsible with one line of name, detail and status", () => {
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command: "echo hi" } })} />);
  const row = rowOf(m.container);
  expect(row.getAttribute("data-slot")).toBe("trace-collapsible");
  expect(row.getAttribute("data-state")).toBe("closed");
  const line = row.querySelector('[data-slot="trace-line"]') as HTMLElement;
  expect(line).not.toBeNull();
  expect(line.querySelector('[data-part="name"]')?.textContent).toBe("bash");
  expect(line.querySelector('[data-part="detail"]')?.textContent).toBe("echo hi");
  expect(line.querySelector('[data-part="status"]')?.textContent).toBe("Done in 1.5s (3 lines)");
  expect(m.container.querySelector('[data-part="rail"], [data-part="summary"]')).toBeNull();
  m.unmount();
});

it("a row with no output is a plain div without a trigger", () => {
  const m = mount(<ToolRow tool={tool({ name: "write", args: { path: "w.ts" }, result: undefined, status: "running" })} />);
  const row = rowOf(m.container);
  expect(row.tagName).toBe("DIV");
  expect(trigger(row)).toBeNull();
  expect(row.querySelector('[data-part="detail"]')?.textContent).toBe("w.ts");
  m.unmount();
});

it("a long path or command is cut at 100 characters and other details at 160", () => {
  const long = `/very/long/${"segment/".repeat(40)}file.ts`;
  const bash = mount(<ToolRow tool={tool({ name: "bash", args: { command: `grep ${"x".repeat(300)}\nsecond` } })} />);
  expect(part(bash.container, "detail")).toBe(`grep ${"x".repeat(95)}…`);
  bash.unmount();
  const read = mount(<ToolRow tool={tool({ name: "read", args: { path: long } })} />);
  expect(part(read.container, "detail")).toBe(`${long.slice(0, 100)}…`);
  read.unmount();
  const other = mount(<ToolRow tool={tool({ name: "codemode", args: { query: "q".repeat(400) } })} />);
  expect(part(other.container, "detail")).toBe(`${"q".repeat(160)}…`);
  other.unmount();
  const exact = mount(<ToolRow tool={tool({ name: "read", args: { path: "p".repeat(100) } })} />);
  expect(part(exact.container, "detail")).toBe("p".repeat(100));
  exact.unmount();
  const failed = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "failed", result: { text: "r".repeat(300), isError: true } })} />);
  expect(part(failed.container, "status")).toBe("Failed");
  failed.unmount();
});

it("tool output previews 10 lines and the expander shows every line", () => {
  const text = Array.from({ length: 15 }, (_, i) => `line${i + 1}`).join("\n");
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command: "seq" }, result: { text, isError: false } })} />);
  openRow(rowOf(m.container));
  const shown = () => m.container.querySelector('[data-slot="code-block-content"][data-part="output"]')?.textContent;
  expect(shown()).toBe(Array.from({ length: 10 }, (_, i) => `line${i + 1}`).join("\n"));
  const btn = m.container.querySelector('[data-slot="code-block-footer"] button[data-action="expand-output"]') as HTMLButtonElement;
  expect(btn.textContent).toBe("Show 5 more lines");
  click(btn);
  expect(shown()).toBe(text);
  expect(m.container.querySelector('button[data-action="expand-output"]')).toBeNull();
  m.unmount();
});

it("a failed tool shows Failed without the reason", () => {
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "failed", result: { text: "\nboom\nmore", isError: true } })} />);
  // 15.1: the reason text goes to the toast, never inline in the row's status.
  expect(part(m.container, "status")).toBe("Failed");
  m.unmount();
});

it("a failed tool shows its output in a destructive code block", () => {
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command: "x" }, status: "failed", result: { text: "boom", isError: true } })} />);
  openRow(rowOf(m.container));
  expect(m.container.querySelector('[data-slot="code-block"]')?.getAttribute("data-variant")).toBe("destructive");
  m.unmount();
});

it("an edit diff is collapsed by default, shows ten lines and keeps whitespace", () => {
  const diff = "@@ -1,9 +1,9 @@\n c1\n c2\n c3\n c4\n c5\n-    old   spaced\n+    new   spaced\n c6\n c7\n c8\n c9";
  const m = mount(<ToolRow tool={tool({ name: "edit", args: { path: "f" }, diff })} />);
  const row = rowOf(m.container);
  expect(row.getAttribute("data-state")).toBe("closed");
  expect(m.container.querySelector("[data-diff]")).toBeNull();
  expect(part(m.container, "status")).toBe("Done in 1.5s (+1 -1)");
  click(trigger(row));
  expect(row.getAttribute("data-state")).toBe("open");
  const box = m.container.querySelector('[data-slot="code-block"][data-part="diff"]') as HTMLElement;
  expect(box).not.toBeNull();
  const lines = [...box.querySelectorAll('[data-slot="code-block-line"][data-diff]')];
  expect(lines.length).toBe(10);
  expect(box.querySelector('[data-diff="remove"]')?.textContent).toBe("    old   spaced");
  expect(box.querySelector('[data-diff="add"]')?.textContent).toBe("    new   spaced");
  expect(box.querySelector('button[data-action="expand-output"]')?.textContent).toBe("Show 2 more lines");
  m.unmount();
});

it("a tool group in an expanded chain opens its group but not its rows", () => {
  const t1 = tool({ name: "read", args: { path: "a" }, callId: "c1", id: "tool:c1" });
  const t2 = tool({ name: "read", args: { path: "b" }, callId: "c2", id: "tool:c2" });
  const t3 = tool({ name: "read", args: { path: "c" }, callId: "c3", id: "tool:c3" });
  const m = mount(<Transcript {...props} tools="expanded" items={[t1, t2, t3]} />);
  const group = m.container.querySelector('[data-kind="tool-group"]') as HTMLElement;
  expect(group).not.toBeNull();
  expect(group.getAttribute("data-tool-count")).toBe("3");
  expect(group.querySelector('[data-part="group-count"]')?.textContent).toBe("3 tool calls");
  const rows = [...group.querySelectorAll<HTMLElement>('[data-kind="tool"]')];
  expect(rows.length).toBe(3);
  for (const r of rows) expect(r.getAttribute("data-state")).toBe("closed");
  m.unmount();
  const grouped = mount(<Transcript {...props} items={[t1, t2, t3]} />);
  expect(grouped.container.querySelector('[data-kind="tool"]'), "a collapsed chain mounts no tool rows").toBeNull();
  grouped.unmount();
});
