// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { Attachment, Capabilities, QueuedSubmission, WhenBusy } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Composer } from "./Composer.tsx";
import type { FileReaderBoundary } from "./attachments.ts";

const base: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: { supported: true }, followUp: { supported: true }, abort: { supported: true },
  abortSubmission: { supported: true }, compact: { supported: true },
  configure: { model: { supported: true }, thinking: { supported: true }, tier: { supported: true, options: ["S1"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "txt" };

type Submit = { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy };
function setup(over: { capabilities?: Capabilities | null; busy?: boolean; queued?: QueuedSubmission[] } = {}) {
  const submits: Submit[] = [];
  const onAbort = vi.fn();
  const onCancelQueued = vi.fn();
  const m = mount(
    <Composer capabilities={over.capabilities === undefined ? base : over.capabilities} busy={over.busy ?? false} queued={over.queued ?? []}
      reader={reader} onSubmit={(i) => submits.push(i)} onAbort={onAbort} onCancelQueued={onCancelQueued} />,
  );
  return { m, submits, onAbort, onCancelQueued };
}

const textarea = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
function typeText(el: HTMLTextAreaElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function press(el: Element, key: string) {
  act(() => { el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true })); });
}
function click(el: Element) {
  act(() => { el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })); });
}
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });
const png = { name: "a.png", type: "image/png", size: 3 };

function dragEvent(type: string, files: unknown[]): Event {
  const e = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(e, "dataTransfer", { value: { types: ["Files"], files } });
  return e;
}

it("enter submits text and clears the input", () => {
  const { m, submits } = setup();
  const t = textarea(m.container);
  typeText(t, "hello  ");
  press(t, "Enter");
  expect(submits).toEqual([{ text: "hello", attachments: [], whenBusy: "followUp" }]);
  expect(textarea(m.container).value).toBe("");
  m.unmount();
});

it("while busy the composer offers steer and follow-up", () => {
  const { m, submits } = setup({ busy: true });
  expect(m.container.querySelector('button[data-action="send"]')).toBeNull();
  const steer = m.container.querySelector('button[data-when-busy="steer"]')!;
  expect(m.container.querySelector('button[data-when-busy="followUp"]')).not.toBeNull();
  typeText(textarea(m.container), "go");
  click(steer);
  expect(submits).toEqual([{ text: "go", attachments: [], whenBusy: "steer" }]);
  m.unmount();
});

it("abort button calls onAbort while busy", () => {
  const { m, onAbort } = setup({ busy: true });
  click(m.container.querySelector('button[data-action="abort"]')!);
  expect(onAbort).toHaveBeenCalledTimes(1);
  m.unmount();
});

it("queued submissions are listed with a cancel control", () => {
  const queued: QueuedSubmission[] = [{ clientSubmissionId: "c1", text: "later", whenBusy: "followUp" }];
  const { m, onCancelQueued } = setup({ busy: true, queued });
  const li = m.container.querySelector('ul[aria-label="Queued"] li[data-queued="c1"]')!;
  expect(li.textContent).toContain("later");
  click(li.querySelector('button[aria-label="Cancel queued message"]')!);
  expect(onCancelQueued).toHaveBeenCalledWith("c1");
  m.unmount();
});

it("file picker adds attachment chips", async () => {
  const { m } = setup();
  const input = m.container.querySelector<HTMLInputElement>('input[type="file"][aria-label="Attach files"]')!;
  Object.defineProperty(input, "files", { value: [png], configurable: true });
  act(() => { input.dispatchEvent(new Event("change", { bubbles: true })); });
  await flush();
  const chip = m.container.querySelector('ul[aria-label="Attachments"] li[data-attachment="a.png"]')!;
  expect(chip.getAttribute("data-kind")).toBe("image");
  m.unmount();
});

it("dragging files over the composer shows a drop state and dropping adds attachments", async () => {
  const { m } = setup();
  const form = m.container.querySelector<HTMLFormElement>('form[aria-label="Composer"]')!;
  expect(form.dataset.drop).toBe("idle");
  act(() => { form.dispatchEvent(dragEvent("dragenter", [])); });
  expect(form.dataset.drop).toBe("over");
  const drop = dragEvent("drop", [png]);
  act(() => { form.dispatchEvent(drop); });
  await flush();
  expect(drop.defaultPrevented).toBe(true);
  expect(form.dataset.drop).toBe("idle");
  expect(m.container.querySelector('li[data-attachment="a.png"]')).not.toBeNull();
  m.unmount();
});

it("unsupported capabilities are disabled with their reason", () => {
  const caps: Capabilities = { ...base, steer: { supported: false, reason: "no steer" }, abort: { supported: false, reason: "no abort" } };
  const { m, submits, onAbort } = setup({ capabilities: caps, busy: true });
  const steer = m.container.querySelector<HTMLButtonElement>('button[data-when-busy="steer"]')!;
  const abort = m.container.querySelector<HTMLButtonElement>('button[data-action="abort"]')!;
  expect(steer.disabled).toBe(true);
  expect(steer.title).toBe("no steer");
  expect(abort.disabled).toBe(true);
  expect(abort.title).toBe("no abort");
  const reasons = Array.from(m.container.querySelectorAll("small[data-reason]")).map((s) => s.textContent);
  expect(reasons).toEqual(expect.arrayContaining(["no steer", "no abort"]));
  typeText(textarea(m.container), "x");
  click(steer);
  click(abort);
  expect(submits).toEqual([]);
  expect(onAbort).not.toHaveBeenCalled();
  expect(m.container.querySelector<HTMLButtonElement>('button[data-when-busy="followUp"]')!.disabled).toBe(false);
  m.unmount();
});
