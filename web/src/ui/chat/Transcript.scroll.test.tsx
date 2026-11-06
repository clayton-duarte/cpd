// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { TranscriptItem } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Transcript } from "./Transcript.tsx";

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

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
});
afterEach(() => vi.unstubAllGlobals());

const viewportOf = (container: HTMLElement): Element => {
  const found = container.querySelectorAll('[data-part="transcript"] [data-part="viewport"]');
  expect(found.length).toBe(1);
  return found[0] as Element;
};

it("the transcript viewport is inside a ScrollArea", () => {
  const m = mount(<Transcript {...base} items={[user, assistant]} />);
  const viewport = viewportOf(m.container);
  expect(viewport.getAttribute("aria-label")).toBe("Conversation transcript");
  expect(viewport.closest('[data-slot="scroll-area"]')).not.toBeNull();
  m.unmount();
});

it("the transcript viewport no longer carries overflow-y-auto", () => {
  const m = mount(<Transcript {...base} items={[user, assistant]} />);
  expect(viewportOf(m.container).className).not.toContain("overflow-y-auto");
  m.unmount();
});
