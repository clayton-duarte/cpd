// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";
import { App } from "./App.tsx";

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });


class FakeTransport implements Transport {
  sent: unknown[] = [];
  connects = 0;
  closed = false;
  private messageHandlers: ((raw: unknown) => void)[] = [];
  private statusHandlers: ((s: "open" | "closed") => void)[] = [];
  connect(): void { if (!this.closed) this.connects++; }
  send(raw: unknown): void { this.sent.push(raw); }
  close(): void { this.closed = true; }
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

const reader = {
  readBase64: async () => "QUJD",
  readText: async () => "text",
};

const ref = { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" } as const;
const mgr = { ref: { id: "m1", key: "ctx", runner: "durable", role: "manager", state: "idle", title: "Mgr" }, contextId: "ctx", planId: null, jobKey: null, archived: false, dot: { state: "idle", attention: null } } as const;
const node = { ref, contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const snapshot = {
  type: "cockpit.snapshot",
  tree: {
    cursor: 1,
    contexts: [{ id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [mgr],
      plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node] }] }],
  },
  plan: {
    planId: "other", contextId: "ctx", title: "Other", phase: "dispatch", counts, review: null, openEscalations: 0,
    workflows: [], jobs: [], cursor: 1,
  },
  inbox: { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 },
};
const footer = {
  model: { state: "known", value: "p/m" }, tier: { state: "known", value: "S1" }, thinking: { state: "known", value: "high" },
  contextPercent: { state: "known", value: 5 }, cost: { state: "known", value: 0.1 }, branch: { state: "known", value: "main" },
};
const yes = { supported: true } as const;
const capabilities = {
  images: { supported: true, maxCount: 4, maxTotalBase64: 786432 },
  steer: yes, followUp: yes, abort: yes, abortSubmission: yes, compact: yes,
  configure: { model: yes, thinking: yes, tier: { supported: true, options: ["S1", "S2"] } },
  dialogs: { mode: "interactive" },
};

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function start() {
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  mounted = view;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot));
  return { transport, container: view.container };
}


const q = (c: HTMLElement, sel: string) => c.querySelector<HTMLElement>(sel);
const loadedPlan = { ...snapshot.plan, planId: "plan", title: "Plan One" };

it("selecting a Lead session shows that Lead Plan in PlanView", () => {
  const { transport, container } = start();
  // the loaded plan "other" is not the Lead's: it is hidden
  act(() => { q(container, '[data-plan="plan"] button[data-select="plan"]')!.click(); });
  expect(q(container, 'section[aria-label="Plan pane"] section[aria-label="Plan other"]')).toBeNull();
  expect(q(container, 'section[aria-label="Plan pane"] [data-part="flow-canvas"]')).not.toBeNull();
  act(() => transport.emitMessage({ ...snapshot, plan: loadedPlan }));
  expect(q(container, 'section[aria-label="Plan pane"] section[aria-label="Plan plan"]')).not.toBeNull();
});

it("selecting a Manager session shows the ManagerView in the Plan pane", () => {
  const { container } = start();
  act(() => { q(container, '[data-context="ctx"] button[data-select="context"]')!.click(); });
  expect(q(container, 'section[aria-label="Plan pane"] [data-manager-view="ctx"]')).not.toBeNull();
  expect(q(container, 'section[aria-label="Plan pane"] [data-manager-plan="plan"]')).not.toBeNull();
  expect(q(container, 'section[aria-label="Plan pane"] section[aria-label^="Plan "]')).toBeNull();
  expect(q(container, 'section[aria-label="Plan pane"] [data-part="flow-canvas"]')).toBeNull();
});

it("selecting a plan in ManagerView shows its PlanView and Lead", () => {
  const { transport, container } = start();
  act(() => { q(container, '[data-context="ctx"] button[data-select="context"]')!.click(); });
  act(() => { q(container, 'section[aria-label="Plan pane"] [data-manager-plan="plan"] [role="button"]')!.click(); });
  act(() => transport.emitMessage({ ...snapshot, plan: loadedPlan }));
  expect(q(container, 'section[aria-label="Plan pane"] [data-manager-view]')).toBeNull();
  expect(q(container, 'section[aria-label="Plan pane"] section[aria-label="Plan plan"]')).not.toBeNull();
  expect(q(container, '[data-session="s1"][aria-current="true"]')).not.toBeNull();
});
