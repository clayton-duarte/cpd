// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { Attachment, Capabilities, FooterStats, ModelChoice, QueuedSubmission, WhenBusy } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Footer } from "../chat/Footer.tsx";
import { Composer } from "./Composer.tsx";
import type { FileReaderBoundary } from "./attachments.ts";
import { TierSelector } from "./TierSelector.tsx";

const caps: Capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: { supported: true }, followUp: { supported: true }, abort: { supported: true },
  abortSubmission: { supported: true }, compact: { supported: true },
  configure: { model: { supported: true }, thinking: { supported: true }, tier: { supported: true, options: ["S1", "S2"] } },
  dialogs: { mode: "interactive" },
};
const reader: FileReaderBoundary = { readBase64: async () => "QUJD", readText: async () => "txt" };
type Submit = { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy };

function setup(over: { busy?: boolean; queued?: QueuedSubmission[]; settings?: React.ReactNode } = {}) {
  const submits: Submit[] = [];
  const onAbort = vi.fn();
  const m = mount(
    <Composer sessionId="s1" capabilities={caps} busy={over.busy ?? false} queued={over.queued ?? []} reader={reader}
      settings={over.settings}
      onSubmit={(i) => { submits.push(i); return () => ({ dispose() {} }); }} onAbort={onAbort} onCancelQueued={() => {}} />,
  );
  return { m, submits, onAbort };
}
const textarea = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
function typeText(el: HTMLTextAreaElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function press(el: Element, key: string, init: KeyboardEventInit = {}) {
  act(() => { el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init })); });
}
function click(el: Element) {
  act(() => { el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })); });
}

it("composer card shows the queued chips in the prompt header", () => {
  const queued: QueuedSubmission[] = [
    { clientSubmissionId: "c1", submissionId: 1, text: "first", whenBusy: "followUp" },
    { clientSubmissionId: "c2", submissionId: 2, text: "second", whenBusy: "followUp" },
  ];
  const { m } = setup({ queued });
  const header = m.container.querySelector('[data-slot="input-group-addon"][data-align="block-start"]');
  const bar = header?.querySelector('ul[data-part="queue"]');
  expect(bar).not.toBeNull();
  const chips = bar!.querySelectorAll("li[data-queued-chip]");
  expect(chips.length).toBe(2);
  expect(chips[0]!.textContent).toContain("queued");
  expect(chips[0]!.querySelector('[data-part="text"]')?.textContent).toBe("first");
  expect(chips[1]!.querySelector('[data-part="text"]')?.textContent).toBe("second");
  m.unmount();
  const empty = setup();
  expect(empty.m.container.querySelector('[data-part="queue"]')).toBeNull();
  expect(empty.m.container.querySelector('[data-align="block-start"]')).toBeNull();
  empty.m.unmount();
});

it("composer renders the attach button and the tier selector in the footer", () => {
  const settings = <TierSelector capabilities={caps} footer={null} models={null} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />;
  const { m } = setup({ settings });
  const footer = m.container.querySelector('[data-slot="input-group-addon"][data-align="block-end"]');
  const attach = footer?.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]');
  expect(attach).not.toBeNull();
  expect(footer?.querySelector('[data-part="tier-selector"]')).not.toBeNull();
  expect(footer?.querySelector('[data-part="actions"] button[data-action="send"]')).not.toBeNull();
  m.unmount();
});

it("composer renders Stop but no Steer or Follow-up while the agent is running", () => {
  const { m, onAbort } = setup({ busy: true });
  expect(m.container.querySelector('[data-part="send-split"], button[data-when-busy]')).toBeNull();
  expect(m.container.querySelector('[data-part="actions"] button[data-action="abort"]')).not.toBeNull();
  expect(onAbort).not.toHaveBeenCalled();
  m.unmount();
});

it("composer sends a follow-up on Enter, busy or not, and ignores Alt Enter as a mode", () => {
  const { m, submits } = setup({ busy: true });
  const t = textarea(m.container);
  typeText(t, "go");
  press(t, "Enter");
  expect(submits).toEqual([{ text: "go", attachments: [], whenBusy: "followUp" }]);
  press(t, "Enter", { altKey: true });
  expect(submits).toHaveLength(1);
  m.unmount();
});

it("tier selector shows the tier, model and thinking level on three menu triggers", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "vertex/claude-sonnet-5-5" }, tier: { state: "known", value: "S1" }, thinking: { state: "known", value: "high" },
    contextPercent: { state: "unknown", reason: "x" }, cost: { state: "unknown", reason: "x" }, branch: { state: "unknown", reason: "x" },
  };
  const models: ModelChoice[] = [{ model: "vertex/claude-sonnet-5-5", name: "Sonnet", thinkingLevels: ["low", "high"], contextWindow: 1000 }];
  const m = mount(<TierSelector capabilities={caps} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  const root = m.container.querySelector('[data-part="tier-selector"]')!;
  expect(root.querySelector('button[aria-label="Tier"]')?.textContent).toBe("S1");
  expect(root.querySelector('button[aria-label="Model"]')?.textContent).toBe("Sonnet");
  expect(root.querySelector('button[aria-label="Thinking level"]')?.textContent).toBe("high");
  expect(root.querySelector('button[data-part="tier-pill"]')).toBeNull();
  m.unmount();
});

it("footer shows only the context card with its percent and window", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "a/m" }, tier: { state: "known", value: "S1" }, thinking: { state: "known", value: "high" },
    contextPercent: { state: "known", value: 8.8 }, cost: { state: "known", value: 0.698 }, branch: { state: "known", value: "main" },
  };
  const m = mount(<Footer footer={footer} contextWindow={1_000_000} />);
  const f = m.container.querySelector('footer[aria-label="Session footer"]')!;
  expect(f.getAttribute("data-part")).toBe("telemetry");
  expect(f.querySelector('[data-stat="context"]')?.textContent).toBe("8.8% context");
  expect(f.querySelector('[data-part="cwd"], [data-part="tokens"], [data-stat="cost"], [data-stat="model"]')).toBeNull();
  m.unmount();
});
