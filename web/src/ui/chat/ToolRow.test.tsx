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
const part = (c: HTMLElement, p: string) => c.querySelector(`summary [data-part="${p}"]`)?.textContent;

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
    const d = m.container.querySelector('details[data-kind="tool"]') as HTMLDetailsElement;
    expect(d.getAttribute("data-tool")).toBe(t.name);
    expect(d.getAttribute("data-status")).toBe("done");
    expect(d.open).toBe(t.name === "edit");
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

it("output is truncated to 10 lines with an expander", () => {
  const text = Array.from({ length: 15 }, (_, i) => `line${i + 1}`).join("\n");
  const m = mount(<ToolRow tool={tool({ name: "bash", args: { command: "seq" }, result: { text, isError: false } })} />);
  const out = () => m.container.querySelector('pre[data-part="output"]')?.textContent ?? "";
  expect(out()).toContain("line10");
  expect(out()).not.toContain("line11");
  const btn = m.container.querySelector('button[data-action="expand-output"]') as HTMLButtonElement;
  expect(btn.textContent).toBe("Show 5 more lines");
  act(() => btn.click());
  expect(out()).toContain("line15");
  expect(m.container.querySelector('button[data-action="expand-output"]')).toBeNull();
  m.unmount();
});

it("edit tool diffs render added and removed lines", () => {
  const diff = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n same";
  const m = mount(<ToolRow tool={tool({ name: "edit", args: { path: "f" }, diff })} />);
  const kinds = [...m.container.querySelectorAll("[data-diff]")].map((e) => [e.getAttribute("data-diff"), e.textContent]);
  expect(kinds).toEqual([
    ["context", "--- a/f"],
    ["context", "+++ b/f"],
    ["context", "@@ -1 +1 @@"],
    ["remove", "-old"],
    ["add", "+new"],
    ["context", " same"],
  ]);
  expect(part(m.container, "status")).toBe("Done in 1.5s (+1 -1)");
  m.unmount();
});

it("consecutive tool items are grouped under one summary row", () => {
  const t1 = tool({ name: "read", args: { path: "a" }, callId: "c1" });
  const t2 = tool({ name: "read", args: { path: "b" }, callId: "c2" });
  const t3 = tool({ name: "read", args: { path: "c" }, callId: "c3" });
  const props = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
  const m = mount(<Transcript {...props} items={[t1, t2, t3]} />);
  const g = m.container.querySelectorAll('details[data-kind="tool-group"]');
  expect(g.length).toBe(1);
  const group = g[0] as HTMLDetailsElement;
  expect(group.open).toBe(false);
  expect(group.querySelector('[data-part="group-count"]')?.textContent).toBe("3 tool calls");
  expect(group.querySelectorAll('details[data-kind="tool"]').length).toBe(3);
  m.unmount();
  const open = mount(<Transcript {...props} tools="expanded" items={[t1, t2, t3]} />);
  expect((open.container.querySelector('details[data-kind="tool-group"]') as HTMLDetailsElement).open).toBe(true);
  open.unmount();
  const single = mount(<Transcript {...props} items={[t1]} />);
  expect(single.container.querySelector('[data-kind="tool-group"]')).toBeNull();
  expect(single.container.querySelectorAll('details[data-kind="tool"]').length).toBe(1);
  single.unmount();
});
