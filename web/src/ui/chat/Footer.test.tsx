// @vitest-environment jsdom
import { expect, it } from "vitest";
import type { FooterStats } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Footer } from "./Footer.tsx";

const stat = (c: HTMLElement, name: string) => c.querySelector(`footer[aria-label="Session footer"] [data-stat="${name}"]`);

it("shows model, tier, thinking, context percent, cost and branch", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "anthropic/claude-x" },
    tier: { state: "known", value: "S2" },
    thinking: { state: "known", value: "high" },
    contextPercent: { state: "known", value: 42 },
    cost: { state: "known", value: 1.23456 },
    branch: { state: "known", value: "main" },
  };
  const m = mount(<Footer footer={footer} />);
  expect(stat(m.container, "model")?.textContent).toContain("anthropic/claude-x");
  expect(stat(m.container, "tier")?.textContent).toContain("S2");
  expect(stat(m.container, "thinking")?.textContent).toContain("high");
  expect(stat(m.container, "context")?.textContent).toContain("42%");
  expect(stat(m.container, "cost")?.textContent).toContain("$1.235");
  expect(stat(m.container, "branch")?.textContent).toContain("main");
  expect(m.container.querySelector('[data-state="unknown"]')).toBeNull();
  m.unmount();
});

it("unknown stats render an explicit unknown marker", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "m" },
    tier: { state: "unknown", reason: "no tier" },
    thinking: { state: "known", value: "off" },
    contextPercent: { state: "unknown", reason: "no usage yet" },
    cost: { state: "known", value: 0 },
    branch: { state: "unknown", reason: "not a git repo" },
  };
  const m = mount(<Footer footer={footer} />);
  const u = stat(m.container, "context")?.querySelector('[data-state="unknown"]');
  expect(u?.textContent).toBe("unknown");
  expect(u?.getAttribute("title")).toBe("no usage yet");
  expect(stat(m.container, "branch")?.querySelector('[data-state="unknown"]')?.getAttribute("title")).toBe("not a git repo");
  expect(stat(m.container, "thinking")?.textContent).toContain("off");
  m.unmount();
  const m2 = mount(<Footer footer={null} />);
  const all = m2.container.querySelectorAll('[data-state="unknown"]');
  expect(all.length).toBe(6);
  expect(all[0]?.getAttribute("title")).toBe("not attached");
  m2.unmount();
});
