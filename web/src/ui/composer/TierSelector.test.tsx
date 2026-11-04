// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
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

const sel = (c: HTMLElement, label: string) => c.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!;
const opts = (s: HTMLSelectElement) => Array.from(s.options).map((o) => o.value);
function choose(s: HTMLSelectElement, value: string) {
  act(() => {
    const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
    setter.call(s, value);
    s.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

it("lists tiers and models from the session capabilities", () => {
  const m = mount(<TierSelector capabilities={caps()} footer={footer} models={models} busy={false} onConfigure={() => {}} onRequestModels={() => {}} />);
  expect(opts(sel(m.container, "Tier"))).toEqual(["S1", "S2", "S3"]);
  expect(sel(m.container, "Tier").value).toBe("S2");
  expect(opts(sel(m.container, "Model"))).toEqual(["a/m1", "a/m2"]);
  expect(opts(sel(m.container, "Thinking level"))).toEqual(["off", "low", "high"]);
  m.unmount();
});

it("choosing a tier, model or thinking level calls onConfigure", () => {
  const calls: ConfigureChange[] = [];
  const onRequestModels = vi.fn();
  const m = mount(<TierSelector capabilities={caps()} footer={footer} models={models} busy={false} onConfigure={(c) => calls.push(c)} onRequestModels={onRequestModels} />);
  choose(sel(m.container, "Tier"), "S3");
  choose(sel(m.container, "Model"), "a/m2");
  choose(sel(m.container, "Thinking level"), "high");
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
  const model = sel(m.container, "Model");
  expect(model.disabled).toBe(true);
  expect(model.title).toBe("follows tier");
  expect(sel(m.container, "Thinking level").disabled).toBe(true);
  expect(m.container.querySelector("small[data-reason]")?.textContent).toBe("follows tier");
  expect(sel(m.container, "Tier").disabled).toBe(false);
  m.unmount();
});
