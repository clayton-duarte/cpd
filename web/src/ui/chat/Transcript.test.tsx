// @vitest-environment jsdom
import { act, useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PendingSubmission } from "../../state/store.ts";
import type { SizeObserver } from "./virtual.ts";
import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";
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
const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };

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
  const d = m.container.querySelector('[data-kind="thinking"]') as HTMLElement;
  expect(d.getAttribute("data-state")).toBe("closed");
  expect(d.querySelector('[data-part="label"]')?.textContent).toBe("Thinking");
  expect(d.textContent).not.toContain("pondering");
  act(() => (d.querySelector('[data-slot="trace-collapsible-trigger"]') as HTMLElement).click());
  expect(d.getAttribute("data-state")).toBe("open");
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
  expect(d.querySelector("summary")?.textContent).toContain("Context compacted");
  expect(d.querySelector("summary")?.textContent).toContain("1.2k tokens");
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

// ---- Phase 2 (CONTRACT 10.4): virtual, sticky Transcript. jsdom has no layout: everything below is faked.
const VIEWPORT = 500;
const EST = 96;
const scrollTops = new WeakMap<Element, number>();
const isLog = (el: Element): boolean => el.getAttribute("role") === "log";
const spaceHeight = (el: Element): number => {
  const space = el.querySelector('[data-part="virtual-space"]') as HTMLElement | null;
  return space === null ? 0 : parseFloat(space.style.height);
};

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollTop", {
    configurable: true,
    get(this: HTMLElement) {
      return scrollTops.get(this) ?? 0;
    },
    set(this: HTMLElement, v: number) {
      scrollTops.set(this, v);
    },
  });
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return isLog(this) ? spaceHeight(this) : 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return isLog(this) ? VIEWPORT : 0;
    },
  });
});
afterEach(() => {
  delete (HTMLElement.prototype as { scrollTop?: number }).scrollTop;
  delete (HTMLElement.prototype as { scrollHeight?: number }).scrollHeight;
  delete (HTMLElement.prototype as { clientHeight?: number }).clientHeight;
});

function fakeSizes(): SizeObserver & { report(el: Element, height: number): void } {
  const subs: { el: Element; cb: (h: number) => void }[] = [];
  return {
    observe(el, cb) {
      const sub = { el, cb };
      subs.push(sub);
      return {
        dispose() {
          const i = subs.indexOf(sub);
          if (i >= 0) subs.splice(i, 1);
        },
      };
    },
    report(el, height) {
      act(() => {
        for (const s of [...subs]) if (s.el === el) s.cb(height);
      });
    },
  };
}

type Props = Parameters<typeof Transcript>[0];
function harness(initial: Partial<Props> & { items: readonly TranscriptItem[] }) {
  const sizes = fakeSizes();
  let current: Props = { ...base, sizes, ...initial };
  let set: (p: Props) => void = () => {};
  function Host() {
    const [p, setP] = useState(current);
    set = setP;
    return <Transcript {...p} />;
  }
  const m = mount(<Host />);
  const log = m.container.querySelector('[role="log"]') as HTMLElement;
  sizes.report(log, VIEWPORT);
  return {
    m,
    sizes,
    log,
    update(patch: Partial<Props>) {
      current = { ...current, ...patch };
      act(() => set(current));
    },
    scrollTo(top: number) {
      log.scrollTop = top;
      act(() => {
        log.dispatchEvent(new Event("scroll"));
      });
    },
    row(key: string) {
      return m.container.querySelector(`[data-row-key="${key}"]`) as HTMLElement | null;
    },
    keys() {
      return [...m.container.querySelectorAll("[data-row-key]")].map((e) => e.getAttribute("data-row-key"));
    },
    jump() {
      return m.container.querySelector('button[data-action="jump-to-bottom"]') as HTMLButtonElement | null;
    },
  };
}
const users = (from: number, to: number): TranscriptItem[] =>
  Array.from({ length: to - from }, (_, i) => ({ kind: "user", id: `i${from + i}`, text: `message ${from + i}`, attachments: [] }) as TranscriptItem);

it("a 1000 item transcript mounts at most 60 rows", () => {
  const h = harness({ items: users(0, 1000) });
  const n = h.keys().length;
  expect(n, "mounted rows").toBeGreaterThan(0);
  expect(n, "mounted rows").toBeLessThanOrEqual(60);
  h.m.unmount();
});

it("rows carry data-row-key", () => {
  const tool = (id: string): ToolItem => ({ kind: "tool", id: `tool:${id}`, callId: id, name: "read", args: {}, status: "running" });
  const h = harness({ items: [user, assistant, tool("t1"), tool("t2"), { kind: "compaction", id: "c1", summary: "s", tokensBefore: 1 }, tool("t3")] });
  expect(h.keys()).toEqual(["u1", "a1", "group:tool:t1", "c1", "tool:t3"]);
  expect(h.row("u1")?.querySelector('article[data-kind="user"]'), "row markup inside the keyed div").not.toBeNull();
  h.m.unmount();
});

it("the transcript is pinned to the bottom on first render", () => {
  const h = harness({ items: users(0, 1000) });
  expect(h.m.container.querySelector('[data-part="transcript"] > [role="log"]'), "log inside transcript wrapper").not.toBeNull();
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  expect(h.log.scrollTop, "scrollTop equals scrollHeight").toBe(h.log.scrollHeight);
  expect(h.log.scrollHeight).toBeGreaterThan(0);
  expect(h.row("i999"), "last row mounted").not.toBeNull();
  expect(h.jump(), "no jump button at bottom").toBeNull();
  h.m.unmount();
});

it("a new item keeps the viewport pinned while pinned", () => {
  const h = harness({ items: users(0, 50) });
  expect(h.log.scrollTop).toBe(50 * EST);
  h.update({ items: users(0, 51) });
  expect(h.log.scrollTop).toBe(51 * EST);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});

it("scrolling up disengages the pin and shows Jump to bottom", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  expect(h.log.getAttribute("data-at-bottom")).toBe("false");
  expect(h.jump()?.textContent).toBe("Jump to bottom");
  expect(h.jump()?.parentElement?.getAttribute("data-part"), "button is a sibling after the log in the wrapper").toBe("transcript");
  h.m.unmount();
});

it("a new item does not move the viewport while not pinned", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  h.update({ items: users(0, 51) });
  expect(h.log.scrollTop).toBe(1000);
  expect(h.log.getAttribute("data-at-bottom")).toBe("false");
  h.m.unmount();
});

it("Jump to bottom scrolls to the bottom and re-pins", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  const btn = h.jump() as HTMLButtonElement;
  act(() => btn.click());
  expect(h.log.scrollTop).toBe(h.log.scrollHeight);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  expect(h.jump()).toBeNull();
  h.update({ items: users(0, 51) });
  expect(h.log.scrollTop, "stays pinned after jump").toBe(51 * EST);
  h.m.unmount();
});

it("a tail window change re-pins the viewport", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  h.update({ items: users(100, 160), tailEpoch: 1 });
  expect(h.log.scrollTop).toBe(60 * EST);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});

it("a new pending submission re-pins the viewport", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  const p: PendingSubmission = { clientSubmissionId: "p1", text: "hi", imageCount: 0, whenBusy: "followUp", state: "sending" };
  h.update({ pending: [p] });
  expect(h.log.scrollTop).toBe(h.log.scrollHeight);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});

it("prepending earlier items keeps the first visible row at the same offset", () => {
  const h = harness({ items: users(50, 150) });
  h.scrollTo(2000); // row i70 (offset 1920) + 80
  h.update({ items: users(40, 150) });
  expect(h.log.scrollTop).toBe(2000 + 10 * EST);
  expect(h.log.getAttribute("data-at-bottom")).toBe("false");
  h.m.unmount();
});

it("a measured height change above the viewport keeps the visible row in place", () => {
  const h = harness({ items: users(0, 100) });
  h.scrollTo(2000);
  const above = h.row("i17") as HTMLElement; // offsets 1632..1728, above the viewport top
  expect(above, "row above the viewport is mounted (overscan)").not.toBeNull();
  h.sizes.report(above, EST + 100);
  expect(h.log.scrollTop).toBe(2100);
  h.m.unmount();
});

it("queued and pending messages render inside the log after the rows", () => {
  const h = harness({
    items: users(0, 3),
    queued: [{ clientSubmissionId: "q1", text: "queued text", whenBusy: "followUp" }],
    pending: [{ clientSubmissionId: "p1", text: "sending text", imageCount: 0, whenBusy: "followUp", state: "sending" }],
  });
  const list = h.log.querySelector('ul[aria-label="Queued messages"]') as HTMLElement | null;
  expect(list, "queued list inside the log").not.toBeNull();
  const space = h.log.querySelector('[data-part="virtual-space"]') as HTMLElement | null;
  expect(space, "virtual-space inside the log").not.toBeNull();
  expect((space as HTMLElement).compareDocumentPosition(list as HTMLElement) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(list?.querySelector('li[data-queued="q1"]')).not.toBeNull();
  expect(list?.querySelector('li[data-pending="sending"]')).not.toBeNull();
  h.m.unmount();
});

it("an unrelated re-render does not snap a pinned transcript back to the bottom", () => {
  const h = harness({ items: users(0, 50) });
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  const bottom = h.log.scrollTop;
  h.log.scrollTop = bottom - 300; // the user scrolled up; the scroll event has not fired yet
  h.update({ items: users(0, 50) }); // identical items, new array: a re-render
  expect(h.log.scrollTop, "scrollTop untouched by the commit").toBe(bottom - 300);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});

it("a scroll event that moves up unpins even within the near-bottom distance", () => {
  const h = harness({ items: users(0, 50) });
  const top = h.log.scrollHeight - VIEWPORT - 20; // 20px above the bottom, inside NEAR_BOTTOM_PX
  h.scrollTo(top);
  expect(h.log.getAttribute("data-at-bottom")).toBe("false");
  h.update({ items: users(0, 50) });
  expect(h.log.scrollTop, "a following re-render does not move scrollTop").toBe(top);
  h.m.unmount();
});

it("a new item still scrolls a pinned transcript to the bottom", () => {
  const h = harness({ items: users(0, 50) });
  h.update({ items: users(0, 52) });
  expect(h.log.scrollTop).toBe(h.log.scrollHeight);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});

it("a new pending submission still scrolls an unpinned transcript to the bottom", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  const p: PendingSubmission = { clientSubmissionId: "p9", text: "hi", imageCount: 0, whenBusy: "followUp", state: "sending" };
  h.update({ pending: [p] });
  expect(h.log.scrollTop).toBe(h.log.scrollHeight);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});

it("the Jump to bottom button still scrolls to the bottom", () => {
  const h = harness({ items: users(0, 50) });
  h.scrollTo(1000);
  act(() => (h.jump() as HTMLButtonElement).click());
  expect(h.log.scrollTop).toBe(h.log.scrollHeight);
  expect(h.log.getAttribute("data-at-bottom")).toBe("true");
  h.m.unmount();
});
