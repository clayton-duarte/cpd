// @vitest-environment jsdom
import { act } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { mount } from "../testing/render.tsx";

const renders = { tree: 0, plan: 0, inbox: 0 };
vi.mock("./cockpit/ContextTree.tsx", async () => {
  const actual = await vi.importActual<typeof import("./cockpit/ContextTree.tsx")>("./cockpit/ContextTree.tsx");
  return {
    ...actual,
    ContextTree: (props: { onSelect?: (id: string) => void; sessionId?: string }) => {
      renders.tree++;
      return <nav aria-label="Context tree"><button data-session="s1" onClick={() => (props as { onSelectSession?: (id: string) => void }).onSelectSession?.("s1")}>s1</button></nav>;
    },
  };
});
vi.mock("./cockpit/PlanView.tsx", () => ({ PlanView: () => { renders.plan++; return <section data-marker="plan" />; } }));
vi.mock("./cockpit/Inbox.tsx", () => ({ Inbox: () => { renders.inbox++; return <aside data-marker="inbox" />; } }));

import { App } from "./App.tsx";

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });


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

const ref = { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" } as const;
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const node = { ref, contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
const snapshot = {
  type: "cockpit.snapshot",
  tree: { cursor: 1, contexts: [{ id: "ctx", title: "C", archived: false, records: 0, openQuestions: 0, sessions: [],
    plans: [{ id: "plan", contextId: "ctx", title: "P", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node] }] }] },
  plan: { planId: "plan", contextId: "ctx", title: "P", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
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
beforeEach(() => { renders.tree = 0; renders.plan = 0; renders.inbox = 0; });
afterEach(() => { mounted?.unmount(); mounted = null; });

it("streaming deltas do not re-render the context tree, plan or inbox panes", () => {
  const transport = new FakeTransport();
  const scheduler = new FakeScheduler();
  const view = mount(<App transport={transport} scheduler={scheduler} reader={{ readBase64: async () => "", readText: async () => "" }} newId={() => "c1"} />);
  mounted = view;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot));
  act(() => transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "My chat", meta: { ref, footer } }));
  act(() => transport.emitMessage({
    type: "session.window", sessionId: "s1", mode: "tail",
    window: { items: [{ kind: "user", id: "u:1", text: "hi", attachments: [] }], hasEarlier: false, cursor: null },
  }));
  const settled = { ...renders };
  expect(settled.tree).toBeGreaterThan(0);
  act(() => transport.emitMessage({
    type: "session.append", sessionId: "s1",
    items: [{ kind: "assistant", id: "a:1", streaming: true, blocks: [{ type: "text", text: "" }] }],
  }));
  for (const text of ["a", "b", "c"]) {
    act(() => transport.emitMessage({ type: "session.delta", sessionId: "s1", itemId: "a:1", blockIndex: 0, blockType: "text", text }));
  }
  act(() => scheduler.advance(16));
  expect(renders).toEqual({ tree: 1, plan: 1, inbox: 1 });
});
