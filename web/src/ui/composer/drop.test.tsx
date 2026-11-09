// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Capabilities } from "../../protocol/domain.ts";
import type { Connection, SubmitInput } from "../../state/connection.ts";
import { emptyTranscript } from "../../state/transcript.ts";
import type { SessionView } from "../../state/store.ts";
import { mount } from "../../testing/render.tsx";
import { ChatPane } from "../ChatPane.tsx";
import type { FileReaderBoundary } from "./attachments.ts";

vi.mock("@base-ui/react/scroll-area", () => import("../../testing/scroll-area-mock.tsx"));

// jsdom has no ResizeObserver, which ChatPane's virtualised transcript (external) may need: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const yes = { supported: true } as const;
const capabilities: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: yes, followUp: yes, abort: yes, abortSubmission: yes, compact: yes,
  configure: { model: yes, thinking: yes, tier: { supported: true, options: ["S1", "S2"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "text" };

const view: SessionView = {
  id: "s1", attached: true,
  ref: { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" },
  title: "My chat", footer: null, capabilities, transcript: emptyTranscript, busy: false,
  queued: [], pending: [], dialog: null, models: null, error: null,
};

class FakeConnection implements Connection {
  submits: SubmitInput[] = [];
  start(): void {}
  selectSession(): void {}
  selectPlan(): void {}
  loadEarlier(): void {}
  submit(input: SubmitInput): string { this.submits.push(input); return "c1"; }
  observeSubmission(): { dispose(): void } { return { dispose() {} }; }
  abort(): void {}
  abortSubmission(): void {}
  compact(): void {}
  configure(): void {}
  listModels(): void {}
  respond(): void {}
  action(): string { return "a1"; }
  readLogs(): void {}
  readSettings(): void {}
  saveSettings(): void {}
  close(): void {}
}

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function setup() {
  const connection = new FakeConnection();
  const m = mount(<ChatPane view={view} connection={connection} reader={reader} />);
  mounted = m;
  return { connection, container: m.container };
}

const textarea = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
const png = (name: string) => ({ name, type: "image/png", size: 3 });

async function dropOn(el: Element, files: readonly object[]) {
  const drop = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(drop, "dataTransfer", { value: { files, types: ["Files"] } });
  await act(async () => { el.dispatchEvent(drop); });
}
function dragEnter(el: Element) {
  const ev = new Event("dragenter", { bubbles: true, cancelable: true });
  Object.defineProperty(ev, "dataTransfer", { value: { files: [], types: ["Files"] } });
  act(() => { el.dispatchEvent(ev); });
}
function typeAndEnter(c: HTMLElement, value: string) {
  const el = textarea(c);
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  act(() => { el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
}

it("dropping a file on the composer attaches it once", async () => {
  const { container } = setup();
  await dropOn(textarea(container), [png("a.png")]);
  expect(container.querySelectorAll('[data-part="attachments"] [data-attachment]')).toHaveLength(1);
  expect(container.querySelectorAll('[data-part="attachments"] [data-part="name"]')).toHaveLength(1);
});

it("dropping three images sends one submit with three images", async () => {
  const { connection, container } = setup();
  await dropOn(textarea(container), [png("a.png"), png("b.png"), png("c.png")]);
  expect(container.querySelectorAll("[data-part=\"attachments\"] [data-attachment]")).toHaveLength(3);
  typeAndEnter(container, "look");
  expect(connection.submits).toHaveLength(1);
  expect(connection.submits[0]!.images).toHaveLength(3);
});

it("dropping an image into the composer focuses the composer", async () => {
  const { container } = setup();
  await dropOn(textarea(container), [png("a.png")]);
  expect(document.activeElement).toBe(textarea(container));
});

it("dropping one image sends it once in the submit", async () => {
  const { connection, container } = setup();
  await dropOn(textarea(container), [png("a.png")]);
  typeAndEnter(container, "look");
  expect(connection.submits).toHaveLength(1);
  expect(connection.submits[0]!.images).toEqual([{ mimeType: "image/png", dataBase64: "QUJD" }]);
});

it("an image attachment shows a thumbnail with its file name and a remove button", async () => {
  const { container } = setup();
  await dropOn(textarea(container), [png("pic.png")]);
  const item = container.querySelector('[aria-label="Staged attachments"] [data-attachment="image"]')!;
  expect(item).not.toBeNull();
  const img = item.querySelector<HTMLImageElement>('[data-part="thumb"] img')!;
  expect(img.getAttribute("src")).toBe("data:image/png;base64,QUJD");
  expect(item.querySelector('[data-part="name"]')?.textContent).toBe("pic.png");
  const remove = item.querySelector<HTMLButtonElement>('button[aria-label="Remove pic.png"][data-action="remove-attachment"]')!;
  expect(remove).not.toBeNull();
  act(() => remove.click());
  expect(container.querySelector('[data-attachment]')).toBeNull();
});

it("dragging files over the composer shows the drop cue", () => {
  const { container } = setup();
  expect(container.querySelector('span[data-part="drop-cue"]')).toBeNull();
  dragEnter(textarea(container));
  const group = container.querySelector('[data-slot="input-group"]')!;
  expect(group.getAttribute("data-drop")).toBe("over");
  expect(group.querySelector('span[data-part="drop-cue"]')?.textContent).toBe("Drop to attach");
});

it("a rejected file is listed with its reason in a destructive alert", async () => {
  const { container } = setup();
  await dropOn(textarea(container), [{ name: "tool.bin", type: "application/zip", size: 3 }]);
  const alert = container.querySelector<HTMLElement>('ul[role="alert"][data-part="rejections"]')!;
  expect(alert).not.toBeNull();
  const items = alert.querySelectorAll("li[data-rejection]");
  expect(items).toHaveLength(1);
  expect(items[0]!.className).toContain("text-destructive");
  expect(items[0]!.textContent).toContain("tool.bin: Binary files other than png, jpeg, gif and webp images cannot be attached");
});
