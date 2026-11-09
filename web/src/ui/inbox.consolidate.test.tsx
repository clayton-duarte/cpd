// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";
import { App } from "./App.tsx";

vi.mock("@base-ui/react/scroll-area", () => import("../testing/scroll-area-mock.tsx"));

class FakeTransport implements Transport {
  sent: unknown[] = [];
  /** When true, send throws like ws-transport does on a closed socket. */
  closedSocket = false;
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void {}
  send(raw: unknown): void {
    if (this.closedSocket) throw new Error("transport is not open");
    this.sent.push(raw);
  }
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
const merge = { kind: "pr.merge", planId: "p1", pull: "o/r#1" } as const;
const snapshot = {
  type: "cockpit.snapshot",
  tree: { cursor: 1, contexts: [{ id: "ctx", title: "C", archived: false, records: 0, openQuestions: 0, sessions: [], plans: [] }] },
  plan: { planId: "plan", contextId: "ctx", title: "P", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
  inbox: {
    openCount: 1, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, cursor: 1,
    rows: [{ id: 5, kind: "pr-ready", label: "o/r#1 ready to merge", urgency: "yellow", sessionId: "lead1", contextId: "c1", planId: "p1", jobKey: null, url: null, workflow: null, openedAt: "2026-01-01T00:00:00Z", ageSeconds: 300, actions: [{ label: "Merge", action: merge }] }],
  },
};

let mounted: { unmount(): void } | null = null;
afterEach(() => {
  mounted?.unmount();
  mounted = null;
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

function click(el: Element | null | undefined): void {
  if (el === null || el === undefined) throw new Error("element not found");
  act(() => { el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })); });
}
const toast = () => document.body.querySelector('[data-part="cockpit-error-toast"]');
const reportBtn = () => document.body.querySelector<HTMLButtonElement>('[data-part="cockpit-error-toast"] [data-part="error-toast-report"]');
const dismissBtn = () => document.body.querySelector<HTMLButtonElement>('[data-part="cockpit-error-toast"] button[aria-label="Dismiss error"]');

function errorToastApp() {
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  const transport = new FakeTransport();
  let nextId = 0;
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => `c${++nextId}`} />);
  mounted = view;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot));
  click(document.body.querySelector('[data-part="item-action"]'));
  act(() => transport.emitMessage({ type: "action.result", actionId: "c1", ok: false, error: { code: "cpd_failed", message: "merge failed", details: { cpdCode: "store_too_new" } } }));
  return transport;
}

const reports = (transport: FakeTransport) =>
  transport.sent.filter((m) => (m as { action?: { kind?: string } }).action?.kind === "error.report") as { actionId: string }[];

it("error toast closes after Send to target succeeds", () => {
  const transport = errorToastApp();
  expect(reportBtn()?.textContent).toBe("Send to host");
  // a first attempt while the socket is closed must not poison the next one (reportInFlight reset)
  transport.closedSocket = true;
  click(reportBtn());
  transport.closedSocket = false;
  click(reportBtn() ?? document.body.querySelector('[data-part="error-toast-retry"]'));
  const sent = reports(transport);
  expect(sent).toHaveLength(1);
  act(() => transport.emitMessage({ type: "action.result", actionId: sent[0]!.actionId, ok: true, output: {} }));
  expect(toast()).toBeNull();
});

it("error toast stays dismissable and not stuck Sending when send throws while closed", () => {
  const transport = errorToastApp();
  transport.closedSocket = true;
  click(reportBtn());
  expect(toast()).not.toBeNull();
  expect(reportBtn()?.textContent).not.toBe("Sending…");
  // the failure is shown, and the Send can be retried
  expect(toast()?.querySelector('[data-part="error-toast-failure"]')).not.toBeNull();
  expect(dismissBtn()?.disabled).toBe(false);
  click(dismissBtn());
  expect(toast()).toBeNull();
});
