// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { FoldKind, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { mount } from "../../testing/render.tsx";
import { ChatHeader } from "./ChatHeader.tsx";
import { Transcript } from "./Transcript.tsx";

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const stateOf = (el: Element): string | null =>
  (el.matches('[data-slot="trace-collapsible"]') ? el : el.querySelector('[data-slot="trace-collapsible"]'))?.getAttribute("data-state") ?? el.getAttribute("data-state");
const localIso = new Date(2026, 4, 4, 13, 58, 7).toISOString();

afterEach(() => {
  document.body.innerHTML = "";
});

it("chat header shows only the title", () => {
  const m = mount(<ChatHeader sessionKey="ctx:plan" title="My title" role="lead" runner="pi" />);
  const h = m.container.querySelector("header") as HTMLElement;
  expect(h.querySelector('h1[data-part="title"]')?.textContent).toBe("My title");
  expect(h.textContent).toBe("My title");
  expect(h.querySelector('[data-part="subtitle"], [data-part="toggles"], button')).toBeNull();
  m.unmount();
});

it("user message renders a right aligned muted bubble with a sent time footer", () => {
  const user: TranscriptItem = { kind: "user", id: "u1", at: localIso, text: "hello", attachments: [] };
  const m = mount(<Transcript {...base} items={[user]} />);
  const a = m.container.querySelector('[data-kind="user"]') as HTMLElement;
  expect(a.getAttribute("data-align")).toBe("end");
  expect(a.querySelector('[data-slot="bubble"][data-variant="muted"] [data-part="text"]')?.textContent).toBe("hello");
  expect(a.querySelector('[data-slot="message-footer"] span')?.textContent).toBe("You · 13:58");
  m.unmount();
});

it("user message renders attachment chips", () => {
  const user: TranscriptItem = {
    kind: "user",
    id: "u1",
    text: "look",
    attachments: [
      { kind: "image", mimeType: "image/png" },
      { kind: "text", name: "notes.txt", text: "x" } as never,
    ],
  };
  const m = mount(<Transcript {...base} items={[user]} />);
  const chips = m.container.querySelectorAll('[data-part="attachments"] [data-attachment]');
  expect(chips.length).toBe(2);
  expect(chips[0]?.querySelector('[data-part="name"]')?.textContent).toBe("image/png");
  expect(chips[1]?.querySelector('[data-part="name"]')?.textContent).toBe("notes.txt");
  m.unmount();
});

it("assistant text renders as a muted bubble with a time footer and no stamp", () => {
  const item: TranscriptItem = { kind: "assistant", id: "a1", at: localIso, streaming: false, blocks: [{ type: "text", text: "**bold**" }] };
  const m = mount(<Transcript {...base} items={[item]} sessionKey="ctx:plan" />);
  const msg = m.container.querySelector('[data-kind="assistant"]') as HTMLElement;
  const bubble = msg.querySelector('[data-slot="bubble"]') as HTMLElement;
  expect(bubble.getAttribute("data-variant")).toBe("muted");
  expect(bubble.querySelector('[data-slot="bubble-content"][data-part="card"] .typeset-chat strong')?.textContent).toBe("bold");
  expect(msg.querySelector('[data-part="stamp"]')).toBeNull();
  expect(msg.querySelector('[data-slot="message-footer"]')?.textContent).toContain("13:58");
  expect(msg.textContent).not.toContain("ctx:plan");
  m.unmount();
});

it("thinking renders inside a collapsed chain of thought", () => {
  const item: TranscriptItem = { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "thinking", text: "pondering deeply" }] };
  const m = mount(<Transcript {...base} items={[item]} />);
  const chain = m.container.querySelector('[data-kind="chain-of-thought"]') as HTMLElement;
  expect(chain).not.toBeNull();
  expect(stateOf(chain)).toBe("closed");
  expect(chain.textContent).not.toContain("pondering deeply");
  expect(chain.querySelector('[data-part="label"]')?.textContent).toBe("Chain of Thought");
  act(() => (chain.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  const t = chain.querySelector('[data-kind="thinking"]') as HTMLElement;
  expect(t.querySelector('[data-part="label"]')?.textContent).toBe("Thought");
  act(() => (t.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  expect(t.querySelector('[data-part="thinking-body"]')?.textContent).toBe("pondering deeply");
  m.unmount();
});

it("cpd fold renders a one line phi marker", () => {
  const kinds: FoldKind[] = ["cpd-context", "cpd-plan-decision", "cpd-plan-summary", "cpd-skills", "cpd"];
  const items: TranscriptItem[] = kinds.map((k) => ({ kind: "fold", id: `f-${k}`, at: localIso, foldKind: k, title: `Title ${k}`, body: "body", defaultOpen: false }));
  const m = mount(<Transcript {...base} items={items} />);
  for (const k of kinds) {
    const d = m.container.querySelector(`[data-kind="fold"][data-fold="${k}"]`) as HTMLElement;
    const marker = d.querySelector('[data-part="marker"]') as HTMLElement;
    expect(marker).not.toBeNull();
    expect(marker.querySelector('[data-part="phi"]')?.textContent).toBe("φ");
    expect(marker.querySelector('[data-part="source"]')?.textContent).toBe(k);
    expect(marker.querySelector('[data-part="marker-text"]')?.textContent).toBe(`Title ${k}`);
    expect(marker.querySelector('[data-part="time"]')?.textContent).toBe("13:58");
  }
  m.unmount();
});

it("compaction renders a collapsed divider", () => {
  const item: TranscriptItem = { kind: "compaction", id: "c1", summary: "earlier work", tokensBefore: 1234 };
  const m = mount(<Transcript {...base} items={[item]} />);
  const d = m.container.querySelector('[data-kind="compaction"]') as HTMLElement;
  expect(stateOf(d)).toBe("closed");
  expect(d.querySelector('[data-slot="trace-collapsible-trigger"] [data-part="label"]')?.textContent).toBe("Context compacted · from 1.2k tokens");
  act(() => (d.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  expect(stateOf(d)).toBe("open");
  expect(d.querySelector('[data-part="compaction-body"]')?.textContent).toContain("earlier work");
  m.unmount();
});

it("a sent pending user row shows no failure text", () => {
  const p: PendingSubmission = { clientSubmissionId: "p1", text: "hi there", imageCount: 0, whenBusy: "followUp", state: "sent" };
  const m = mount(<Transcript {...base} items={[]} pending={[p]} />);
  const li = m.container.querySelector('li[data-pending="sent"]') as HTMLElement;
  expect(li.textContent).toContain("hi there");
  expect(li.textContent).toContain("You");
  expect(li.textContent).not.toContain("sent");
  expect(li.textContent).not.toContain("failed");
  expect(li.textContent).not.toContain("unknown error");
  expect(li.querySelector('[data-part="pending-status"]')?.getAttribute("data-state")).toBe("sent");
  expect(li.querySelector('[role="alert"]')).toBeNull();
  m.unmount();
});

it("transcript rows are message scroller items keyed by data-row", () => {
  const items: TranscriptItem[] = [
    { kind: "user", id: "u1", text: "a", attachments: [] },
    { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "text", text: "b" }] },
    { kind: "compaction", id: "c1", summary: "s" },
    { kind: "fold", id: "f1", foldKind: "cpd", title: "t", body: "b", defaultOpen: false },
  ];
  const m = mount(<Transcript {...base} items={items} />);
  const rows = [...m.container.querySelectorAll('[data-slot="message-scroller-item"]')];
  expect(rows.map((r) => r.getAttribute("data-row"))).toEqual(["u1", "a1", "c1", "f1"]);
  expect(m.container.querySelector('[data-slot="message-scroller-viewport"][aria-label="Conversation transcript"]')).not.toBeNull();
  m.unmount();
});
