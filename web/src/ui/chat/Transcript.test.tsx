// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { TranscriptItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Transcript } from "./Transcript.tsx";

const user: TranscriptItem = { kind: "user", id: "u1", text: "hello  there", attachments: [{ kind: "image", mimeType: "image/png" }] };
const assistant: TranscriptItem = {
  kind: "assistant",
  id: "a1",
  streaming: false,
  blocks: [
    { type: "thinking", text: "pondering" },
    { type: "text", text: "**bold answer**" },
  ],
};
const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {} };

it("renders user and assistant messages", () => {
  const m = mount(<Transcript {...base} items={[user, assistant]} />);
  const log = m.container.querySelector('[role="log"]');
  expect(log?.getAttribute("aria-label")).toBe("Transcript");
  const u = m.container.querySelector('article[data-kind="user"]');
  expect(u?.textContent).toContain("hello  there");
  expect(u?.querySelector('[data-attachment="image"]')?.textContent).toBe("image/png");
  const a = m.container.querySelector('article[data-kind="assistant"]');
  expect(a?.querySelector("strong")?.textContent).toBe("bold answer");
  m.unmount();
});

it("assistant thinking blocks are collapsed behind Thinking…", () => {
  const m = mount(<Transcript {...base} items={[assistant]} />);
  const d = m.container.querySelector('details[data-kind="thinking"]') as HTMLDetailsElement;
  expect(d.open).toBe(false);
  expect(d.querySelector("summary")?.textContent).toBe("Thinking…");
  expect(d.textContent).toContain("pondering");
  m.unmount();
});

it("streaming assistant text shows a working marker", () => {
  const streaming: TranscriptItem = { kind: "assistant", id: "a2", streaming: true, blocks: [{ type: "text", text: "partial" }] };
  const m = mount(<Transcript {...base} items={[streaming, assistant]} />);
  const markers = m.container.querySelectorAll('[role="status"][data-part="working"]');
  expect(markers.length).toBe(1);
  expect(markers[0]?.textContent).toBe("Working…");
  const first = m.container.querySelector('article[data-kind="assistant"]');
  expect(first?.lastElementChild).toBe(markers[0]);
  m.unmount();
});

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
});

it("compaction renders as a fold", () => {
  const item: TranscriptItem = { kind: "compaction", id: "c1", summary: "earlier work summarised", tokensBefore: 1234 };
  const m = mount(<Transcript {...base} items={[item]} />);
  const d = m.container.querySelector('details[data-kind="compaction"]') as HTMLDetailsElement;
  expect(d.open).toBe(false);
  expect(d.querySelector("summary")?.textContent).toContain("Compaction");
  expect(d.querySelector("summary")?.textContent).toContain("1234");
  expect(d.textContent).toContain("earlier work summarised");
  m.unmount();
});

it("queued messages render after the transcript", () => {
  const m = mount(
    <Transcript
      {...base}
      items={[user]}
      queued={[{ clientSubmissionId: "q1", text: "queued text", whenBusy: "followUp" }]}
      pending={[
        { clientSubmissionId: "p1", text: "sending text", imageCount: 0, whenBusy: "followUp", state: "sending" },
        { clientSubmissionId: "p2", text: "bad text", imageCount: 0, whenBusy: "followUp", state: "failed", error: { code: "internal", message: "boom" } },
      ]}
    />,
  );
  const list = m.container.querySelector('ul[aria-label="Queued messages"]') as HTMLElement;
  expect(list.querySelector('li[data-queued="q1"]')?.textContent).toContain("queued text");
  expect(list.querySelector('li[data-pending="sending"]')?.textContent).toContain("sending…");
  expect(list.querySelector('li[data-pending="failed"]')?.textContent).toContain("failed: boom");
  const article = m.container.querySelector('article[data-kind="user"]') as HTMLElement;
  expect(article.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  m.unmount();
  const m2 = mount(<Transcript {...base} items={[user]} />);
  expect(m2.container.querySelector('ul[aria-label="Queued messages"]')).toBeNull();
  m2.unmount();
});
