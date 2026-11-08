// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";
import { App } from "./App.tsx";

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

// The panes are not what this test proves: they render one marker each, so the shell mounts cheaply.
vi.mock("./cockpit/ContextTree.tsx", () => ({ ContextTree: () => <div data-pane="ContextTree" /> }));
vi.mock("./cockpit/PlanView.tsx", () => ({ PlanView: () => <div data-pane="PlanView" /> }));
vi.mock("./cockpit/Inbox.tsx", () => ({ Inbox: () => <div data-pane="Inbox" /> }));
vi.mock("./ChatPane.tsx", () => ({ ChatPane: () => <div data-pane="ChatPane" /> }));

class FakeTransport implements Transport {
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void {}
  send(): void {}
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

const reader = { readBase64: async () => "", readText: async () => "" };
const snapshot = {
  type: "cockpit.snapshot",
  tree: { cursor: 1, contexts: [] },
  plan: null,
  inbox: { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 },
  status: null,
  costs: null,
};

let mounted: { unmount(): void } | null = null;
beforeEach(() => { vi.spyOn(console, "error").mockImplementation(() => {}); });
afterEach(() => { mounted?.unmount(); mounted = null; window.sessionStorage.clear(); vi.restoreAllMocks(); });

it("reports healthy only after the first cockpit snapshot", () => {
  const onHealthy = vi.fn();
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} onHealthy={onHealthy} />);
  mounted = view;

  // An open transport with a mounted shell and a populated root proves nothing: no data has arrived.
  act(() => transport.emitStatus("open"));
  expect(onHealthy).not.toHaveBeenCalled();

  // The first snapshot is what turns the desktop healthy (cockpit.tree goes from null to a tree).
  act(() => transport.emitMessage(snapshot));
  expect(onHealthy).toHaveBeenCalledTimes(1);

  // It reports once: later traffic does not turn the window healthy again.
  act(() => transport.emitMessage(snapshot));
  expect(onHealthy).toHaveBeenCalledTimes(1);
});
