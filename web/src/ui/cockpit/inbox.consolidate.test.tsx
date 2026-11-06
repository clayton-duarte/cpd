// @vitest-environment jsdom
import { act, useState } from "react";
import type { ComponentType, ReactElement } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { CockpitAction, InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import type { Disposable, Transport } from "../../protocol/runtime.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { mount } from "../../testing/render.tsx";
import { App } from "../App.tsx";
import { Inbox } from "./Inbox.tsx";

// pro-d53 props (`onSelectPlan`, `pendingActionIds`, `completedActionIds`) and `InboxRow.curation` are
// part of the contract (DESIGN-panes 2a, 2.6, 2.7) but not of the typed props yet: the loose view passes them.
const LooseInbox = Inbox as unknown as ComponentType<Record<string, unknown>>;

const base = { contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300 };
function row(id: number, kind: string, label: string, over: Record<string, unknown> = {}): InboxRow {
  return { ...base, id, kind, label, urgency: "white", sessionId: null, actions: [], ...over } as InboxRow;
}
function model(rows: InboxRow[]): InboxModel {
  return { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 1 };
}
const items = (c: Element) => Array.from(c.querySelectorAll<HTMLElement>('[data-kind][data-id]'));
function click(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });
}

const widths = { available: 0, required: 0 };
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
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
}
afterEach(() => {
  vi.unstubAllGlobals();
  delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  delete (HTMLElement.prototype as { scrollWidth?: number }).scrollWidth;
  document.body.innerHTML = "";
});

const mixed = () => [
  row(1, "dialog", "r1", { urgency: "red" }),
  row(2, "dialog", "r2", { urgency: "red" }),
  row(3, "deploy", "y1", { urgency: "yellow" }),
  row(4, "unread", "w1", { urgency: "white" }),
];
const tier = (c: Element, name: string) => c.querySelector<HTMLButtonElement>(`[data-slot="toggle-group-item"][data-tier-filter="${name}"]`);
const shown = (c: Element) => items(c).map((el) => el.getAttribute("data-id"));
const TIERS = ["blocker", "delivery", "decision", "support", "notice", "unread"];

it("inbox tier filter lists all, blocker, delivery, decision, support, notice and unread with counts", () => {
  const m = mount(<LooseInbox inbox={model(mixed())} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  const group = m.container.querySelector('[data-slot="toggle-group"]');
  expect(group?.getAttribute("data-part")).toBe("inbox-filters");
  expect(group?.getAttribute("aria-label")).toBe("Filter inbox by tier");
  expect(group?.className).toContain("ml-auto");
  const names = Array.from(group?.querySelectorAll('[data-slot="toggle-group-item"]') ?? []);
  expect(names.map((b) => b.getAttribute("data-tier-filter"))).toEqual(["all", ...TIERS]);
  // each item reads `<label> <count>`: the label span, then the count span (font-mono text-[10px] opacity-80)
  expect(names.map((b) => Array.from(b.querySelectorAll("span")).map((s) => s.textContent))).toEqual([
    ["all", "4"], ["blocker", "2"], ["delivery", "1"], ["decision", "0"], ["support", "0"], ["notice", "0"], ["unread", "1"],
  ]);
  for (const b of names) {
    expect(b.querySelectorAll("span")[1]?.className).toEqual(expect.stringContaining("font-mono"));
    expect((b as HTMLButtonElement).disabled).toBe(false);
  }
  // the preview's markup: the count is a plain span, with no data-part marker (merge rule: preview wins)
  expect(group?.querySelector('[data-part="filter-count"]')).toBeNull();
  const colours = ["text-fg-strong", "text-status-blocked-bright", "text-status-landing-bright", "text-status-decision-bright", "text-status-focus-bright", "text-status-active-bright", "text-fg-strong"];
  names.forEach((b, i) => expect(b.className).toContain(colours[i]!));
  m.unmount();
});

it("inbox filter all selects every tier and a tier click isolates it", () => {
  const m = mount(<LooseInbox inbox={model(mixed())} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  for (const n of ["all", ...TIERS]) expect(tier(m.container, n)?.getAttribute("data-state")).toBe("on");
  click(tier(m.container, "blocker"));
  expect(shown(m.container)).toEqual(["1", "2"]);
  expect(tier(m.container, "all")?.getAttribute("data-state")).toBe("off");
  click(tier(m.container, "unread"));
  expect(shown(m.container)).toEqual(["1", "2", "4"]);
  click(tier(m.container, "blocker"));
  click(tier(m.container, "unread"));
  expect(tier(m.container, "all")?.getAttribute("data-state")).toBe("on");
  expect(shown(m.container)).toEqual(["1", "2", "3", "4"]);
  // a tier with no rows is selectable and shows the one empty copy (DESIGN-panes 2.8)
  click(tier(m.container, "notice"));
  expect(items(m.container)).toHaveLength(0);
  expect(m.container.querySelector("[data-empty]")?.textContent).toBe("up to date, drink some water");
  m.unmount();
});

it("inbox filters collapse into a popover when the column is narrow", () => {
  stubLayout(200, 400);
  const m = mount(<LooseInbox inbox={model(mixed())} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  const trigger = m.container.querySelector<HTMLButtonElement>('[data-part="inbox-filters-trigger"]');
  expect(trigger?.getAttribute("aria-label")).toBe("Filter inbox by tier");
  expect(trigger?.textContent).toBe("all4");
  expect(m.container.querySelector('[data-part="inbox-filters"]')).toBeNull();
  click(trigger);
  const group = document.body.querySelector('[data-slot="popover-content"] [data-part="inbox-filters"]');
  expect(group?.getAttribute("data-orientation")).toBe("vertical");
  expect(group?.querySelectorAll('[data-slot="toggle-group-item"]')).toHaveLength(7);
  expect(Array.from(group?.querySelectorAll('[data-slot="toggle-group-item"]') ?? []).map((b) => b.textContent)).toEqual([
    "all4", "blocker2", "delivery1", "decision0", "support0", "notice0", "unread1",
  ]);
  expect(group?.querySelector('[data-part="filter-count"]')).toBeNull();
  click(group?.querySelector('[data-tier-filter="blocker"]'));
  expect(shown(m.container)).toEqual(["1", "2"]);
  expect(trigger?.textContent).toBe("1 tier2");
  m.unmount();
});

it("inbox row renders an Item with media, content and actions", () => {
  const r = row(1, "dialog", "Need an answer\nsecond line", { urgency: "red", sessionId: "s1", ageSeconds: 300 });
  const m = mount(<LooseInbox inbox={model([r])} onAction={vi.fn()} onSelectSession={vi.fn()} />);
  const el = items(m.container)[0]!;
  expect(el.tagName).not.toBe("A");
  expect(el.getAttribute("role")).toBe("button");
  expect(el.getAttribute("tabindex")).toBe("0");
  expect(el.getAttribute("data-variant")).toBe("outline");
  expect(el.getAttribute("data-size")).toBe("sm");
  expect(el.parentElement?.getAttribute("data-slot")).toBe("item-group");
  const media = el.querySelector('[data-slot="item-media"]');
  expect(media?.querySelector("svg.lucide-circle-alert")).not.toBeNull();
  const age = media?.querySelector('span[data-part="age"]');
  expect(age?.textContent).toBe("5m");
  expect(age?.parentElement?.className).toContain("mt-auto");
  expect(age?.parentElement?.className).toContain("leading-normal");
  expect(el.querySelector('[data-slot="item-content"] [data-slot="item-title"]')?.textContent).toBe("Need an answer");
  expect(el.querySelector('[data-slot="item-content"] [data-slot="item-description"]')?.textContent).toBe("second line");
  const actions = el.querySelector('[data-slot="item-actions"]');
  expect(actions?.querySelector("svg.lucide-chevron-right")).not.toBeNull();
  m.unmount();
});

// A stand-in for the store: `onAction` mints an id like `connection.action` and marks it pending.
function Harness(props: { onAction: (a: CockpitAction) => void; control: { finish?: () => void }; rows: InboxRow[] }): ReactElement {
  const [pending, setPending] = useState<string[]>([]);
  const [completed, setCompleted] = useState<string[]>([]);
  props.control.finish = () => {
    setPending([]);
    setCompleted(["a1"]);
  };
  return (
    <LooseInbox
      inbox={model(props.rows)}
      onSelectSession={vi.fn()}
      pendingActionIds={pending}
      completedActionIds={completed}
      onAction={(a: CockpitAction) => {
        props.onAction(a);
        setPending(["a1"]);
        return "a1";
      }}
    />
  );
}

const merge = { kind: "pr.merge", planId: "p1", pull: "o/r#1" } as const;
const mergeRow = () => row(5, "pr-ready", "o/r#1 ready to merge", { urgency: "yellow", sessionId: "lead1", actions: [{ label: "Merge", action: merge }] });
const actionBtn = (c: Element) => c.querySelector<HTMLButtonElement>('[data-part="item-action"]');

it("inbox action button disables and spins while its action is pending", () => {
  const onAction = vi.fn();
  const control: { finish?: () => void } = {};
  const m = mount(<Harness onAction={onAction} control={control} rows={[mergeRow()]} />);
  expect(actionBtn(m.container)?.textContent).toBe("Merge");
  expect(actionBtn(m.container)?.disabled).toBe(false);
  click(actionBtn(m.container));
  expect(onAction).toHaveBeenCalledExactlyOnceWith(merge);
  expect(actionBtn(m.container)?.disabled).toBe(true);
  expect(actionBtn(m.container)?.querySelector("svg.lucide-loader-circle")?.getAttribute("class")).toContain("animate-spin");
  click(actionBtn(m.container));
  expect(onAction).toHaveBeenCalledTimes(1);
  act(() => control.finish?.());
  expect(actionBtn(m.container)).toBeNull();
  m.unmount();
});

class FakeTransport implements Transport {
  sent: unknown[] = [];
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void {}
  send(raw: unknown): void { this.sent.push(raw); }
  close(): void {}
  onMessage(h: (raw: unknown) => void): Disposable {
    this.messageHandlers.push(h);
    return { dispose: () => { this.messageHandlers = this.messageHandlers.filter((x) => x !== h); } };
  }
  onStatus(h: (s: "open" | "closed") => void): Disposable {
    this.statusHandlers.push(h);
    return { dispose: () => { this.statusHandlers = this.statusHandlers.filter((x) => x !== h); } };
  }
  emitMessage(raw: unknown): void { for (const h of [...this.messageHandlers]) h(raw); }
  emitStatus(s: "open" | "closed"): void { for (const h of [...this.statusHandlers]) h(s); }
}
const reader = { readBase64: async () => "QUJD", readText: async () => "text" };
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
function snapshot(rows: InboxRow[]) {
  return {
    type: "cockpit.snapshot",
    tree: { cursor: 1, contexts: [{ id: "ctx", title: "C", archived: false, records: 0, openQuestions: 0, sessions: [], plans: [] }] },
    plan: { planId: "plan", contextId: "ctx", title: "P", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
    inbox: model(rows),
  };
}

it("inbox action error shows a dismissible destructive alert", () => {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot([mergeRow()])));
  const c = view.container;
  click(actionBtn(c));
  expect(transport.sent).toContainEqual({ type: "cockpit.action", actionId: "c1", action: merge });
  expect(actionBtn(c)?.disabled).toBe(true);
  expect(actionBtn(c)?.querySelector("svg.animate-spin")).not.toBeNull();
  act(() => transport.emitMessage({ type: "action.result", actionId: "c1", ok: false, error: { code: "cpd_failed", message: "merge failed" } }));
  expect(actionBtn(c)?.disabled).toBe(false);
  expect(actionBtn(c)?.querySelector("svg.animate-spin")).toBeNull();
  const toast = document.body.querySelector('[data-part="cockpit-error-toast"]');
  const alert = toast?.querySelector('[data-slot="alert"][role="alert"]');
  expect(alert?.textContent).toContain("cpd_failed");
  expect(alert?.textContent).toContain("merge failed");
  expect(alert?.className).toContain("text-destructive");
  click(toast?.querySelector('button[aria-label="Dismiss error"]'));
  expect(document.body.querySelector('[data-part="cockpit-error-toast"]')).toBeNull();
  view.unmount();
});

it("inbox row click selects the plan and the session", () => {
  const onSelectPlan = vi.fn();
  const onSelectSession = vi.fn();
  const onAction = vi.fn();
  const r = row(1, "dialog", "Need an answer", { urgency: "red", planId: "p9", sessionId: "s9" });
  const linked = row(2, "pr-ready", "o/r#1 ready to merge", { urgency: "yellow", planId: "p1", sessionId: "lead1", url: "https://example.test/pr/1" });
  const m = mount(<LooseInbox inbox={model([r, linked])} onAction={onAction} onSelectSession={onSelectSession} onSelectPlan={onSelectPlan} />);
  click(items(m.container)[0]);
  expect(onSelectPlan).toHaveBeenCalledExactlyOnceWith("p9");
  expect(onSelectSession).toHaveBeenCalledExactlyOnceWith("s9");
  expect(onAction).not.toHaveBeenCalled();
  // the linked title and the item-link icon open row.url in a new tab and do not select
  onSelectPlan.mockClear();
  onSelectSession.mockClear();
  const second = items(m.container)[1]!;
  const title = second.querySelector<HTMLAnchorElement>('[data-slot="item-title"] a');
  const icon = second.querySelector<HTMLAnchorElement>('a[data-part="item-link"]');
  for (const a of [title, icon]) {
    expect(a?.getAttribute("href")).toBe("https://example.test/pr/1");
    expect(a?.getAttribute("target")).toBe("_blank");
    expect(a?.getAttribute("rel")).toBe("noopener noreferrer");
    click(a);
  }
  expect(icon?.getAttribute("aria-label")).toBe("Open external link");
  expect(onSelectPlan).not.toHaveBeenCalled();
  expect(onSelectSession).not.toHaveBeenCalled();
  m.unmount();
});

it("inbox curate-retire row opens the curate dialog", () => {
  const onAction = vi.fn();
  const onSelectPlan = vi.fn();
  const onSelectSession = vi.fn();
  const curation = [
    { id: "r1", text: "first record", detail: "stale a.md" },
    { id: "r2", text: "second record", detail: "dup b.md" },
  ];
  const r = row(7, "curate-retire", "Curate c1", { urgency: "blue", contextId: "c1", planId: null, sessionId: "m1", curation });
  const m = mount(<LooseInbox inbox={model([r])} onAction={onAction} onSelectSession={onSelectSession} onSelectPlan={onSelectPlan} />);
  expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  click(items(m.container)[0]);
  expect(onSelectSession).toHaveBeenCalledExactlyOnceWith("m1");
  const dialog = document.body.querySelector('[role="dialog"]');
  expect(dialog?.textContent).toContain("Retire records of c1");
  expect(dialog?.textContent).toContain("Checked records will be retired; unchecked records will be kept.");
  expect(dialog?.textContent).toContain("first record");
  const boxes = Array.from(dialog?.querySelectorAll<HTMLInputElement>('input[type="checkbox"]') ?? []);
  expect(boxes.map((b) => b.checked)).toEqual([true, true]);
  click(boxes[1]);
  click(Array.from(dialog?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Confirm"));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ kind: "context.retire", contextId: "c1", approve: "r1", reject: "r2" });
  expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  m.unmount();
});

it("inbox unread row click selects the session and dispatches inbox.dismiss", () => {
  const onAction = vi.fn();
  const onSelectSession = vi.fn();
  const m = mount(<LooseInbox inbox={model([row(11, "unread", "new reply", { sessionId: "s1" }), row(-1, "unread", "synthetic", { id: -1, sessionId: "s2" })])} onAction={onAction} onSelectSession={onSelectSession} />);
  click(items(m.container)[0]);
  expect(onSelectSession).toHaveBeenCalledExactlyOnceWith("s1");
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ kind: "inbox.dismiss", itemId: 11 });
  onAction.mockClear();
  click(items(m.container)[1]);
  expect(onSelectSession).toHaveBeenLastCalledWith("s2");
  expect(onAction).not.toHaveBeenCalled();
  m.unmount();
});

function errorToastApp(error: { code: string; message: string; details?: Record<string, unknown> }) {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot([mergeRow()])));
  click(actionBtn(view.container));
  act(() => transport.emitMessage({ type: "action.result", actionId: "c1", ok: false, error }));
  return { transport, view };
}
const reportBtn = () => document.body.querySelector('[data-part="cockpit-error-toast"] [data-part="error-toast-report"]');

it("error toast Send to host dispatches error.report and dismisses", () => {
  const { transport, view } = errorToastApp({ code: "cpd_failed", message: "too new", details: { cpdCode: "store_too_new" } });
  expect(reportBtn()?.textContent).toBe("Send to host");
  click(reportBtn());
  expect(transport.sent).toContainEqual({ type: "cockpit.action", actionId: expect.any(String), action: { kind: "error.report", target: "host", code: "cpd_failed", message: "too new" } });
  expect(document.body.querySelector('[data-part="cockpit-error-toast"]')).toBeNull();
  view.unmount();
});

it("error toast Send to cpd for a protocol_error", () => {
  const { view } = errorToastApp({ code: "protocol_error", message: "bad frame" });
  expect(reportBtn()?.textContent).toBe("Send to cpd");
  view.unmount();
});
