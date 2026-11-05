// @vitest-environment jsdom
// Red tests for the consolidated chat header and footer (DESIGN-chat 1, DESIGN-compose 3; pro-d56,
// pro-d57, pro-d60): the header is the title alone; the footer is one Context HoverCard + Progress.
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { FooterStats } from "../../protocol/domain.ts";
import { mount } from "../../testing/render.tsx";
import { ChatHeader } from "./ChatHeader.tsx";
import { Footer } from "./Footer.tsx";

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

it("chat header shows only the title without breadcrumbs", () => {
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix the bug" role="lead" runner="pi" />);
  const header = m.container.querySelector("header")!;
  expect(header.querySelector("h1")?.textContent).toBe("Fix the bug");
  expect(header.textContent).toBe("Fix the bug");
  expect(header.querySelector('[data-part="subtitle"], [data-part="key"], [data-part="role"], [data-part="runner"]')).toBeNull();
  expect(header.querySelector('nav, [data-slot="breadcrumb"]')).toBeNull();
  m.unmount();
  const untitled = mount(<ChatHeader sessionKey="lead-1" title={null} />);
  expect(untitled.container.querySelector("header h1")?.textContent, "the session key stands in for a missing title").toBe("lead-1");
  untitled.unmount();
});

it("chat header has no thinking or tools toggle buttons", () => {
  // The toggle props are gone from the header's type; a caller that still passes them gets no buttons.
  const legacy = { onToggleThinking: () => {}, onToggleTools: () => {}, thinking: "expanded", tools: "expanded" } as Record<string, unknown>;
  const m = mount(<ChatHeader sessionKey="lead-1" title="T" {...legacy} />);
  expect(m.container.querySelector('button[data-action="toggle-thinking"], button[data-action="toggle-tools"]')).toBeNull();
  expect(m.container.querySelector('[data-part="toggles"]')).toBeNull();
  expect(m.container.querySelectorAll("header button").length).toBe(0);
  m.unmount();
});

const known: FooterStats = {
  model: { state: "known", value: "anthropic/claude-x" },
  tier: { state: "known", value: "S2" },
  thinking: { state: "known", value: "high" },
  contextPercent: { state: "known", value: 42 },
  cost: { state: "known", value: 1.5 },
  branch: { state: "known", value: "main" },
};

it("footer shows context usage in a hover card with a progress bar", async () => {
  vi.useFakeTimers();
  const m = mount(<Footer footer={known} contextWindow={200_000} />);
  const trigger = m.container.querySelector<HTMLElement>('footer[aria-label="Session footer"] [data-slot="hover-card-trigger"][data-stat="context"]');
  expect(trigger, "the context button is the hover card trigger").not.toBeNull();
  expect(trigger?.textContent).toBe("42% context");
  expect(document.querySelector('[data-slot="hover-card-content"]'), "closed until hovered").toBeNull();
  await act(async () => { trigger!.focus(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(100); });
  const card = document.querySelector('[data-slot="hover-card-content"]');
  expect(card, "the card opens on focus").not.toBeNull();
  expect(card?.textContent).toContain("42% / 200.0k");
  const bar = card?.querySelector('[data-slot="progress"][data-part="context-bar"]');
  expect(bar?.getAttribute("role")).toBe("progressbar");
  expect(bar?.querySelector<HTMLElement>('[data-slot="progress-indicator"]')?.style.transform, "the bar is filled to 42%").toBe("translateX(-58%)");
  expect(bar?.getAttribute("aria-label")).toBe("Context used");
  m.unmount();
});

it("footer has no session telemetry row", () => {
  const m = mount(<Footer footer={known} cwd="/work" sessionKey="lead-1" contextWindow={200_000} />);
  const footer = m.container.querySelector('footer[aria-label="Session footer"]')!;
  expect(footer.querySelector('[data-stat="context"]')).not.toBeNull();
  for (const stat of ["model", "tier", "thinking", "cost", "branch"]) {
    expect(footer.querySelector(`[data-stat="${stat}"]`), stat).toBeNull();
  }
  for (const part of ["cwd", "session-key", "tokens", "cache"]) {
    expect(footer.querySelector(`[data-part="${part}"]`), part).toBeNull();
  }
  expect(footer.textContent).not.toContain("anthropic/claude-x");
  expect(footer.querySelectorAll("button").length, "the context card is the whole footer").toBe(1);
  m.unmount();
});
