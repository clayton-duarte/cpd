// @vitest-environment jsdom
import { act } from "react";
import { expect, it, vi } from "vitest";
import type { InboxModel, InboxRow, Urgency } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 5 };

function row(id: number, urgency: Urgency, over: Partial<InboxRow> = {}): InboxRow {
  return { ...base, id, kind: "k", label: `Row ${id}`, urgency, sessionId: `s${id}`, actions: [], ...over };
}

function model(rows: InboxRow[]): InboxModel {
  return { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 0 };
}

function render(rows: InboxRow[]) {
  const onAction = vi.fn();
  const onSelectSession = vi.fn();
  const m = mount(<Inbox inbox={model(rows)} onAction={onAction} onSelectSession={onSelectSession} />);
  return { ...m, onAction, onSelectSession };
}

function key(el: Element | null | undefined, k: string): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
  });
}

function click(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

const mixed = () => [row(1, "red"), row(2, "red"), row(3, "magenta"), row(4, "yellow")];

it("inbox shows the total and per urgency counts", () => {
  const m = render([row(1, "red"), row(2, "red"), row(3, "yellow"), row(4, "magenta")]);
  expect(m.container.querySelector('[data-part="total"]')?.textContent).toBe("4");
  const counts = Array.from(m.container.querySelectorAll('[data-part="count"]'));
  expect(counts.map((c) => c.getAttribute("data-urgency"))).toEqual(["red", "yellow", "blue", "magenta", "cyan", "white"]);
  expect(counts.map((c) => c.getAttribute("data-tier"))).toEqual(["1", "2", "3", "4", "5", "6"]);
  expect(counts.map((c) => c.textContent)).toEqual(["2", "1", "0", "1", "0", "0"]);
  expect(counts[0]?.getAttribute("title")).toBe("2 blocking");
  m.unmount();
});

it("inbox groups rows under urgency tiers in order", () => {
  const m = render([row(1, "white"), row(2, "cyan"), row(3, "magenta"), row(4, "blue"), row(5, "yellow"), row(6, "red")]);
  const groups = Array.from(m.container.querySelectorAll('section[data-part="group"]'));
  expect(groups.map((g) => g.querySelector('[data-part="group-label"]')?.textContent)).toEqual([
    "blocking",
    "delivery",
    "review",
    "decide",
    "in flight",
    "unread",
  ]);
  expect(groups.map((g) => g.getAttribute("data-urgency"))).toEqual(["red", "yellow", "blue", "magenta", "cyan", "white"]);
  expect(groups.map((g) => g.querySelector("li[data-kind]")?.getAttribute("data-id"))).toEqual(["6", "5", "4", "3", "2", "1"]);
  m.unmount();
});

it("inbox rows render inline actions and dispatch them", () => {
  const approve = { kind: "inbox.approve", itemId: 7 } as const;
  const m = render([row(7, "yellow", { actions: [{ label: "Approve deploy", action: approve }] })]);
  const inline = m.container.querySelector('[data-part="actions"]');
  const btn = Array.from(inline?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Approve deploy");
  click(btn);
  expect(m.onAction).toHaveBeenCalledTimes(1);
  expect(m.onAction).toHaveBeenCalledWith(approve);
  m.unmount();
});

it("inbox focused row shows the action bar", () => {
  const m = render(mixed());
  const bars = m.container.querySelectorAll('[data-part="bar"]');
  expect(bars.length).toBe(1);
  const focused = m.container.querySelector('li[data-focused="true"]');
  expect(focused?.getAttribute("data-id")).toBe("1");
  const bar = focused?.querySelector('[data-part="bar"]');
  expect(bar?.getAttribute("role")).toBe("group");
  expect(bar?.getAttribute("aria-label")).toBe("Row actions");
  expect(Array.from(bar?.querySelectorAll("button") ?? []).map((b) => b.textContent)).toEqual(["Open ⏎", "Snooze S", "Dismiss E"]);
  expect(m.container.querySelectorAll('li[data-focused="false"] [data-part="bar"]').length).toBe(0);
  m.unmount();
});

it("inbox moves focus with J and K and opens with Enter", () => {
  const m = render(mixed());
  const focusedId = () => m.container.querySelector('li[data-focused="true"]')?.getAttribute("data-id");
  const lis = () => Array.from(m.container.querySelectorAll("li[data-kind]"));
  expect(focusedId()).toBe("1");
  key(lis()[0], "j");
  expect(focusedId()).toBe("2");
  key(lis()[1], "j");
  expect(focusedId()).toBe("4"); // display order: red, red, yellow, magenta
  key(lis()[2], "k");
  expect(focusedId()).toBe("2");
  key(lis()[1], "Enter");
  expect(m.onSelectSession).toHaveBeenCalledTimes(1);
  expect(m.onSelectSession).toHaveBeenCalledWith("s2");
  m.unmount();
});

it("inbox footer shows the key hints", () => {
  const m = render(mixed());
  expect(m.container.querySelector('[data-part="footer"]')?.textContent).toBe("J/K move · ⏎ open · S snooze · E dismiss");
  m.unmount();
});

it("inbox disables snooze and dismiss", () => {
  const m = render(mixed());
  const snooze = m.container.querySelector<HTMLButtonElement>('[data-part="bar-snooze"]');
  const dismiss = m.container.querySelector<HTMLButtonElement>('[data-part="bar-dismiss"]');
  expect(snooze?.disabled).toBe(true);
  expect(dismiss?.disabled).toBe(true);
  click(snooze);
  click(dismiss);
  expect(m.onAction).not.toHaveBeenCalled();
  expect(m.onSelectSession).not.toHaveBeenCalled();
  m.unmount();
});
