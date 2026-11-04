// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300 };
const RED = "\u{F0362}";
const YELLOW = "\u{F069A}";
const LINK_CUE = "\u{F11AD}";

function row(id: number, kind: string, label: string, over: Partial<InboxRow> = {}): InboxRow {
  return { ...base, id, kind, label, urgency: "white", sessionId: null, actions: [], ...over };
}
function model(rows: InboxRow[], engineRunning = true): InboxModel {
  return { openCount: rows.length, engineRunning, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 1 };
}
function render(rows: InboxRow[]) {
  const onAction = vi.fn();
  const onSelectSession = vi.fn();
  const m = mount(<Inbox inbox={model(rows)} onAction={onAction} onSelectSession={onSelectSession} />);
  return { ...m, onAction, onSelectSession };
}
const items = (c: Element) => Array.from(c.querySelectorAll("li[data-kind]"));
function click(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}
function rightClick(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 5, clientY: 5 }));
  });
}
const menuItems = () => Array.from(document.body.querySelectorAll('[role="menuitem"]'));
const menuLabels = () => menuItems().map((i) => i.textContent);

afterEach(() => {
  vi.unstubAllGlobals();
});

it("inbox rows are flat in model order with no tier groups or counts", () => {
  const m = render([
    row(1, "unread", "white row", { urgency: "white" }),
    row(2, "dialog", "red row", { urgency: "red" }),
    row(3, "deploy", "yellow row", { urgency: "yellow" }),
  ]);
  expect(items(m.container).map((li) => li.getAttribute("data-id"))).toEqual(["1", "2", "3"]);
  expect(m.container.querySelector('section[data-part="group"]')).toBeNull();
  expect(m.container.querySelector('[data-part="group-label"]')).toBeNull();
  expect(m.container.querySelector('[data-part="count"]')).toBeNull();
  expect(m.container.querySelector('[data-part="total"]')).toBeNull();
  expect(m.container.querySelector('[data-part="kind"]')).toBeNull();
  m.unmount();
});

it("an inbox row shows the urgency glyph the label and the age on one line", () => {
  const m = render([row(1, "dialog", "Need an answer", { urgency: "red", sessionId: "s1", ageSeconds: 300 })]);
  const first = items(m.container)[0];
  const text = first?.textContent ?? "";
  expect(text).toContain(RED);
  expect(text).toContain("Need an answer");
  expect(text).toContain("5m");
  expect(text.indexOf(RED)).toBeLessThan(text.indexOf("Need an answer"));
  expect(text.indexOf("Need an answer")).toBeLessThan(text.indexOf("5m"));
  expect(first?.textContent).not.toContain("dialog");
  m.unmount();
});

it("a label with a newline shows its second line muted on a second row", () => {
  const m = render([row(1, "dialog", "First line\nsecond line", { sessionId: "s1" })]);
  const first = items(m.container)[0];
  const second = Array.from(first?.querySelectorAll("*") ?? []).find((e) => e.children.length === 0 && e.textContent === "second line");
  expect(second, "second line element").toBeDefined();
  expect(second?.closest("[class*='muted']"), "muted").not.toBeNull();
  expect(first?.textContent?.indexOf("First line")).toBeLessThan(first?.textContent?.indexOf("second line") ?? -1);
  const firstLine = Array.from(first?.querySelectorAll("*") ?? []).find((e) => e.children.length === 0 && e.textContent === "First line");
  expect(firstLine?.contains(second ?? null)).toBe(false);
  expect(firstLine?.parentElement?.contains(second ?? null), "second line is not on the first line's row").toBe(false);
  m.unmount();
});

it("an inbox row has no inline action buttons and no key footer", () => {
  const approve = { kind: "inbox.approve", itemId: 1 } as const;
  const m = render([row(1, "deploy", "Deploy ready", { urgency: "yellow", actions: [{ label: "Approve", action: approve }] })]);
  expect(m.container.querySelector("button")).toBeNull();
  expect(m.container.querySelector('[data-part="actions"]')).toBeNull();
  expect(m.container.querySelector('[data-part="bar"]')).toBeNull();
  expect(m.container.querySelector('[data-part="footer"]')).toBeNull();
  expect(m.container.textContent).not.toContain("J/K");
  expect(m.container.textContent).not.toContain("Engine is not running");
  m.unmount();
});

it("a link row shows a link cue after its label", () => {
  const m = render([row(1, "deploy", "Deploy ready", { urgency: "yellow", url: "https://example.test/run/1" })]);
  const text = items(m.container)[0]?.textContent ?? "";
  expect(text).toContain(`Deploy ready${LINK_CUE}`);
  m.unmount();
});

it("left click on a dialog row selects its session", () => {
  const m = render([row(1, "dialog", "Need an answer", { urgency: "red", sessionId: "s1" })]);
  click(items(m.container)[0]);
  expect(m.onSelectSession).toHaveBeenCalledExactlyOnceWith("s1");
  expect(m.onAction).not.toHaveBeenCalled();
  m.unmount();
});

it("left click on a PR row selects the Plan Lead session", () => {
  // for PR kinds the row's sessionId is the Plan's Lead session
  const m = render([row(1, "pr-ready", "acme/api#3 ready to merge", { urgency: "yellow", sessionId: "lead1", workflow: "w1", url: "https://example.test/pr/3" })]);
  click(items(m.container)[0]);
  expect(m.onSelectSession).toHaveBeenCalledExactlyOnceWith("lead1");
  expect(m.onAction).not.toHaveBeenCalled();
  m.unmount();
});

it("left click on a review-requested row copies its url", () => {
  const writeText = vi.fn(async () => undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const m = render([row(1, "review-requested", "Review PR 9", { urgency: "red", url: "https://example.test/pr/9" })]);
  click(items(m.container)[0]);
  expect(writeText).toHaveBeenCalledExactlyOnceWith("https://example.test/pr/9");
  expect(m.onSelectSession).not.toHaveBeenCalled();
  m.unmount();
});

it("left click on an engine-down row starts the engine", () => {
  const m = render([row(1, "engine-down", "engine is down", { urgency: "red" })]);
  click(items(m.container)[0]);
  expect(m.onAction).toHaveBeenCalledExactlyOnceWith({ kind: "engine.start" });
  expect(m.onSelectSession).not.toHaveBeenCalled();
  m.unmount();
});

it("right click opens a menu with snooze 15 min and 1 h and until tomorrow 09:00 and dismiss", () => {
  const m = render([row(7, "dialog", "Need an answer", { urgency: "red", sessionId: "s1" })]);
  rightClick(items(m.container)[0]);
  const labels = menuLabels();
  expect(labels).toEqual(expect.arrayContaining(["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"]));
  const pick = (t: string) => menuItems().find((i) => i.textContent === t);
  click(pick("Snooze 15 min"));
  expect(m.onAction).toHaveBeenLastCalledWith({ kind: "inbox.snooze", itemId: 7, duration: "15m" });
  rightClick(items(m.container)[0]);
  click(pick("Snooze 1 h"));
  expect(m.onAction).toHaveBeenLastCalledWith({ kind: "inbox.snooze", itemId: 7, duration: "1h" });
  rightClick(items(m.container)[0]);
  click(pick("Snooze until tomorrow 09:00"));
  expect(m.onAction).toHaveBeenLastCalledWith({ kind: "inbox.snooze", itemId: 7, duration: "tomorrow" });
  rightClick(items(m.container)[0]);
  click(pick("Dismiss"));
  expect(m.onAction).toHaveBeenLastCalledWith({ kind: "inbox.dismiss", itemId: 7 });
  m.unmount();
});

it("the menu offers Go to Lead and Copy Link when the row has them", () => {
  const writeText = vi.fn(async () => undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const pr = row(1, "pr-ready", "acme/api#3 ready to merge", { urgency: "yellow", sessionId: "lead1", workflow: "w1", url: "https://example.test/pr/3" });
  const m = render([pr, row(2, "unread", "plain", { sessionId: null })]);
  const [first, second] = items(m.container);
  rightClick(first);
  expect(menuLabels()).toEqual(expect.arrayContaining(["Merge", "Go to Lead", "Copy Link", "Dismiss"]));
  click(menuItems().find((i) => i.textContent === "Merge"));
  expect(m.onAction).toHaveBeenLastCalledWith({ kind: "pr.merge", planId: "p1", workflow: "w1" });
  rightClick(first);
  click(menuItems().find((i) => i.textContent === "Go to Lead"));
  expect(m.onSelectSession).toHaveBeenLastCalledWith("lead1");
  rightClick(first);
  click(menuItems().find((i) => i.textContent === "Copy Link"));
  expect(writeText).toHaveBeenCalledWith("https://example.test/pr/3");
  rightClick(second);
  expect(menuLabels()).not.toContain("Go to Lead");
  expect(menuLabels()).not.toContain("Copy Link");
  m.unmount();
});

it("an empty inbox shows up to date, drink some water", () => {
  const m = render([]);
  expect(m.container.textContent).toContain("up to date, drink some water");
  expect(m.container.textContent).not.toContain("Nothing needs you");
  m.unmount();
});
