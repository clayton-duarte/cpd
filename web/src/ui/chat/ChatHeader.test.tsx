// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import { mount } from "../../testing/render.tsx";
import { ChatHeader } from "./ChatHeader.tsx";
import { toneClasses } from "../dango-status.ts";

it("shows only the title", () => {
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix the bug" role="lead" runner="pi" />);
  expect(m.container.querySelector("header h1")?.textContent).toBe("Fix the bug");
  expect(m.container.querySelector("header")?.textContent).toBe("Fix the bug");
  m.unmount();
  const m2 = mount(<ChatHeader sessionKey="lead-1" title={null} />);
  expect(m2.container.querySelector("header h1")?.textContent).toBe("lead-1");
  m2.unmount();
});

it("sets document.title from the session key and title", () => {
  document.title = "before";
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix the bug" />);
  expect(document.title).toBe("lead-1 · Fix the bug");
  act(() => m.unmount());
  const m2 = mount(<ChatHeader sessionKey="lead-2" title={null} />);
  expect(document.title).toBe("lead-2");
  m2.unmount();
});

it("restores the document title on unmount", () => {
  document.title = "original";
  const m = mount(<ChatHeader sessionKey="lead-1" title="T" />);
  expect(document.title).toBe("lead-1 · T");
  m.unmount();
  expect(document.title).toBe("original");
});

const completion = { label: "safe_to_archive", text: "Safe to archive · 93%", score: 93, tone: "done" } as const;

it("lead completion badge renders in the title row, right of the title", () => {
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={completion} />);
  const header = m.container.querySelector("header") as HTMLElement;
  const heading = header.querySelector('div[data-part="heading"]') as HTMLElement;
  const badge = header.querySelector('[data-part="completion-badge"]');
  expect(badge).not.toBeNull();
  expect(heading.querySelector('h1[data-part="title"]')).not.toBeNull();
  expect(heading.contains(badge)).toBe(false);
  expect(heading.nextElementSibling).toBe(badge);
  expect(header.lastElementChild).toBe(badge);
  expect(badge?.textContent).toBe("Safe to archive · 93%");
  m.unmount();
  const mgr = mount(<ChatHeader sessionKey="m-1" title="Fix" role="manager" completion={completion} />);
  expect(mgr.container.querySelector('[data-part="completion-badge"]')).toBeNull();
  mgr.unmount();
  const none = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={null} />);
  expect(none.container.querySelector('[data-part="completion-badge"]')).toBeNull();
  expect(none.container.querySelector("header")?.textContent).toBe("Fix");
  none.unmount();
});

it("completion badge shows the text tone and slot", () => {
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={completion} />);
  const badge = m.container.querySelector('[data-part="completion-badge"]') as HTMLElement | null;
  expect(badge?.dataset["slot"]).toBe("badge");
  expect(badge?.textContent).toBe("Safe to archive · 93%");
  expect(badge?.className).toContain(toneClasses.done.text);
  m.unmount();
});

it("header is a single non-wrapping row", () => {
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={completion} />);
  const header = m.container.querySelector("header") as HTMLElement;
  expect(header.className).toContain("flex");
  expect(header.className).toContain("items-center");
  expect(header.className).not.toContain("flex-wrap");
  const heading = header.querySelector('div[data-part="heading"]') as HTMLElement;
  expect(heading.className).toContain("min-w-0");
  expect(heading.className).toContain("flex-1");
  expect(header.querySelector("h1")?.className).toContain("truncate");
  m.unmount();
});

const hover = async (el: HTMLElement) => {
  await act(async () => { el.focus(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(100); });
};

it("completion badge opens a hover card listing the other scores", async () => {
  vi.useFakeTimers();
  const others = [{ label: "unanswered_ask", score: 24 }, { label: "open_work", score: 10 }] as const;
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={{ ...completion, others }} />);
  const header = m.container.querySelector("header") as HTMLElement;
  const badge = header.querySelector<HTMLElement>('[data-part="completion-badge"]')!;
  expect(header.lastElementChild).toBe(badge);
  expect(badge.textContent).toBe("Safe to archive · 93%");
  expect(badge.className).toContain(toneClasses.done.text);
  expect(document.querySelector('[data-part="completion-scores"]')).toBeNull();
  await hover(badge);
  const card = document.querySelector('[data-part="completion-scores"]');
  expect(card).not.toBeNull();
  const rows = [...card!.querySelectorAll<HTMLElement>('[data-part="completion-other"]')];
  expect(rows.map((r) => r.textContent)).toEqual(["Unanswered ask · 24%", "Open work · 10%"]);
  expect(rows.every((r) => r.dataset["slot"] === "badge")).toBe(true);
  m.unmount();
  vi.useRealTimers();
});

it("completion badge without others has no hover card and keeps the native title", async () => {
  vi.useFakeTimers();
  const title = "kev-4b verdict: Safe to archive, archive-safety 93%";
  for (const c of [completion, { ...completion, others: [] }]) {
    const m = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={c} />);
    const badge = m.container.querySelector<HTMLElement>('[data-part="completion-badge"]')!;
    expect(badge.getAttribute("title")).toBe(title);
    await hover(badge);
    expect(document.querySelector('[data-part="completion-scores"]')).toBeNull();
    m.unmount();
  }
  const m = mount(<ChatHeader sessionKey="lead-1" title="Fix" role="lead" completion={{ ...completion, others: [{ label: "open_work", score: 10 }] }} />);
  expect(m.container.querySelector('[data-part="completion-badge"]')?.hasAttribute("title")).toBe(false);
  m.unmount();
  vi.useRealTimers();
});
