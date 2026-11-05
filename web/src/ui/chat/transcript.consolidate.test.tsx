// @vitest-environment jsdom
// Red tests for the consolidated transcript (DESIGN-chat 2; pro-d55..pro-d61): shadcn Message and
// Bubble turns, a Chain of Thought row, collapsed edit blocks, image thumbnails, a 160px scroll edge.
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { mount } from "../../testing/render.tsx";
import { ToolRow } from "./ToolRow.tsx";
import { Transcript } from "./Transcript.tsx";

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const clock = (at: string): string => {
  const d = new Date(at);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
};
const tool = (id: string, over: Partial<ToolItem> = {}): ToolItem => ({
  kind: "tool", id: `tool:${id}`, callId: id, name: "read", args: { path: `${id}.ts` }, status: "done",
  result: { text: "a\nb\n", isError: false, durationMs: 10 }, ...over,
});
const answer = (text = "done"): TranscriptItem => ({ kind: "assistant", id: "a-text", streaming: false, blocks: [{ type: "text", text }] });

afterEach(() => {
  vi.restoreAllMocks();
  delete (HTMLElement.prototype as { clientHeight?: number }).clientHeight;
});

it("transcript limits turns to 85% width, agent left and user right", () => {
  const items: TranscriptItem[] = [
    { kind: "user", id: "u1", text: "question", attachments: [] },
    answer("reply"),
  ];
  const m = mount(<Transcript {...base} items={items} />);
  const user = m.container.querySelector('[data-kind="user"]')!;
  const agent = m.container.querySelector('[data-kind="assistant"]')!;
  const userRow = user.closest<HTMLElement>('[class*="max-w-[85%]"]');
  const agentRow = agent.closest<HTMLElement>('[class*="max-w-[85%]"]');
  expect(userRow?.classList.contains("ml-auto"), "user turn is right aligned and at most 85% wide").toBe(true);
  expect(agentRow?.classList.contains("mr-auto"), "agent turn is left aligned and at most 85% wide").toBe(true);
  expect(user.getAttribute("data-slot")).toBe("message");
  expect(user.getAttribute("data-align")).toBe("end");
  expect(agent.getAttribute("data-align")).toBe("start");
  expect(user.querySelector('[data-slot="bubble"]')?.getAttribute("data-variant")).toBe("muted");
  expect(agent.querySelector('[data-slot="bubble"]')?.getAttribute("data-variant")).toBe("muted");
  m.unmount();
});

it("assistant messages carry no session key header", () => {
  const m = mount(<Transcript {...base} items={[answer("reply")]} sessionKey="lead-1" />);
  const agent = m.container.querySelector('[data-kind="assistant"]')!;
  expect(agent.textContent).toContain("reply");
  expect(agent.querySelector('[data-slot="message-header"]')).toBeNull();
  expect(agent.querySelector('[data-part="stamp"]')).toBeNull();
  expect(m.container.textContent).not.toContain("lead-1");
  expect(agent.querySelector('[data-slot="message-footer"] [data-action="copy-turn"]'), "the footer carries the Copy action").not.toBeNull();
  m.unmount();
});

it("consecutive thought and tool items group into a collapsed chain of thought", () => {
  const items: TranscriptItem[] = [
    { kind: "assistant", id: "a-think", streaming: false, at: "2026-05-01T14:05:00Z", blocks: [{ type: "thinking", text: "pondering the plan" }] },
    tool("t1"),
    tool("t2"),
    answer("all done"),
  ];
  const m = mount(<Transcript {...base} items={items} />);
  const chains = m.container.querySelectorAll<HTMLElement>('[data-kind="chain-of-thought"]');
  expect(chains.length, "one chain for the whole run").toBe(1);
  const chain = chains[0]!;
  const collapsible = chain.querySelector<HTMLElement>('[data-slot="trace-collapsible"]')!;
  expect(collapsible.getAttribute("data-state")).toBe("closed");
  const meta = chain.querySelector('[data-part="meta"]')?.textContent ?? "";
  expect(meta).toContain("3 steps");
  expect(meta).toMatch(/\d{2}:\d{2}/);
  expect(chain.textContent).not.toContain("pondering the plan");
  expect(m.container.querySelector('[data-kind="tool"]'), "no tool row while the chain is collapsed").toBeNull();
  act(() => chain.querySelector<HTMLElement>('[data-slot="trace-collapsible-trigger"]')!.click());
  expect(chain.querySelector('[data-slot="trace-collapsible"]')?.getAttribute("data-state")).toBe("open");
  expect(chain.querySelector('[data-kind="thinking"]')).not.toBeNull();
  expect(chain.querySelector('[data-kind="tool-group"]')).not.toBeNull();
  expect(m.container.querySelector('[data-kind="assistant"]')?.textContent).toContain("all done");
  m.unmount();
});

it("user messages show their sent time", () => {
  const at = "2026-05-01T14:05:00Z";
  const items: TranscriptItem[] = [{ kind: "user", id: "u1", text: "hello", attachments: [], at }];
  const m = mount(<Transcript {...base} items={items} />);
  const footer = m.container.querySelector('[data-kind="user"] [data-slot="message-footer"]');
  expect(footer?.textContent).toContain(`You · ${clock(at)}`);
  expect(footer?.querySelector('[data-action="copy-turn"]')).not.toBeNull();
  m.unmount();
});

it("edit tool blocks are collapsed by default", () => {
  const edit = tool("e1", { name: "edit", args: { path: "src/x.ts" }, diff: "@@ -1 +1 @@\n-old\n+new", result: { text: "edited", isError: false } });
  const m = mount(<ToolRow tool={edit} />);
  const row = m.container.querySelector<HTMLElement>('[data-kind="tool"][data-tool="edit"]')!;
  expect(row.getAttribute("data-state")).toBe("closed");
  expect(m.container.querySelector('[data-slot="code-block"]'), "no diff until opened").toBeNull();
  act(() => row.querySelector<HTMLElement>('[data-slot="trace-collapsible-trigger"]')!.click());
  expect(row.getAttribute("data-state")).toBe("open");
  const diff = m.container.querySelector('[data-slot="code-block"][data-part="diff"]');
  expect(diff, "the diff is a CodeBlock").not.toBeNull();
  expect(diff?.querySelectorAll('[data-slot="code-block-line"][data-diff="add"]').length).toBe(1);
  expect(diff?.querySelectorAll('[data-slot="code-block-line"][data-diff="remove"]').length).toBe(1);
  m.unmount();
});

it("user messages render image thumbnails", () => {
  const echoed: TranscriptItem = {
    kind: "user", id: "u1", text: "look", attachments: [{ kind: "image", mimeType: "image/png", dataBase64: "QUJD" }],
  };
  // `images` is the pending submission's bytes (DESIGN-chat 2.7); the field is added with the store change.
  const pending = {
    clientSubmissionId: "p1", text: "and this", imageCount: 1, whenBusy: "followUp", state: "sending",
    images: [{ mimeType: "image/jpeg", dataBase64: "REVG" }],
  } as unknown as PendingSubmission;
  const m = mount(<Transcript {...base} items={[echoed]} pending={[pending]} />);
  const thumb = m.container.querySelector('[data-kind="user"] img[data-part="thumb"]');
  expect(thumb?.getAttribute("src")).toBe("data:image/png;base64,QUJD");
  expect(thumb?.getAttribute("alt")).toBe("Attached screenshot");
  const optimistic = m.container.querySelector('li[data-pending="sending"] img[data-part="thumb"]');
  expect(optimistic?.getAttribute("src")).toBe("data:image/jpeg;base64,REVG");
  m.unmount();
});

it("long tool paths are truncated", () => {
  const path = `src/${"deeply/nested/".repeat(20)}file.ts`;
  const m = mount(<ToolRow tool={tool("long", { args: { path } })} />);
  const detail = m.container.querySelector('[data-kind="tool"] [data-part="detail"]')?.textContent ?? "";
  expect(path.length).toBeGreaterThan(200);
  expect(detail.length).toBeLessThanOrEqual(101);
  expect(detail.endsWith("…")).toBe(true);
  expect(detail.startsWith("src/deeply/")).toBe(true);
  m.unmount();
});

it("transcript auto-scrolls within a 160px edge threshold", () => {
  // jsdom has no layout: fake the viewport (400px) and how far the last row's bottom is below it.
  let overflow = 0;
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const slot = this.getAttribute("data-slot");
    const top = 0;
    const bottom = slot === "message-scroller-viewport" ? 400 : slot === "message-scroller-item" ? 400 + overflow : 0;
    return { top, bottom, left: 0, right: 0, width: 0, height: bottom - top, x: 0, y: top, toJSON: () => ({}) };
  });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return this.getAttribute("data-slot") === "message-scroller-viewport" ? 400 : 0;
    },
  });
  const m = mount(<Transcript {...base} items={[{ kind: "user", id: "u1", text: "hi", attachments: [] }, answer("hello")]} />);
  const viewport = m.container.querySelector<HTMLElement>('[data-slot="message-scroller-viewport"]');
  expect(viewport, "the transcript scrolls in a MessageScroller viewport").not.toBeNull();
  const scrollable = (): string => viewport!.getAttribute("data-scrollable") ?? "";
  overflow = 150;
  act(() => { viewport!.dispatchEvent(new Event("scroll")); });
  expect(scrollable(), "150px from the bottom counts as at the end").not.toContain("end");
  overflow = 200;
  act(() => { viewport!.dispatchEvent(new Event("scroll")); });
  expect(scrollable(), "200px from the bottom is no longer at the end").toContain("end");
  m.unmount();
});
