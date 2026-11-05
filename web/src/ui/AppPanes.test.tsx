// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";
import { App } from "./App.tsx";

vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const broken = vi.hoisted(() => ({ panes: new Set<string>() }));
vi.mock("./cockpit/ContextTree.tsx", () => ({
  ContextTree: () => { if (broken.panes.has("ContextTree")) throw new Error("ContextTree exploded"); return <div data-pane="ContextTree" />; },
}));
vi.mock("./cockpit/PlanView.tsx", () => ({
  PlanView: () => { if (broken.panes.has("PlanView")) throw new Error("PlanView exploded"); return <div data-pane="PlanView" />; },
}));
vi.mock("./cockpit/Inbox.tsx", () => ({
  Inbox: () => { if (broken.panes.has("Inbox")) throw new Error("Inbox exploded"); return <div data-pane="Inbox" />; },
}));
vi.mock("./ChatPane.tsx", () => ({
  ChatPane: () => { if (broken.panes.has("ChatPane")) throw new Error("ChatPane exploded"); return <div data-pane="ChatPane" />; },
}));

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

const reader = { readBase64: async () => "QUJD", readText: async () => "text" };
const ref = { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" } as const;
const node = { ref, contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const snapshot = {
  type: "cockpit.snapshot",
  tree: {
    cursor: 1,
    contexts: [{ id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [],
      plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node] }] }],
  },
  plan: { planId: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
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
beforeEach(() => { vi.spyOn(console, "error").mockImplementation(() => {}); });
afterEach(() => { mounted?.unmount(); mounted = null; broken.panes.clear(); vi.restoreAllMocks(); });

function start(onCrash?: (r: unknown) => void): HTMLElement {
  const transport = new FakeTransport();
  const props = { transport, scheduler: new FakeScheduler(), reader, newId: () => "c1" };
  // App's optional onCrash prop does not exist yet: spread it untyped.
  const extra: object = onCrash === undefined ? {} : { onCrash };
  const view = mount(<App {...props} {...extra} />);
  mounted = view;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot));
  act(() => {
    transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "My chat", meta: { ref, footer } });
    transport.emitMessage({
      type: "session.window", sessionId: "s1", mode: "tail",
      window: { items: [{ kind: "user", id: "u:1", text: "hello tail", attachments: [] }], hasEarlier: false, cursor: null },
    });
  });
  return view.container;
}

it("a throwing ContextTree leaves PlanView, ChatPane and Inbox mounted", () => {
  broken.panes.add("ContextTree");
  const container = start();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain("ContextTree");
  for (const pane of ["PlanView", "ChatPane", "Inbox"]) {
    expect(container.querySelector(`[data-pane="${pane}"]`), pane).not.toBeNull();
  }
  expect(container.querySelector('[data-pane="ContextTree"]')).toBeNull();
});

it("each pane boundary is named after its pane", () => {
  for (const pane of ["ContextTree", "PlanView", "ChatPane", "Inbox"]) {
    broken.panes.clear();
    broken.panes.add(pane);
    const onCrash = vi.fn();
    const container = start(onCrash);
    const alerts = [...container.querySelectorAll('[role="alert"]')];
    expect(alerts.some((a) => a.textContent?.includes(pane)), pane).toBe(true);
    expect(alerts.some((a) => a.textContent?.includes(`${pane} exploded`)), pane).toBe(true);
    expect(onCrash, pane).toHaveBeenCalledWith(expect.objectContaining({ source: "error-boundary", error: `${pane} exploded` }));
    mounted?.unmount();
    mounted = null;
  }
});
