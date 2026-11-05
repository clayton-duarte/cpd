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
  expect(m.container.querySelector("li button")).toBeNull();
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

const mixed = () => [
  row(1, "dialog", "r1", { urgency: "red" }),
  row(2, "dialog", "r2", { urgency: "red" }),
  row(3, "deploy", "y1", { urgency: "yellow" }),
  row(4, "unread", "w1", { urgency: "white" }),
];
const btn = (c: Element, name: string) => c.querySelector<HTMLButtonElement>(`[data-slot="toggle-group-item"][data-tier-filter="${name}"]`);
const countOf = (b: Element | null) => b?.querySelector('[data-part="filter-count"]')?.textContent;
const shown = (c: Element) => items(c).map((li) => li.getAttribute("data-id"));
const on = (c: Element, name: string) => btn(c, name)?.getAttribute("data-state") === "on";
const pressed = (c: Element, name: string) => btn(c, name)?.getAttribute("aria-pressed");
const TIER_NAMES = ["blocker", "delivery", "decision", "support", "notice", "unread"];

it("the filter bar has an all button and the six tier buttons with a count each", () => {
  const m = render(mixed());
  const group = m.container.querySelector('[data-slot="toggle-group"]');
  expect(group?.getAttribute("data-part")).toBe("inbox-filters");
  expect(group?.getAttribute("aria-label")).toBe("Filter inbox by tier");
  const names = Array.from(group?.querySelectorAll('[data-slot="toggle-group-item"]') ?? []).map((b) => b.getAttribute("data-tier-filter"));
  expect(names).toEqual(["all", ...TIER_NAMES]);
  for (const n of names) expect(countOf(btn(m.container, n ?? ""))).toMatch(/^\d+$/);
  expect(btn(m.container, "blocker")?.textContent).toBe("blocker2");
  expect(btn(m.container, "blocker")?.querySelector('[data-part="filter-count"]')?.className).toContain("font-mono");
  for (const n of ["all", ...TIER_NAMES]) {
    expect(on(m.container, n)).toBe(true);
    expect(pressed(m.container, n)).toBe("true");
  }
  m.unmount();
});

it("a tier with no rows is not disabled and shows 0", () => {
  const m = render(mixed());
  const b = btn(m.container, "notice");
  expect(b?.disabled).toBe(false);
  expect(countOf(b)).toBe("0");
  for (const el of m.container.querySelectorAll('[data-slot="toggle-group-item"]')) expect((el as HTMLButtonElement).disabled).toBe(false);
  m.unmount();
});

it("the all button shows the total and every tier shows its own count", () => {
  const m = render(mixed());
  expect(countOf(btn(m.container, "all"))).toBe("4");
  expect(TIER_NAMES.map((n) => countOf(btn(m.container, n)))).toEqual(["2", "1", "0", "0", "0", "1"]);
  m.unmount();
});

it("clicking a tier isolates it and the rows follow", () => {
  const m = render(mixed());
  click(btn(m.container, "blocker"));
  expect(shown(m.container)).toEqual(["1", "2"]);
  expect(on(m.container, "blocker")).toBe(true);
  expect(pressed(m.container, "blocker")).toBe("true");
  expect(on(m.container, "delivery")).toBe(false);
  expect(pressed(m.container, "delivery")).toBe("false");
  expect(on(m.container, "all")).toBe(false);
  m.unmount();
});

it("after isolating, further tier clicks add and remove tiers", () => {
  const m = render(mixed());
  click(btn(m.container, "blocker"));
  click(btn(m.container, "unread"));
  expect(shown(m.container)).toEqual(["1", "2", "4"]);
  click(btn(m.container, "blocker"));
  expect(shown(m.container)).toEqual(["4"]);
  expect(on(m.container, "blocker")).toBe(false);
  expect(on(m.container, "unread")).toBe(true);
  m.unmount();
});

it("clicking the active tier again returns to all", () => {
  const m = render(mixed());
  click(btn(m.container, "blocker"));
  click(btn(m.container, "blocker"));
  expect(shown(m.container)).toEqual(["1", "2", "3", "4"]);
  expect(on(m.container, "all")).toBe(true);
  expect(on(m.container, "delivery")).toBe(true);
  m.unmount();
});

it("selecting every tier returns to all", () => {
  const m = render(mixed());
  click(btn(m.container, "blocker"));
  for (const n of TIER_NAMES.slice(1)) click(btn(m.container, n));
  expect(on(m.container, "all")).toBe(true);
  expect(shown(m.container)).toEqual(["1", "2", "3", "4"]);
  click(btn(m.container, "blocker"));
  expect(shown(m.container)).toEqual(["1", "2"]);
  click(btn(m.container, "all"));
  expect(shown(m.container)).toEqual(["1", "2", "3", "4"]);
  m.unmount();
});

it("a selected tier with no rows shows the filter-empty message", () => {
  const m = render(mixed());
  click(btn(m.container, "notice"));
  expect(on(m.container, "notice")).toBe(true);
  expect(items(m.container)).toHaveLength(0);
  expect(m.container.querySelector("[data-empty]")?.textContent).toBe("no items in this filter");
  m.unmount();
});

it("the title and the filters share one header row with the group right-aligned", () => {
  const m = render(mixed());
  const header = m.container.querySelector('[data-part="inbox-header"]');
  const group = m.container.querySelector('[data-part="inbox-filters"]');
  for (const c of ["flex", "h-8", "items-center", "justify-between"]) expect(header?.className).toContain(c);
  expect(header?.className).not.toContain("flex-wrap");
  expect(group?.parentElement).toBe(header);
  expect(group?.className).toContain("ml-auto");
  expect(group?.className).not.toContain("w-full");
  m.unmount();
});

it("the filter items use the outline variant with a tier text colour and no fill", () => {
  const m = render(mixed());
  for (const n of ["all", ...TIER_NAMES]) {
    const b = btn(m.container, n);
    expect(b?.getAttribute("data-variant")).toBe("outline");
    expect(b?.className).toMatch(/(^|\s)text-(status-[a-z]+-bright|fg-strong)(\s|$)/);
    expect(b?.className).not.toMatch(/(^|\s)bg-status-/);
  }
  expect(btn(m.container, "blocker")?.className).toContain("text-status-blocked-bright");
  m.unmount();
});

it("the Inbox title shows no count", () => {
  const m = render(mixed());
  const header = m.container.querySelector('[data-part="inbox-header"]');
  expect(header?.querySelector("h2")?.textContent).toBe("Inbox");
  expect(Array.from(header?.children ?? []).map((e) => e.tagName)).toEqual(["H2", "DIV"]);
  m.unmount();
});
