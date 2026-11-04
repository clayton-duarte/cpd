// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { FoldKind, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { mount } from "../../testing/render.tsx";
import { ChatHeader } from "./ChatHeader.tsx";
import { Transcript } from "./Transcript.tsx";

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const localIso = new Date(2026, 4, 4, 13, 58, 7).toISOString();

afterEach(() => {
  document.body.innerHTML = "";
});

it("chat header shows title, subtitle and the thinking and tools toggles", () => {
  const m = mount(
    <ChatHeader sessionKey="ctx:plan" title="My title" role="lead" runner="pi" onToggleThinking={() => {}} onToggleTools={() => {}} />,
  );
  const h = m.container.querySelector("header") as HTMLElement;
  expect(h.querySelector('[data-part="title"]')?.textContent).toBe("My title");
  expect(h.querySelector('h1[data-part="key"]')?.textContent).toBe("ctx:plan");
  expect(h.querySelector('[data-part="subtitle"]')?.textContent).toBe("ctx:plan · lead · pi");
  const group = h.querySelector('[data-part="toggles"]');
  expect(group?.getAttribute("role")).toBe("group");
  expect(group?.getAttribute("aria-label")).toBe("Transcript display");
  expect(h.querySelector('button[data-action="toggle-thinking"]')?.textContent).toContain("Thinking: folded");
  expect(h.querySelector('button[data-action="toggle-tools"]')?.textContent).toContain("Tools: grouped");
  m.unmount();
});

it("chat header toggles report their state", () => {
  const onThinking = vi.fn();
  const onTools = vi.fn();
  const props = { sessionKey: "k", title: null, onToggleThinking: onThinking, onToggleTools: onTools };
  const m = mount(<ChatHeader {...props} />);
  const th = m.container.querySelector('button[data-action="toggle-thinking"]') as HTMLButtonElement;
  const to = m.container.querySelector('button[data-action="toggle-tools"]') as HTMLButtonElement;
  expect(th.getAttribute("aria-pressed")).toBe("false");
  expect(to.getAttribute("aria-pressed")).toBe("false");
  expect(th.getAttribute("aria-keyshortcuts")).toBe("Control+T");
  expect(to.getAttribute("aria-keyshortcuts")).toBe("Control+O");
  expect(th.querySelector("kbd")?.textContent).toBe("⌃T");
  expect(to.querySelector("kbd")?.textContent).toBe("⌃O");
  act(() => th.click());
  act(() => to.click());
  expect(onThinking).toHaveBeenCalledTimes(1);
  expect(onTools).toHaveBeenCalledTimes(1);
  m.unmount();
  const m2 = mount(<ChatHeader {...props} thinking="expanded" tools="expanded" />);
  const th2 = m2.container.querySelector('button[data-action="toggle-thinking"]');
  const to2 = m2.container.querySelector('button[data-action="toggle-tools"]');
  expect(th2?.getAttribute("aria-pressed")).toBe("true");
  expect(th2?.textContent).toContain("Thinking: expanded");
  expect(to2?.getAttribute("aria-pressed")).toBe("true");
  expect(to2?.textContent).toContain("Tools: expanded");
  m2.unmount();
});

it("user message renders a right aligned bubble with a timestamp", () => {
  const user: TranscriptItem = { kind: "user", id: "u1", at: localIso, text: "hello", attachments: [] };
  const m = mount(<Transcript {...base} items={[user]} />);
  const a = m.container.querySelector('article[data-kind="user"]') as HTMLElement;
  expect(a.querySelector('[data-part="bubble"] [data-part="text"]')?.textContent).toBe("hello");
  expect(a.querySelector('[data-part="meta"]')?.textContent).toBe("you · 13:58");
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

it("assistant text renders as a bubble without an extra bordered card", () => {
  const item: TranscriptItem = { kind: "assistant", id: "a1", at: localIso, streaming: false, blocks: [{ type: "text", text: "**bold**" }] };
  const m = mount(<Transcript {...base} items={[item]} sessionKey="ctx:plan" />);
  const art = m.container.querySelector('article[data-kind="assistant"]') as HTMLElement;
  const bubble = art.querySelector('[data-slot="bubble"]') as HTMLElement;
  expect(bubble).not.toBeNull();
  expect(bubble.getAttribute("data-align")).toBe("start");
  expect(art.querySelector('[data-part="card"]')).toBeNull();
  const content = bubble.querySelector('[data-slot="bubble-content"]') as HTMLElement;
  expect(content.querySelector(".typeset-chat strong")?.textContent).toBe("bold");
  expect(content.querySelector('[data-part="stamp"]')?.textContent).toBe("ctx:plan · 13:58");
  for (const e of [art, ...art.querySelectorAll("*")]) {
    const tokens = (e.getAttribute("class") ?? "").split(/\s+/);
    expect(tokens).not.toContain("bg-card");
    expect(tokens).not.toContain("border-border");
  }
  m.unmount();
});

it("thinking renders as a folded trace with one rail", () => {
  const item: TranscriptItem = { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "thinking", text: "pondering deeply" }] };
  const m = mount(<Transcript {...base} items={[item]} />);
  const t = m.container.querySelector('[data-kind="thinking"]') as HTMLElement;
  expect(t).not.toBeNull();
  expect(t.getAttribute("data-state")).toBe("closed");
  expect(t.closest('[data-slot="trace"]') ?? t.querySelector('[data-slot="trace"]')).not.toBeNull();
  expect(t.textContent).not.toContain("pondering deeply");
  expect(t.querySelector('[data-part="label"]')?.textContent).toBe("Thinking");
  const pill = t.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement;
  expect(pill.className).not.toMatch(/rounded-xl|(^|\s)border(\s|$)/);
  act(() => pill.click());
  expect(t.getAttribute("data-state")).toBe("open");
  expect(t.querySelector('[data-part="thinking-body"]')?.textContent).toBe("pondering deeply");
  expect(t.querySelectorAll('[data-part="rail"]').length).toBe(1);
  expect(t.querySelector('[data-part="thinking-body"] [data-part="rail"]')).toBeNull();
  m.unmount();
});

it("cpd fold renders a one line phi marker", () => {
  const kinds: FoldKind[] = ["cpd-context", "cpd-plan-decision", "cpd-plan-summary", "cpd-skills", "cpd"];
  const items: TranscriptItem[] = kinds.map((k) => ({ kind: "fold", id: `f-${k}`, at: localIso, foldKind: k, title: `Title ${k}`, body: "body", defaultOpen: false }));
  const m = mount(<Transcript {...base} items={items} />);
  for (const k of kinds) {
    const d = m.container.querySelector(`details[data-kind="fold"][data-fold="${k}"]`) as HTMLElement;
    const marker = d.querySelector('summary[data-part="marker"]') as HTMLElement;
    expect(marker).not.toBeNull();
    expect(marker.querySelector('[data-part="phi"]')?.textContent).toBe("φ");
    expect(marker.querySelector('[data-part="source"]')?.textContent).toBe(k);
    expect(marker.querySelector('[data-part="marker-text"]')?.textContent).toBe(`Title ${k}`);
    expect(marker.querySelector('[data-part="time"]')?.textContent).toBe("13:58");
  }
  m.unmount();
});

it("compaction renders a divider", () => {
  const item: TranscriptItem = { kind: "compaction", id: "c1", summary: "earlier work", tokensBefore: 1234 };
  const m = mount(<Transcript {...base} items={[item]} />);
  const d = m.container.querySelector('details[data-kind="compaction"]') as HTMLDetailsElement;
  expect(d.open).toBe(false);
  expect(d.querySelector('summary [data-part="label"]')?.textContent).toBe("Context compacted · from 1.2k tokens");
  expect(d.querySelectorAll('summary [data-part="rule"]').length).toBe(2);
  expect(d.querySelector('[data-part="compaction-body"]')?.textContent).toContain("earlier work");
  m.unmount();
});

it("a sent pending user row shows no failure text", () => {
  const p: PendingSubmission = { clientSubmissionId: "p1", text: "hi there", imageCount: 0, whenBusy: "followUp", state: "sent" };
  const m = mount(<Transcript {...base} items={[]} pending={[p]} />);
  const li = m.container.querySelector('li[data-pending="sent"]') as HTMLElement;
  expect(li.textContent).toBe("hi theresent");
  expect(li.textContent).not.toContain("failed");
  expect(li.textContent).not.toContain("unknown error");
  expect(li.querySelector('[data-part="pending-status"]')?.getAttribute("data-state")).toBe("sent");
  expect(li.querySelector('[role="alert"]')).toBeNull();
  m.unmount();
});

it("transcript rows keep the virtualization hooks", () => {
  const items: TranscriptItem[] = [
    { kind: "user", id: "u1", text: "a", attachments: [] },
    { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "text", text: "b" }] },
    { kind: "compaction", id: "c1", summary: "s" },
    { kind: "fold", id: "f1", foldKind: "cpd", title: "t", body: "b", defaultOpen: false },
  ];
  const m = mount(<Transcript {...base} items={items} />);
  const wrappers = m.container.querySelectorAll("[data-row-key]");
  expect(wrappers.length).toBe(4);
  for (const w of wrappers) {
    expect(w.children.length).toBe(1);
    expect((w.firstElementChild as HTMLElement).getAttribute("data-part")).toBe("row-body");
    expect(w.hasAttribute("data-index")).toBe(false);
    for (const el of [w, ...w.querySelectorAll<HTMLElement>("*")]) {
      const s = (el as HTMLElement).style;
      expect(s.position).not.toBe("absolute");
      expect(s.height).toBe("");
    }
  }
  m.unmount();
});
