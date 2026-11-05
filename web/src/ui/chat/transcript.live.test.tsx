// @vitest-environment jsdom
// Red tests for the live Chain of Thought, the Thinking… step, entry animations and the
// ResizeObserver auto-scroll (pro-d62). Fakes only: a fake ResizeObserver and a fake scroll container.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { MutableRefObject, RefObject } from "react";
import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import type { PendingSubmission } from "../../state/store.ts";
import { mount } from "../../testing/render.tsx";
import { MessageScrollerProvider } from "../components/message-scroller.tsx";
import { Transcript, TranscriptAutoScroll, showTrailingLiveChain } from "./Transcript.tsx";

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  readonly callback: () => void;
  constructor(callback: () => void) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
  fire(): void {
    this.callback();
  }
}

beforeEach(() => {
  FakeResizeObserver.instances = [];
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const user: TranscriptItem = { kind: "user", id: "u1", text: "hello", attachments: [] };
const done = (id: string): ToolItem => ({
  kind: "tool", id: `tool:${id}`, callId: id, name: "read", args: { path: `${id}.ts` }, status: "done",
  result: { text: "a\n", isError: false, durationMs: 5 },
});
const chainState = (c: HTMLElement): string | null =>
  c.querySelector('[data-kind="chain-of-thought"] [data-slot="trace-collapsible"]')?.getAttribute("data-state") ?? null;

it("showTrailingLiveChain is true while busy after the last tool step", () => {
  expect(showTrailingLiveChain(true, false)).toBe(true);
});

it("showTrailingLiveChain is false when idle", () => {
  expect(showTrailingLiveChain(false, false)).toBe(false);
});

it("a busy turn renders the live Thinking… step", () => {
  const m = mount(<Transcript {...base} busy items={[user, done("t1")]} />);
  const live = m.container.querySelector('[data-part="live-thinking"]');
  expect(live).not.toBeNull();
  expect(live?.textContent).toContain("Thinking…");
  m.unmount();
});

it("the Chain of Thought is open while busy and collapses when the turn ends", () => {
  const items = [user, done("t1")];
  const m = mount(<Transcript {...base} busy items={items} />);
  expect(chainState(m.container)).toBe("open");
  m.unmount();
  const idle = mount(<Transcript {...base} busy={false} items={items} />);
  expect(chainState(idle.container)).toBe("closed");
  idle.unmount();
});

it("a new message and a new step carry the entry animation class", () => {
  const pending: PendingSubmission = { clientSubmissionId: "c1", text: "next", state: "sending" } as PendingSubmission;
  const m = mount(<Transcript {...base} busy items={[user, done("t1")]} pending={[pending]} />);
  const c = m.container;
  expect(c.querySelector('[data-kind="user"]')?.className).toContain("animate-in");
  expect(c.querySelector('[data-pending] [data-slot="message"], li[data-pending] > *')?.className).toContain("animate-in");
  expect(c.querySelector('[data-kind="chain-of-thought"]')?.className).toContain("animate-in");
  expect(c.querySelector('[data-part="tool-rail"]')?.className).toContain("animate-in");
  m.unmount();
});

interface FakeScroller {
  el: HTMLDivElement;
  content: HTMLDivElement;
  pinned: MutableRefObject<boolean>;
}
function fakeScroller(init: { scrollHeight: number; clientHeight: number; scrollTop: number }): FakeScroller {
  const el = document.createElement("div");
  Object.defineProperty(el, "scrollHeight", { value: init.scrollHeight, writable: true, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: init.clientHeight, writable: true, configurable: true });
  Object.defineProperty(el, "scrollTop", { value: init.scrollTop, writable: true, configurable: true });
  return { el, content: document.createElement("div"), pinned: { current: true } };
}
function mountScroll(f: FakeScroller, first: { items: readonly TranscriptItem[]; pendingCount: number }) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const render = (items: readonly TranscriptItem[], pendingCount: number) =>
    act(() =>
      root.render(
        <MessageScrollerProvider>
          <TranscriptAutoScroll
            tailEpoch={0}
            items={items}
            queuedCount={0}
            pendingCount={pendingCount}
            busy
            viewportRef={{ current: f.el } as RefObject<HTMLDivElement | null>}
            contentRef={{ current: f.content } as RefObject<HTMLDivElement | null>}
            pinnedRef={f.pinned}
          />
        </MessageScrollerProvider>,
      ),
    );
  render(first.items, first.pendingCount);
  return {
    render,
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}
const scrollTo = (f: FakeScroller, top: number) => {
  f.el.scrollTop = top;
  f.el.dispatchEvent(new Event("scroll"));
};

it("TranscriptAutoScroll pins to the bottom on send", () => {
  const f = fakeScroller({ scrollHeight: 2000, clientHeight: 500, scrollTop: 100 });
  const m = mountScroll(f, { items: [], pendingCount: 0 });
  scrollTo(f, 50);
  expect(f.pinned.current).toBe(false);
  m.render([user], 1);
  expect(f.pinned.current).toBe(true);
  expect(f.el.scrollTop).toBe(2000);
  m.unmount();
});

it("TranscriptAutoScroll pins to the bottom when content grows and the user is at the bottom", () => {
  const f = fakeScroller({ scrollHeight: 1000, clientHeight: 500, scrollTop: 500 });
  const m = mountScroll(f, { items: [], pendingCount: 0 });
  scrollTo(f, 500);
  (f.el as { scrollHeight: number }).scrollHeight = 1400;
  act(() => FakeResizeObserver.instances.forEach((o) => o.fire()));
  expect(f.el.scrollTop).toBe(1400);
  m.unmount();
});

it("TranscriptAutoScroll does not pin when the user scrolled up", () => {
  const f = fakeScroller({ scrollHeight: 2000, clientHeight: 500, scrollTop: 1500 });
  const m = mountScroll(f, { items: [], pendingCount: 0 });
  scrollTo(f, 200);
  expect(f.pinned.current).toBe(false);
  (f.el as { scrollHeight: number }).scrollHeight = 2600;
  act(() => FakeResizeObserver.instances.forEach((o) => o.fire()));
  expect(f.el.scrollTop).toBe(200);
  m.unmount();
});
