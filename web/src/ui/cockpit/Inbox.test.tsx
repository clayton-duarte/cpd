// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { mount } from "../../testing/render.tsx";
import { Inbox } from "./Inbox.tsx";

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300 };

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
const items = (c: Element) => Array.from(c.querySelectorAll("a[data-kind]"));
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

const widths = { available: 0, required: 0 };
const observers: Array<() => void> = [];
function stubLayout(available: number, required: number): void {
  widths.available = available;
  widths.required = required;
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.getAttribute("data-part") === "inbox-header" ? widths.available : 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "scrollWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.getAttribute("aria-hidden") === "true" && this.className.includes("invisible") ? widths.required : 0;
    },
  });
  observers.length = 0;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(cb: () => void) {
        observers.push(cb);
      }
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    },
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  delete (HTMLElement.prototype as { scrollWidth?: number }).scrollWidth;
});

it("inbox rows are flat in model order with no tier groups or counts", () => {
  const m = render([
    row(1, "unread", "white row", { urgency: "white" }),
    row(2, "dialog", "red row", { urgency: "red" }),
    row(3, "deploy", "yellow row", { urgency: "yellow" }),
  ]);
  expect(items(m.container).map((el) => el.getAttribute("data-id"))).toEqual(["1", "2", "3"]);
  expect(m.container.querySelector('section[data-part="group"]')).toBeNull();
  expect(m.container.querySelector('[data-part="group-label"]')).toBeNull();
  expect(m.container.querySelector('[data-part="count"]')).toBeNull();
  expect(m.container.querySelector('[data-part="total"]')).toBeNull();
  expect(m.container.querySelector('[data-part="kind"]')).toBeNull();
  m.unmount();
});

it("inbox rows render as outline small shadcn items inside an item group", () => {
  const m = render([row(1, "dialog", "a", { sessionId: "s1" })]);
  const el = items(m.container)[0];
  expect(el?.getAttribute("data-variant")).toBe("outline");
  expect(el?.getAttribute("data-size")).toBe("sm");
  expect(el?.parentElement?.getAttribute("data-slot")).toBe("item-group");
  m.unmount();
});

it("an inbox row shows an urgency icon the label and the age", () => {
  const m = render([row(1, "dialog", "Need an answer", { urgency: "red", sessionId: "s1", ageSeconds: 300 })]);
  const first = items(m.container)[0];
  expect(first?.querySelector('[data-slot="item-media"] svg')).not.toBeNull();
  expect(first?.querySelector('[data-slot="item-title"]')?.textContent).toBe("Need an answer");
  expect(first?.querySelector('[data-slot="item-media"]')?.textContent).toContain("5m");
  expect(first?.textContent).not.toContain("dialog");
  m.unmount();
});

it("a label with a newline shows its second line as the item description", () => {
  const m = render([row(1, "dialog", "First line\nsecond  line\nthird", { sessionId: "s1" })]);
  const first = items(m.container)[0];
  expect(first?.querySelector('[data-slot="item-title"]')?.textContent).toBe("First line");
  expect(first?.querySelector('[data-slot="item-description"]')?.textContent).toBe("second  line third");
  m.unmount();
});

it("a linked row is an anchor that opens its url in a new tab", () => {
  const m = render([row(1, "deploy", "Deploy ready", { urgency: "yellow", url: "https://example.test/run/1" })]);
  const el = items(m.container)[0];
  expect(el?.tagName).toBe("A");
  expect(el?.getAttribute("href")).toBe("https://example.test/run/1");
  expect(el?.getAttribute("target")).toBe("_blank");
  expect(el?.getAttribute("rel")).toBe("noopener noreferrer");
  expect(el?.querySelector('[data-slot="item-actions"] svg.lucide-external-link')).not.toBeNull();
  m.unmount();
});

it("an unlinked row is a placeholder anchor whose click is prevented", () => {
  const m = render([row(1, "dialog", "Need an answer", { sessionId: "s1" })]);
  const el = items(m.container)[0];
  expect(el?.tagName).toBe("A");
  expect(el?.getAttribute("href")).toBe("#");
  expect(el?.getAttribute("target")).toBeNull();
  const ev = new MouseEvent("click", { bubbles: true, cancelable: true });
  act(() => {
    el?.dispatchEvent(ev);
  });
  expect(ev.defaultPrevented).toBe(true);
  expect(el?.querySelector('[data-slot="item-actions"] svg.lucide-chevron-right')).not.toBeNull();
  m.unmount();
});

it("each kind and urgency picks its lucide icon", () => {
  const cases: Array<[string, InboxRow["urgency"], string]> = [
    ["deploy", "yellow", "lucide-rocket"],
    ["pr-ready", "yellow", "lucide-git-pull-request"],
    ["pr-closed", "white", "lucide-git-pull-request"],
    ["review-requested", "red", "lucide-git-pull-request-arrow"],
    ["curate-retire", "blue", "lucide-sparkles"],
    ["k", "red", "lucide-circle-alert"],
    ["k", "yellow", "lucide-circle-dot"],
    ["k", "magenta", "lucide-circle-question-mark"],
    ["k", "blue", "lucide-eye"],
    ["k", "cyan", "lucide-bell"],
    ["k", "white", "lucide-message-square"],
  ];
  const m = render(cases.map(([kind, urgency], i) => row(i + 1, kind, `r${i}`, { urgency })));
  const got = items(m.container).map((el) => el.querySelector('[data-slot="item-media"] svg')?.getAttribute("class") ?? "");
  cases.forEach(([, , cls], i) => expect(got[i]?.split(" ")).toContain(cls));
  m.unmount();
});

it("the item title is coloured by urgency", () => {
  const m = render([row(1, "dialog", "a", { urgency: "red" }), row(2, "dialog", "b", { urgency: "white" })]);
  const titles = items(m.container).map((el) => el.querySelector('[data-slot="item-title"]')?.className ?? "");
  expect(titles[0]).toContain("text-status-blocked");
  expect(titles[1]).toContain("text-muted-foreground");
  m.unmount();
});

it("an inbox row has no inline action buttons and no key footer", () => {
  const dismiss = { kind: "inbox.dismiss", itemId: 1 } as const;
  const m = render([row(1, "deploy", "Deploy ready", { urgency: "yellow", actions: [{ label: "Dismiss", action: dismiss }] })]);
  expect(m.container.querySelector("a button")).toBeNull();
  expect(m.container.querySelector('[data-part="actions"]')).toBeNull();
  expect(m.container.querySelector('[data-part="bar"]')).toBeNull();
  expect(m.container.querySelector('[data-part="footer"]')).toBeNull();
  expect(m.container.textContent).not.toContain("J/K");
  expect(m.container.textContent).not.toContain("Engine is not running");
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
  const pr = row(1, "pr-ready", "acme/api#3 ready to merge", { urgency: "yellow", sessionId: "lead1", workflow: "w1", url: "https://example.test/pr/3", actions: [{ label: "Merge", action: { kind: "pr.merge", planId: "p1", pull: "acme/api#3" } }] });
  const m = render([pr, row(2, "unread", "plain", { sessionId: null })]);
  const [first, second] = items(m.container);
  rightClick(first);
  expect(menuLabels()).toEqual(expect.arrayContaining(["Merge", "Go to Lead", "Copy Link", "Dismiss"]));
  click(menuItems().find((i) => i.textContent === "Merge"));
  expect(m.onAction).toHaveBeenLastCalledWith({ kind: "pr.merge", planId: "p1", pull: "acme/api#3" });
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
const shown = (c: Element) => items(c).map((el) => el.getAttribute("data-id"));
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

const trigger = (c: Element) => c.querySelector<HTMLElement>('[data-part="inbox-filters-trigger"]');
const sampleRows = () => [row(1, "blocked", "a", { urgency: "red" }), row(2, "x", "b", { urgency: "yellow" }), row(3, "x", "c", { urgency: "yellow" })];

it("the header keeps a 2rem gap between the title and the filters", () => {
  const m = render([]);
  expect(m.container.querySelector('[data-part="inbox-header"]')?.className).toContain("gap-8");
  m.unmount();
});

it("wide enough: the filters render inline as a horizontal toggle group and no popover trigger", () => {
  stubLayout(500, 300);
  const m = render(sampleRows());
  expect(trigger(m.container)).toBeNull();
  const group = m.container.querySelector('[data-part="inbox-filters"]');
  expect(group?.getAttribute("data-orientation")).toBe("horizontal");
  m.unmount();
});

it("narrow: the filters collapse into a popover trigger showing the label and the shown count", () => {
  stubLayout(200, 300);
  const m = render(sampleRows());
  const t = trigger(m.container);
  expect(t?.textContent).toBe("all3");
  expect(m.container.querySelector('[data-part="inbox-filters"]')).toBeNull();
  m.unmount();
});

it("narrow: opening the popover shows a vertical toggle group with all seven filters stacked", () => {
  stubLayout(200, 300);
  const m = render(sampleRows());
  click(trigger(m.container));
  const group = document.body.querySelector('[data-part="inbox-filters"]');
  expect(group?.getAttribute("data-orientation")).toBe("vertical");
  const names = Array.from(group?.querySelectorAll("[data-tier-filter]") ?? []).map((b) => b.getAttribute("data-tier-filter"));
  expect(names).toEqual(["all", "blocker", "delivery", "decision", "support", "notice", "unread"]);
  m.unmount();
});

it("narrow: choosing a tier in the popover filters the rows like the inline group", () => {
  stubLayout(200, 300);
  const m = render(sampleRows());
  click(trigger(m.container));
  click(document.body.querySelector('[data-tier-filter="delivery"]'));
  expect(items(m.container)).toHaveLength(2);
  expect(trigger(m.container)?.textContent).toBe("1 tier2");
  m.unmount();
});

it("the collapse follows the measured widths when the observer fires", () => {
  stubLayout(500, 300);
  const m = render(sampleRows());
  expect(trigger(m.container)).toBeNull();
  widths.available = 200;
  act(() => {
    for (const cb of observers) cb();
  });
  expect(trigger(m.container)).not.toBeNull();
  widths.available = 500;
  act(() => {
    for (const cb of observers) cb();
  });
  expect(trigger(m.container)).toBeNull();
  m.unmount();
});

it("the measuring clone is hidden from assistive tech and holds no filter hooks", () => {
  stubLayout(500, 300);
  const m = render(sampleRows());
  const clone = m.container.querySelector('[aria-hidden="true"].invisible');
  expect(clone).not.toBeNull();
  expect(clone?.querySelector("button, [data-tier-filter], [data-part]")).toBeNull();
  expect(clone?.textContent).toContain("Inbox");
  expect(clone?.textContent).toContain("unread0");
  expect(m.container.querySelectorAll('[data-tier-filter="blocker"]')).toHaveLength(1);
  m.unmount();
});

const mergeAction = { label: "Merge", action: { kind: "pr.merge", planId: "p1", pull: "acme/api#3" } } as const;

it("the item media column stretches with the icon on top and the age at the bottom", () => {
  const m = render([row(1, "dialog", "Need an answer\nmore", { sessionId: "s1" })]);
  const media = items(m.container)[0]?.querySelector('[data-slot="item-media"]');
  expect(media?.className).toContain("flex-col");
  expect(media?.className).toContain("justify-between");
  expect(media?.className).toContain("self-stretch");
  expect(media?.firstElementChild?.className).toContain("h-[19.25px]");
  expect(media?.firstElementChild?.querySelector("svg")).not.toBeNull();
  expect(media?.lastElementChild?.querySelector('[data-part="age"]')?.textContent).toBe("5m");
  m.unmount();
});

it("the age sits in a mt-auto block text-sm leading-normal wrapper", () => {
  const m = render([row(1, "dialog", "Need an answer", { sessionId: "s1" })]);
  const wrap = items(m.container)[0]?.querySelector('[data-part="age"]')?.parentElement;
  expect(wrap?.className).toContain("mt-auto");
  expect(wrap?.className).toContain("block");
  expect(wrap?.className).toContain("text-sm");
  expect(wrap?.className).toContain("leading-normal");
  m.unmount();
});

it("the item actions column stretches with the link icon on top and the primary action button at the bottom", () => {
  const m = render([row(1, "pr-ready", "acme/api#3 ready to merge", { url: "https://example.test/pr/3", actions: [mergeAction] })]);
  const actions = items(m.container)[0]?.querySelector('[data-slot="item-actions"]');
  expect(actions?.className).toContain("flex-col");
  expect(actions?.className).toContain("self-stretch");
  expect(actions?.firstElementChild?.matches("svg.lucide-external-link")).toBe(true);
  expect(actions?.lastElementChild?.matches('button[data-part="item-action"]')).toBe(true);
  m.unmount();
});

it("a row with a primary action shows an outline small button that runs it without following the link", () => {
  const m = render([row(1, "pr-ready", "acme/api#3 ready to merge", { url: "https://example.test/pr/3", sessionId: "s1", actions: [mergeAction] })]);
  const button = items(m.container)[0]?.querySelector<HTMLButtonElement>('button[data-part="item-action"]');
  expect(button?.textContent).toBe("Merge");
  expect(button?.getAttribute("data-variant")).toBe("outline");
  expect(button?.getAttribute("data-size")).toBe("sm");
  const ev = new MouseEvent("click", { bubbles: true, cancelable: true });
  act(() => {
    button?.dispatchEvent(ev);
  });
  expect(ev.defaultPrevented).toBe(true);
  expect(m.onAction).toHaveBeenCalledExactlyOnceWith(mergeAction.action);
  expect(m.onSelectSession).not.toHaveBeenCalled();
  m.unmount();
});

it("a row without a primary action shows no button", () => {
  const snooze = { label: "Snooze 15m", action: { kind: "inbox.snooze", itemId: 1, duration: "15m" } } as const;
  const m = render([row(1, "dialog", "Need an answer", { sessionId: "s1", actions: [snooze] })]);
  expect(items(m.container)[0]?.querySelector("button")).toBeNull();
  m.unmount();
});

it("the age is not inside the item actions", () => {
  const m = render([row(1, "dialog", "Need an answer", { sessionId: "s1" })]);
  expect(items(m.container)[0]?.querySelector('[data-slot="item-actions"] [data-part="age"]')).toBeNull();
  m.unmount();
});
