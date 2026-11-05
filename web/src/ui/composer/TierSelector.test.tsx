// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Capabilities, ConfigureChange, FooterStats, ModelChoice } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { TierSelector } from "./TierSelector.tsx";

const caps = (over: Partial<Capabilities["configure"]> = {}): Capabilities => ({
  images: { supported: true, maxCount: 4, maxTotalBase64: 786_432 },
  steer: { supported: true }, followUp: { supported: true }, abort: { supported: true },
  abortSubmission: { supported: true }, compact: { supported: true },
  configure: { model: { supported: true }, thinking: { supported: true }, tier: { supported: true, options: ["S1", "S2", "S3"] }, ...over },
  dialogs: { mode: "interactive" },
});
const footer: FooterStats = {
  model: { state: "known", value: "a/m1" }, tier: { state: "known", value: "S2" }, thinking: { state: "known", value: "low" },
  contextPercent: { state: "unknown", reason: "x" }, cost: { state: "unknown", reason: "x" }, branch: { state: "unknown", reason: "x" },
};
const models: ModelChoice[] = [
  { model: "a/m1", name: "Model One", thinkingLevels: ["off", "low", "high"], contextWindow: 1000 },
  { model: "a/m2", name: "Model Two", thinkingLevels: ["off"], contextWindow: 1000 },
];

const trigger = (c: HTMLElement, label: string) => c.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
// Radix opens a dropdown menu on a primary-button pointerdown; its content is portalled into the body.
function open(c: HTMLElement, label: string) {
  act(() => { trigger(c, label).dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true, button: 0, ctrlKey: false })); });
}
const items = () => Array.from(document.querySelectorAll<HTMLElement>('[role="menuitemradio"]'));
const item = (text: string) => items().find((e) => e.textContent === text)!;
function click(el: Element) { act(() => { (el as HTMLElement).click(); }); }

afterEach(() => { document.body.innerHTML = ""; });

it("lists tiers and models from the session capabilities", () => {
  const m = mount(<TierSelector capabilities={caps()} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  expect(trigger(m.container, "Tier").textContent).toBe("S2");
  expect(trigger(m.container, "Model").textContent).toBe("Model One");
  open(m.container, "Tier");
  expect(items().map((e) => e.textContent)).toEqual(["S1", "S2", "S3"]);
  expect(item("S2").getAttribute("aria-checked")).toBe("true");
  m.unmount();
  const m2 = mount(<TierSelector capabilities={caps()} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  open(m2.container, "Model");
  expect(items().map((e) => e.textContent)).toEqual(["Model One", "Model Two"]);
  m2.unmount();
  const m3 = mount(<TierSelector capabilities={caps()} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  open(m3.container, "Thinking level");
  expect(items().map((e) => e.textContent)).toEqual(["off", "low", "high"]);
  m3.unmount();
});

it("choosing a tier, model or thinking level calls onConfigure", () => {
  const calls: ConfigureChange[] = [];
  const onRequestModels = vi.fn();
  const m = mount(<TierSelector capabilities={caps()} footer={footer} models={models} busy={false} onConfigure={(c) => calls.push(c)} onRequestModels={onRequestModels} />);
  open(m.container, "Tier");
  click(item("S3"));
  open(m.container, "Model");
  expect(onRequestModels).toHaveBeenCalledTimes(1);
  click(item("Model Two"));
  open(m.container, "Thinking level");
  click(item("high"));
  expect(calls).toEqual([
    { kind: "tier", tier: "S3" },
    { kind: "model", model: "a/m2" },
    { kind: "thinking", level: "high" },
  ]);
  m.unmount();
});

it("unsupported controls are disabled with a reason", () => {
  const c = caps({ model: { supported: false, reason: "follows tier" }, thinking: { supported: false, reason: "follows tier" } });
  const m = mount(<TierSelector capabilities={c} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  const model = trigger(m.container, "Model");
  expect(model.disabled).toBe(true);
  expect(model.title).toBe("follows tier");
  expect(trigger(m.container, "Thinking level").disabled).toBe(true);
  expect(trigger(m.container, "Tier").disabled).toBe(false);
  m.unmount();
});
