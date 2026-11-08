// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { mount } from "../../testing/render.tsx";
import { Transcript } from "./Transcript.tsx";

const stateOf = (el: Element): string | null =>
  (el.matches('[data-slot="trace-collapsible"]') ? el : el.querySelector('[data-slot="trace-collapsible"]'))?.getAttribute("data-state") ?? el.getAttribute("data-state");
const user: TranscriptItem = { kind: "user", id: "u1", text: "hello  there", attachments: [{ kind: "image", mimeType: "image/png", dataBase64: "QUJD" }] };
const assistant: TranscriptItem = {
  kind: "assistant",
  id: "a1",
  streaming: false,
  blocks: [
    { type: "thinking", text: "pondering" },
    { type: "text", text: "**bold answer**" },
  ],
};
const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };

it("renders user and assistant messages", () => {
  const m = mount(<Transcript {...base} items={[user, assistant]} />);
  expect(m.container.querySelector('[data-part="transcript"]')).not.toBeNull();
  expect(m.container.querySelector('[data-part="viewport"]')?.getAttribute("aria-label")).toBe("Conversation transcript");
  const u = m.container.querySelector('[data-kind="user"]');
  expect(u?.querySelector('[data-part="text"]')?.textContent).toBe("hello  there");
  expect(u?.querySelector('img[data-part="thumb"]')?.getAttribute("src")).toBe("data:image/png;base64,QUJD");
  const a = m.container.querySelector('[data-kind="assistant"]');
  expect(a?.querySelector("strong")?.textContent).toBe("bold answer");
  m.unmount();
});

it("an image attachment without bytes shows its mime type", () => {
  const bare: TranscriptItem = { kind: "user", id: "u2", text: "x", attachments: [{ kind: "image", mimeType: "image/png" }] };
  const m = mount(<Transcript {...base} items={[bare]} />);
  expect(m.container.querySelector('[data-kind="user"] [data-attachment="image"] [data-part="name"]')?.textContent).toBe("image/png");
  m.unmount();
});

it("assistant thinking blocks are collapsed inside a chain of thought", () => {
  const m = mount(<Transcript {...base} items={[assistant]} />);
  const chain = m.container.querySelector('[data-kind="chain-of-thought"]') as HTMLElement;
  expect(stateOf(chain)).toBe("closed");
  expect(chain.querySelector('[data-part="label"]')?.textContent).toBe("Chain of Thought");
  expect(chain.textContent).not.toContain("pondering");
  act(() => (chain.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  expect(stateOf(chain)).toBe("open");
  const thought = chain.querySelector('[data-kind="thinking"]') as HTMLElement;
  expect(thought.querySelector('[data-part="label"]')?.textContent).toBe("Thought");
  act(() => (thought.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  expect(thought.querySelector('[data-part="thinking-body"]')?.textContent).toContain("pondering");
  m.unmount();
});

it("thinking expanded opens the chain of thought", () => {
  const m = mount(<Transcript {...base} thinking="expanded" items={[assistant]} />);
  expect(stateOf(m.container.querySelector('[data-kind="chain-of-thought"]') as HTMLElement)).toBe("open");
  m.unmount();
});

it("streaming assistant text shows a working marker", () => {
  const streaming: TranscriptItem = { kind: "assistant", id: "a2", streaming: true, blocks: [{ type: "text", text: "partial" }] };
  const m = mount(<Transcript {...base} items={[streaming, assistant]} />);
  const markers = m.container.querySelectorAll('[data-part="working"]');
  expect(markers.length).toBe(1);
  expect(markers[0]?.getAttribute("role")).toBe("status");
  expect(markers[0]?.textContent).toContain("Working…");
  const first = m.container.querySelector('[data-kind="assistant"] [data-slot="message-content"]');
  expect(first?.querySelector('[data-part="working"]')).toBe(markers[0]);
  m.unmount();
});

// `loadingEarlier` is added to the props by the implementation (DESIGN-chat 2.8).
const loading = { loadingEarlier: true } as Record<string, unknown>;

it("Load earlier button calls onLoadEarlier when hasEarlier", () => {
  const onLoadEarlier = vi.fn();
  const m = mount(<Transcript {...base} hasEarlier items={[user]} onLoadEarlier={onLoadEarlier} />);
  const btn = m.container.querySelector('button[data-action="load-earlier"]') as HTMLButtonElement;
  expect(btn.textContent).toBe("Load earlier");
  act(() => btn.click());
  expect(onLoadEarlier).toHaveBeenCalledTimes(1);
  m.unmount();
  const m2 = mount(<Transcript {...base} items={[user]} />);
  expect(m2.container.querySelector('[data-action="load-earlier"]')).toBeNull();
  m2.unmount();
  const m3 = mount(<Transcript {...base} {...loading} hasEarlier items={[user]} />);
  const busy = m3.container.querySelector('button[data-action="load-earlier"]') as HTMLButtonElement;
  expect(busy.textContent).toBe("Loading…");
  expect(busy.disabled).toBe(true);
  m3.unmount();
});

it("compaction renders as a collapsed trace", () => {
  const item: TranscriptItem = { kind: "compaction", id: "c1", summary: "earlier work summarised", tokensBefore: 1234 };
  const m = mount(<Transcript {...base} items={[item]} />);
  const d = m.container.querySelector('[data-kind="compaction"]') as HTMLElement;
  expect(stateOf(d)).toBe("closed");
  expect(d.querySelector('[data-slot="trace-collapsible-trigger"]')?.textContent).toContain("Context compacted · from 1.2k tokens");
  act(() => (d.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  expect(d.textContent).toContain("earlier work summarised");
  m.unmount();
});

// `at` is added to PendingSubmission by the store change (DESIGN-chat 2.7).
const pending = [
  { clientSubmissionId: "p1", text: "sending text", imageCount: 0, whenBusy: "followUp", state: "sending", at: "2026-05-01T14:05:00Z" },
] as unknown as PendingSubmission[];

it("queued messages render after the transcript", () => {
  const m = mount(
    <Transcript
      {...base}
      items={[user]}
      queued={[{ clientSubmissionId: "q1", text: "queued text", whenBusy: "followUp" }]}
      pending={pending}
    />,
  );
  const list = m.container.querySelector('ul[aria-label="Queued messages"]') as HTMLElement;
  expect(list.querySelector('li[data-queued="q1"]')?.textContent).toContain("queued text");
  expect(list.querySelector('li[data-queued="q1"] [data-part="queued-status"]')?.textContent).toBe("queued");
  expect(list.querySelector('li[data-queued="q1"] [data-action="copy-turn"]')).toBeNull();
  const sending = list.querySelector('li[data-pending="sending"]') as HTMLElement;
  expect(sending.textContent).toContain("sending text");
  expect(sending.textContent).not.toContain("sending…");
  expect(sending.querySelector('[data-part="pending-status"]')?.textContent).toMatch(/^You · \d{2}:\d{2}$/);
  const row = m.container.querySelector('[data-kind="user"]') as HTMLElement;
  expect(row.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  m.unmount();
  const m2 = mount(<Transcript {...base} items={[user]} />);
  expect(m2.container.querySelector('ul[aria-label="Queued messages"]')).toBeNull();
  m2.unmount();
});

it("the transcript renders no inline failed text", () => {
  const failed: ToolItem = { kind: "tool", id: "tool:t1", callId: "t1", name: "bash", args: { command: "x" }, status: "failed", result: { text: "boom", isError: true } };
  // A failed submission's row leaves the transcript (15.1); the cast stands in for the removed state.
  const failedPending = {
    clientSubmissionId: "p1", text: "bad text", imageCount: 0, whenBusy: "followUp", state: "failed", error: { code: "internal", message: "boom" },
  } as unknown as PendingSubmission;
  const m = mount(<Transcript {...base} tools="expanded" items={[failed]} pending={[failedPending]} />);
  expect(m.container.textContent ?? "").not.toMatch(/failed:/i);
  expect(m.container.querySelector('[data-part="pending-status"][data-state="failed"]')).toBeNull();
  expect(m.container.querySelector('li[data-pending="failed"] [role="alert"]')).toBeNull();
  m.unmount();
});

it("an empty transcript shows the empty state", () => {
  const m = mount(<Transcript {...base} items={[]} />);
  expect(m.container.querySelector("[data-empty]")?.textContent).toContain("No messages yet");
  m.unmount();
});

it("every row is mounted with its row key, with no virtual window", () => {
  const items: TranscriptItem[] = Array.from({ length: 200 }, (_, i) => ({ kind: "user", id: `u${i}`, text: `m${i}`, attachments: [] }));
  const m = mount(<Transcript {...base} items={items} />);
  const rows = m.container.querySelectorAll('[data-slot="message-scroller-item"][data-row]');
  expect(rows.length).toBe(200);
  expect(rows[0]?.getAttribute("data-row")).toBe("u0");
  expect(m.container.querySelector('[role="log"][aria-label="Transcript"]')).toBeNull();
  expect(m.container.querySelector("[data-row-key], [data-part='virtual-space']")).toBeNull();
  m.unmount();
});

it("the jump to bottom button is the message scroller button", () => {
  const m = mount(<Transcript {...base} items={[user]} />);
  const jump = m.container.querySelector('button[data-action="jump-to-bottom"]');
  expect(jump?.getAttribute("data-slot")).toBe("message-scroller-button");
  m.unmount();
});

it("a lone tool run is a collapsed chain with one step", () => {
  const t: ToolItem = { kind: "tool", id: "tool:t1", callId: "t1", name: "read", args: { path: "a.ts" }, status: "done" };
  const m = mount(<Transcript {...base} items={[t]} />);
  const chain = m.container.querySelector('[data-kind="chain-of-thought"]') as HTMLElement;
  expect(chain.querySelector('[data-part="meta"]')?.textContent).toContain("1 step");
  expect(m.container.querySelector('[data-kind="tool"]')).toBeNull();
  m.unmount();
});
