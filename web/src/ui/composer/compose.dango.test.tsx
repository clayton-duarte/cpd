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
    <Composer capabilities={caps} busy={over.busy ?? false} queued={over.queued ?? []} reader={reader}
      settings={over.settings} label="chat-research"
      onSubmit={(i) => submits.push(i)} onAbort={onAbort} onCancelQueued={() => {}} />,
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

it("composer card shows the queued follow up chip bar", () => {
  const queued: QueuedSubmission[] = [
    { clientSubmissionId: "c1", text: "first", whenBusy: "followUp" },
    { clientSubmissionId: "c2", text: "second", whenBusy: "steer" },
  ];
  const { m } = setup({ queued });
  const bar = m.container.querySelector('ul[data-part="queued-bar"]');
  expect(bar).not.toBeNull();
  const chips = bar!.querySelectorAll("li[data-queued]");
  expect(chips.length).toBe(2);
  expect(chips[0]!.getAttribute("data-when-busy")).toBe("followUp");
  expect(chips[0]!.querySelector('[data-part="queued-kind"]')?.textContent).toBe("follow-up queued");
  expect(chips[0]!.querySelector('[data-part="queued-text"]')?.textContent).toBe("first");
  expect(chips[1]!.querySelector('[data-part="queued-kind"]')?.textContent).toBe("steer queued");
  m.unmount();
  const empty = setup();
  expect(empty.m.container.querySelector('[data-part="queued-bar"]')).toBeNull();
  empty.m.unmount();
});

it("composer renders the attach button and the tier pill", () => {
  const settings = <TierSelector capabilities={caps} footer={null} models={null} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />;
  const { m } = setup({ settings });
  const attach = m.container.querySelector<HTMLButtonElement>('button[data-action="attach"]');
  expect(attach?.getAttribute("aria-label")).toBe("Choose files to attach");
  const row = m.container.querySelector('[data-part="action-row"]');
  expect(row?.contains(attach)).toBe(true);
  expect(row?.querySelector('button[data-part="tier-pill"]')).not.toBeNull();
  m.unmount();
});

it("composer renders Steer and Follow-up as a split button group", () => {
  const { m } = setup({ busy: true });
  const group = m.container.querySelector('div[role="group"][data-part="send-split"]');
  expect(group?.getAttribute("aria-label")).toBe("Send while the agent is running");
  const buttons = group!.querySelectorAll("button");
  expect(buttons.length).toBe(2);
  expect(buttons[0]!.getAttribute("data-when-busy")).toBe("steer");
  expect(buttons[0]!.textContent).toBe("Steer⏎");
  expect(buttons[1]!.getAttribute("data-when-busy")).toBe("followUp");
  expect(buttons[1]!.textContent).toBe("Follow-up⌥⏎");
  m.unmount();
});

it("composer shows Stop only while the agent is running", () => {
  const idle = setup();
  expect(idle.m.container.querySelector('button[data-action="abort"]')).toBeNull();
  expect(idle.m.container.querySelector('button[data-action="send"]')).not.toBeNull();
  idle.m.unmount();
  const { m, onAbort } = setup({ busy: true });
  const stop = m.container.querySelector<HTMLButtonElement>('button[data-action="abort"]');
  expect(stop?.textContent).toBe("Stop⌃C");
  click(stop!);
  expect(onAbort).toHaveBeenCalledTimes(1);
  m.unmount();
});

it("composer sends Steer on Enter and Follow-up on Alt Enter", () => {
  const { m, submits } = setup({ busy: true });
  const t = textarea(m.container);
  typeText(t, "go");
  press(t, "Enter");
  expect(submits).toEqual([{ text: "go", attachments: [], whenBusy: "steer" }]);
  typeText(t, "later");
  press(t, "Enter", { altKey: true });
  expect(submits[1]).toEqual({ text: "later", attachments: [], whenBusy: "followUp" });
  m.unmount();
});

it("tier selector pill shows tier model and thinking", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "vertex/claude-sonnet-5-5" }, tier: { state: "known", value: "S1" }, thinking: { state: "known", value: "high" },
    contextPercent: { state: "unknown", reason: "x" }, cost: { state: "unknown", reason: "x" }, branch: { state: "unknown", reason: "x" },
  };
  const models: ModelChoice[] = [{ model: "vertex/claude-sonnet-5-5", name: "Sonnet", thinkingLevels: ["low", "high"], contextWindow: 1000 }];
  const m = mount(<TierSelector capabilities={caps} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  const pill = m.container.querySelector('button[data-part="tier-pill"]');
  expect(pill?.getAttribute("aria-haspopup")).toBe("true");
  expect(pill?.getAttribute("aria-expanded")).toBe("false");
  expect(pill?.querySelector('[data-part="pill-tier"]')?.textContent).toBe("S1");
  expect(pill?.querySelector('[data-part="pill-model"]')?.textContent).toBe("claude-sonnet-5-5");
  expect(pill?.querySelector('[data-part="pill-thinking"]')?.textContent).toBe("high");
  expect(pill?.querySelector("svg")).not.toBeNull();
  m.unmount();
});

it("footer shows telemetry and a context percent bar", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "a/m" }, tier: { state: "known", value: "S1" }, thinking: { state: "known", value: "high" },
    contextPercent: { state: "known", value: 8.8 }, cost: { state: "known", value: 0.698 }, branch: { state: "known", value: "main" },
  };
  const m = mount(
    <Footer footer={footer} cwd="~/src/cpd" sessionKey="cpd:chat" contextWindow={1_000_000}
      tokens={{ input: 1_200_000, output: 84_000, cacheRead: 11_800_000, cacheWrite: 412_000 }} cacheHitPercent={99.1} />,
  );
  const f = m.container.querySelector('footer[aria-label="Session footer"]')!;
  expect(f.getAttribute("data-part")).toBe("telemetry");
  expect(f.querySelector('[data-part="cwd"]')?.textContent).toBe("~/src/cpd");
  expect(f.querySelector('[data-part="session-key"]')?.textContent).toBe("cpd:chat");
  expect(f.querySelector('[data-part="tokens"]')?.textContent).toBe("↑1.2M ↓84k R11.8M W412k");
  expect(f.querySelector('[data-part="cache"]')?.textContent).toBe("CH 99.1%");
  expect(f.querySelector('[data-stat="cost"]')?.textContent).toContain("$0.698");
  const bar = f.querySelector('[role="progressbar"][data-part="context-bar"]');
  expect(bar?.getAttribute("aria-label")).toBe("Context used");
  expect(bar?.getAttribute("aria-valuenow")).toBe("8.8");
  expect(f.querySelector('[data-stat="context"]')?.textContent).toContain("8.8% / 1.0M");
  m.unmount();
});
