// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
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
    <Composer sessionId="s1" capabilities={over.capabilities === undefined ? base : over.capabilities} busy={over.busy ?? false} queued={over.queued ?? []}
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
  typeText(t, "hello");
  press(t, "Enter");
  expect(submits).toEqual([{ text: "hello", attachments: [], whenBusy: "followUp" }]);
  expect(textarea(m.container).value).toBe("");
  m.unmount();
});

it("shift enter does not submit and enter is ignored while composing", () => {
  const { m, submits } = setup();
  const t = textarea(m.container);
  typeText(t, "hello");
  act(() => { t.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true })); });
  act(() => { t.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", isComposing: true, bubbles: true, cancelable: true })); });
  expect(submits).toEqual([]);
  m.unmount();
});

it("while busy the send button still submits a follow-up", () => {
  const { m, submits } = setup({ busy: true });
  expect(m.container.querySelector("button[data-when-busy]")).toBeNull();
  typeText(textarea(m.container), "go");
  click(m.container.querySelector('button[data-action="send"]')!);
  expect(submits).toEqual([{ text: "go", attachments: [], whenBusy: "followUp" }]);
  m.unmount();
});

const stop = (c: HTMLElement) => c.querySelector<HTMLButtonElement>('[data-part="actions"] button[data-action="abort"]');

it("Composer renders an enabled Stop button that calls onAbort while busy and abort is supported", () => {
  const { m, onAbort } = setup({ busy: true });
  const b = stop(m.container);
  expect(b).not.toBeNull();
  expect(b!.disabled).toBe(false);
  expect(b!.getAttribute("aria-label")).toBe("Stop");
  click(b!);
  expect(onAbort).toHaveBeenCalledTimes(1);
  m.unmount();
});

it("Composer renders no Stop button when not busy", () => {
  const { m } = setup({ busy: false });
  expect(stop(m.container)).toBeNull();
  m.unmount();
});

it("Composer renders no Stop button when abort is unsupported", () => {
  const { m } = setup({ busy: true, capabilities: { ...base, abort: { supported: false, reason: "unsupported" } } });
  expect(stop(m.container)).toBeNull();
  m.unmount();
});

it("queued submissions are listed with a cancel control", () => {
  const queued: QueuedSubmission[] = [{ clientSubmissionId: "c1", submissionId: 1, text: "later", whenBusy: "followUp" }];
  const { m, onCancelQueued } = setup({ busy: true, queued });
  const li = m.container.querySelector('ul[aria-label="Queued submissions"] li[data-queued-chip="c1"]')!;
  expect(li.textContent).toContain("later");
  click(li.querySelector('button[data-action="cancel-queued"]')!);
  expect(onCancelQueued).toHaveBeenCalledWith("c1");
  m.unmount();
});

it("file picker adds attachment chips", async () => {
  const { m } = setup();
  const input = m.container.querySelector<HTMLInputElement>('input[type="file"][aria-label="Attach files"]')!;
  Object.defineProperty(input, "files", { value: [png], configurable: true });
  act(() => { input.dispatchEvent(new Event("change", { bubbles: true })); });
  await flush();
  const chip = m.container.querySelector('[aria-label="Staged attachments"] [data-attachment="image"]')!;
  expect(chip.textContent).toContain("a.png");
  click(chip.querySelector('button[data-action="remove-attachment"]')!);
  expect(m.container.querySelector('[data-attachment="image"]')).toBeNull();
  m.unmount();
});

it("the attach button opens the hidden file picker", () => {
  const { m } = setup();
  const input = m.container.querySelector<HTMLInputElement>('input[type="file"][aria-label="Attach files"]')!;
  const open = vi.spyOn(input, "click").mockImplementation(() => {});
  click(m.container.querySelector('button[aria-label="Attach files"]')!);
  expect(open).toHaveBeenCalledTimes(1);
  m.unmount();
});

it("dragging files over the composer shows a drop state and the drop cue, and a drop adds nothing itself", async () => {
  const { m } = setup();
  const group = m.container.querySelector<HTMLElement>('[data-slot="input-group"]')!;
  expect(group.dataset.drop).toBe("idle");
  act(() => { group.dispatchEvent(dragEvent("dragenter", [])); });
  expect(group.dataset.drop).toBe("over");
  expect(m.container.querySelector('span[data-part="drop-cue"]')?.textContent).toBe("Drop to attach");
  act(() => { group.dispatchEvent(dragEvent("drop", [png])); });
  await flush();
  expect(group.dataset.drop).toBe("idle");
  expect(m.container.querySelector('[data-part="drop-cue"]')).toBeNull();
  expect(m.container.querySelector("[data-attachment]"), "the chat dropzone adds dropped files, once").toBeNull();
  m.unmount();
});

it("without capabilities the textarea, attach button and send button are disabled", () => {
  const { m, submits } = setup({ capabilities: null });
  expect(textarea(m.container).disabled).toBe(true);
  expect(m.container.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]')!.disabled).toBe(true);
  expect(m.container.querySelector<HTMLButtonElement>('button[data-action="send"]')!.disabled).toBe(true);
  expect(m.container.querySelector<HTMLInputElement>('input[type="file"]')!.disabled).toBe(true);
  expect(submits).toEqual([]);
  m.unmount();
});

it("archived Composer is disabled with the archived placeholder", () => {
  const m = mount(
    <Composer sessionId="s1" capabilities={base} archived busy={false} queued={[]} reader={reader}
      onSubmit={() => {}} onAbort={() => {}} onCancelQueued={() => {}} />,
  );
  const t = textarea(m.container);
  expect(t.disabled).toBe(true);
  expect(t.placeholder).toMatch(/archived/i);
  expect(t.placeholder).not.toContain("Enter to send");
  const attach = m.container.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]')!;
  expect(attach.disabled).toBe(true);
  expect(m.container.querySelector<HTMLInputElement>('input[type="file"]')!.disabled).toBe(true);
  expect(m.container.querySelector<HTMLButtonElement>('[data-action="send"]')!.disabled).toBe(true);
  m.unmount();
});

// ---- persisted draft ----
const KEY = (id: string) => `cpd-ui:draft:${id}`;
beforeEach(() => { window.localStorage.clear(); });
afterEach(() => { vi.restoreAllMocks(); window.localStorage.clear(); });

it("draft: typing saves the text and a remount restores it", () => {
  const first = setup();
  typeText(textarea(first.m.container), "hello");
  expect(window.localStorage.getItem(KEY("s1"))).toBe("hello");
  first.m.unmount();
  const second = setup();
  expect(textarea(second.m.container).value).toBe("hello");
  second.m.unmount();
});

it("draft: switching session shows that session draft and keeps the other", () => {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const render = (id: string) => act(() => root.render(
    <Composer sessionId={id} capabilities={base} busy={false} queued={[]} reader={reader}
      onSubmit={() => {}} onAbort={() => {}} onCancelQueued={() => {}} />,
  ));
  render("s1");
  typeText(textarea(container), "alpha");
  render("s2");
  expect(textarea(container).value).toBe("");
  typeText(textarea(container), "beta");
  render("s1");
  expect(textarea(container).value).toBe("alpha");
  expect(window.localStorage.getItem(KEY("s1"))).toBe("alpha");
  expect(window.localStorage.getItem(KEY("s2"))).toBe("beta");
  act(() => root.unmount());
  container.remove();
});

it("draft: sending clears the stored draft", () => {
  const { m, submits } = setup();
  const t = textarea(m.container);
  typeText(t, "hello");
  expect(window.localStorage.getItem(KEY("s1"))).toBe("hello");
  press(t, "Enter");
  expect(submits).toHaveLength(1);
  expect(window.localStorage.getItem(KEY("s1"))).toBeNull();
  expect(textarea(m.container).value).toBe("");
  m.unmount();
});

it("draft: emptying the input removes the stored draft", () => {
  const { m } = setup();
  const t = textarea(m.container);
  typeText(t, "hello");
  expect(window.localStorage.getItem(KEY("s1"))).toBe("hello");
  typeText(t, "");
  expect(window.localStorage.getItem(KEY("s1"))).toBeNull();
  m.unmount();
});

it("draft: unavailable storage does not break typing", () => {
  const boom = () => { throw new Error("storage unavailable"); };
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(boom);
  const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(boom);
  const { m, submits } = setup();
  const t = textarea(m.container);
  typeText(t, "hello");
  expect(textarea(m.container).value).toBe("hello");
  expect(setItem).toHaveBeenCalled();
  press(t, "Enter");
  expect(submits).toEqual([{ text: "hello", attachments: [], whenBusy: "followUp" }]);
  m.unmount();
});
