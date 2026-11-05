// @vitest-environment jsdom
// Red tests for the consolidated composer (DESIGN-compose 1; pro-d54, pro-d57, pro-d58): the
// shadcn InputGroup in the PromptInput slot layout, one Send path (follow-up), a busy pulse.
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { Attachment, Capabilities, QueuedSubmission, WhenBusy } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Composer } from "./Composer.tsx";
import type { FileReaderBoundary } from "./attachments.ts";

const caps: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: { supported: true }, followUp: { supported: true }, abort: { supported: true },
  abortSubmission: { supported: true }, compact: { supported: true },
  configure: { model: { supported: true }, thinking: { supported: true }, tier: { supported: true, options: ["S1"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "txt" };
const queued: QueuedSubmission[] = [{ clientSubmissionId: "q1", submissionId: "s1", text: "later", whenBusy: "followUp" }];

type Submit = { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy };
function setup(over: { busy?: boolean; queued?: QueuedSubmission[] } = {}) {
  const submits: Submit[] = [];
  const m = mount(
    <Composer capabilities={caps} busy={over.busy ?? false} queued={over.queued ?? []} reader={reader}
      onSubmit={(i) => submits.push(i)} onAbort={vi.fn()} onCancelQueued={vi.fn()} settings={<span data-part="tier-selector" />} />,
  );
  return { m, submits };
}
const textarea = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
function typeText(el: HTMLTextAreaElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const group = (c: HTMLElement) => c.querySelector<HTMLElement>('[data-slot="input-group"]');

it("composer renders the prompt input header, body and footer", () => {
  const { m } = setup({ queued });
  const g = group(m.container);
  expect(g, "the prompt is a shadcn InputGroup").not.toBeNull();
  expect(m.container.querySelector('[data-part="composer"]')?.contains(g)).toBe(true);
  const header = g!.querySelector('[data-slot="input-group-addon"][data-align="block-start"]');
  const footer = g!.querySelector('[data-slot="input-group-addon"][data-align="block-end"]');
  const body = textarea(m.container);
  expect(header?.querySelector('ul[aria-label="Queued submissions"] li[data-queued-chip="q1"]')?.textContent).toContain("later");
  expect(footer?.querySelector('button[aria-label="Attach files"]')).not.toBeNull();
  expect(footer?.querySelector('[data-part="tier-selector"]')).not.toBeNull();
  expect(footer?.querySelector('button[data-action="send"]')).not.toBeNull();
  const order = [header, body, footer].map((e) => e as Node);
  expect(order[0]!.compareDocumentPosition(order[1]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(order[1]!.compareDocumentPosition(order[2]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  m.unmount();
});

it("composer pulses the prompt border while the session is busy", () => {
  const busy = setup({ busy: true });
  expect(group(busy.m.container)?.classList.contains("prompt-border-pulse")).toBe(true);
  expect(group(busy.m.container)?.hasAttribute("data-busy")).toBe(true);
  busy.m.unmount();
  const idle = setup({ busy: false });
  expect(group(idle.m.container)).not.toBeNull();
  expect(group(idle.m.container)?.classList.contains("prompt-border-pulse")).toBe(false);
  expect(group(idle.m.container)?.hasAttribute("data-busy")).toBe(false);
  idle.m.unmount();
});

it("composer keeps send enabled and submits as follow-up", () => {
  const { m, submits } = setup({ busy: true });
  const send = () => m.container.querySelector<HTMLButtonElement>('button[data-action="send"]');
  expect(send(), "Send exists while busy").not.toBeNull();
  expect(send()!.disabled, "nothing to send yet").toBe(true);
  typeText(textarea(m.container), "go on");
  expect(send()!.disabled, "busy never disables Send").toBe(false);
  act(() => send()!.click());
  expect(submits).toEqual([{ text: "go on", attachments: [], whenBusy: "followUp" }]);
  typeText(textarea(m.container), "again");
  act(() => { textarea(m.container).dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
  expect(submits[1]).toEqual({ text: "again", attachments: [], whenBusy: "followUp" });
  m.unmount();
});

it("composer has no steer, follow-up or stop buttons", () => {
  const { m } = setup({ busy: true });
  expect(m.container.querySelector("button[data-when-busy]")).toBeNull();
  expect(m.container.querySelector('button[data-action="abort"]')).toBeNull();
  expect(m.container.querySelector('[data-part="send-split"]')).toBeNull();
  const labels = [...m.container.querySelectorAll("button")].map((b) => b.textContent ?? "");
  expect(labels.some((t) => /^(Steer|Follow-up|Stop)/.test(t))).toBe(false);
  expect(m.container.querySelector('button[data-action="send"]'), "Send is the only send control").not.toBeNull();
  m.unmount();
});
