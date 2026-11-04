// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 5 };
const rows: InboxRow[] = [
  {
    ...base,
    id: 4,
    kind: "review-requested",
    label: "Review PR 9\nsecond line",
    urgency: "red",
    sessionId: "s1",
    url: "https://example.test/pr/9",
    actions: [
      { label: "Claim", action: { kind: "inbox.claim", itemId: 4 } },
      { label: "Dismiss", action: { kind: "inbox.dismiss", itemId: 4 } },
    ],
  },
  {
    ...base,
    id: 5,
    kind: "deploy",
    label: "Deploy ready",
    urgency: "yellow",
    sessionId: null,
    actions: [
      { label: "Approve", action: { kind: "inbox.approve", itemId: 5 } },
      { label: "Snooze 15m", action: { kind: "inbox.snooze", itemId: 5, duration: "15m" } },
    ],
  },
];
const inbox: InboxModel = {
  openCount: 2,
  engineRunning: true,
  hidden: { claimed: 0, dismissed: 0, snoozed: 0 },
  rows,
  cursor: 1,
};

function click(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

function buttonByText(root: Element | null, text: string): Element | undefined {
  return Array.from(root?.querySelectorAll("button") ?? []).find((b) => b.textContent === text);
}

it("renders inbox rows by kind", () => {
  const m = mount(<Inbox inbox={inbox} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  const items = m.container.querySelectorAll("li[data-kind]");
  expect(Array.from(items).map((li) => li.getAttribute("data-kind"))).toEqual(["review-requested", "deploy"]);
  const first = items[0];
  expect(first?.getAttribute("data-urgency")).toBe("red");
  expect(first?.querySelector('[data-part="kind"]')?.textContent).toBe("review-requested");
  expect(first?.querySelector("strong")?.textContent).toBe("Review PR 9");
  expect(first?.textContent).toContain("second line");
  expect(first?.querySelector('a[href="https://example.test/pr/9"]')).not.toBeNull();
  expect(first?.querySelector('button[data-action="open-session"]')).not.toBeNull();
  expect(items[1]?.querySelector('button[data-action="open-session"]')).toBeNull();
  m.unmount();
});

it("row actions call onAction with the typed action", () => {
  const onAction = vi.fn();
  const onSelectSession = vi.fn();
  const m = mount(<Inbox inbox={inbox} onAction={onAction} onSelectSession={onSelectSession} />);
  const [first, second] = Array.from(m.container.querySelectorAll("li[data-kind]"));
  click(buttonByText(first ?? null, "Claim"));
  expect(onAction).toHaveBeenLastCalledWith({ kind: "inbox.claim", itemId: 4 });
  click(buttonByText(second ?? null, "Snooze 15m"));
  expect(onAction).toHaveBeenLastCalledWith({ kind: "inbox.snooze", itemId: 5, duration: "15m" });
  expect(onAction).toHaveBeenCalledTimes(2);
  click(first?.querySelector('button[data-action="open-session"]'));
  expect(onSelectSession).toHaveBeenCalledExactlyOnceWith("s1");
  m.unmount();
});

it("shows the open item count", () => {
  const m = mount(<Inbox inbox={inbox} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  expect(m.container.querySelector('aside[aria-label="Inbox"] h2')?.textContent).toBe("Inbox (2)");
  m.unmount();
});
