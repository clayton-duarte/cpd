// @vitest-environment jsdom
import { expect, it } from "vitest";
import type { FooterStats } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { Footer } from "./Footer.tsx";

const stat = (c: HTMLElement, name: string) => c.querySelector(`footer[aria-label="Session footer"] [data-stat="${name}"]`);

it("shows the context percent", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "anthropic/claude-x" },
    tier: { state: "known", value: "S2" },
    thinking: { state: "known", value: "high" },
    contextPercent: { state: "known", value: 42 },
    cost: { state: "known", value: 1.23456 },
    branch: { state: "known", value: "main" },
  };
  const m = mount(<Footer footer={footer} />);
  expect(stat(m.container, "context")?.textContent).toContain("42%");
  expect(m.container.querySelector('[data-state="unknown"]')).toBeNull();
  m.unmount();
});

it("an unknown context percent renders an explicit unknown marker", () => {
  const footer: FooterStats = {
    model: { state: "known", value: "m" },
    tier: { state: "known", value: "S1" },
    thinking: { state: "known", value: "off" },
    contextPercent: { state: "unknown", reason: "no usage yet" },
    cost: { state: "known", value: 0 },
    branch: { state: "known", value: "main" },
  };
  const m = mount(<Footer footer={footer} />);
  const u = stat(m.container, "context")?.querySelector('[data-state="unknown"]');
  expect(u?.textContent).toBe("unknown");
  expect(u?.getAttribute("title")).toBe("no usage yet");
  m.unmount();
  const m2 = mount(<Footer footer={null} />);
  const all = m2.container.querySelectorAll('[data-state="unknown"]');
  expect(all.length).toBe(1);
  expect(all[0]?.getAttribute("title")).toBe("not attached");
  m2.unmount();
});
