// @vitest-environment jsdom
import { act } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../../protocol/runtime.ts";
import type { ToolItem, TranscriptItem } from "../../protocol/domain.ts";
import { FakeScheduler } from "../../testing/scheduler.ts";
import { mount } from "../../testing/render.tsx";
import { App } from "../App.tsx";
import { Transcript } from "./Transcript.tsx";

// jsdom has no ResizeObserver, which react-resizable-panels (external) needs: an inert double.
vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });

const CUE = '[role="status"][data-part="working-cue"]';
const base = { hasEarlier: false, queued: [], pending: [], onLoadEarlier: () => {}, tailEpoch: 0 };
const user: TranscriptItem = { kind: "user", id: "u1", text: "hello", attachments: [] };
const done: TranscriptItem = { kind: "assistant", id: "a1", streaming: false, blocks: [{ type: "text", text: "answer" }] };
const streaming: TranscriptItem = { kind: "assistant", id: "a2", streaming: true, blocks: [{ type: "text", text: "partial" }] };
const running: ToolItem = { kind: "tool", id: "tool:t1", callId: "t1", name: "bash", args: {}, status: "running" };

it("Transcript shows no Thinking cue when busy and nothing streams", () => {
  const m = mount(<Transcript {...base} busy items={[user, done]} />);
  expect(m.container.querySelector(CUE)).toBeNull();
  expect(m.container.textContent).not.toContain("Thinking…");
  m.unmount();
});

it("Transcript names no running tool in a working cue", () => {
  const m = mount(<Transcript {...base} busy items={[user, running]} />);
  expect(m.container.querySelector(CUE)).toBeNull();
  expect(m.container.textContent).not.toContain("Using bash…");
  m.unmount();
});

it("Transcript shows no working cue when busy is false", () => {
  const m = mount(<Transcript {...base} busy={false} items={[user, done]} />);
  expect(m.container.querySelector(CUE)).toBeNull();
  m.unmount();
});

it("Transcript shows exactly one indicator when the last assistant item streams", () => {
  const m = mount(<Transcript {...base} busy items={[user, streaming]} />);
  expect(m.container.querySelectorAll('[data-part="working"]').length).toBe(1);
  expect(m.container.querySelector(CUE)).toBeNull();
  expect(m.container.querySelector('[role="status"][data-part="working"]')?.textContent).toBe("Working…");
  m.unmount();
});

// ---- ChatPane through App: FakeTransport and the reader double, as in App.test.tsx
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
function ref(state: "idle" | "working") {
  return { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state, title: "My chat" } as const;
}
function snapshot(state: "idle" | "working") {
  const node = { ref: ref(state), contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state, attention: null } };
  return {
    type: "cockpit.snapshot",
    tree: {
      cursor: 1,
      contexts: [{ id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [],
        plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node] }] }],
    },
    plan: { planId: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
    inbox: { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 1 },
  };
}

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

function open(state: "idle" | "working") {
  const transport = new FakeTransport();
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
  mounted = view;
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot(state)));
  act(() => { view.container.querySelector<HTMLButtonElement>('button[data-select="plan"]')!.click(); });
  act(() => {
    transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "My chat", meta: { ref: ref(state), footer } });
    transport.emitMessage({
      type: "session.window", sessionId: "s1", mode: "tail",
      window: { items: [{ kind: "user", id: "u:1", text: "hello tail", attachments: [] }], hasEarlier: false, cursor: null },
    });
  });
  return { transport, container: view.container };
}

const PULSE = '[data-slot="input-group"].prompt-border-pulse';

it("ChatPane shows no working cue and no pulse for a static working state alone", () => {
  const { container } = open("working");
  expect(container.querySelector(CUE)).toBeNull();
  expect(container.querySelector(PULSE), "pro-d58: only view.busy or a pending send pulses the prompt").toBeNull();
});

it("ChatPane shows no working cue when the session is idle", () => {
  const { container } = open("idle");
  expect(container.querySelector(CUE)).toBeNull();
  expect(container.querySelector(PULSE)).toBeNull();
});

it("ChatPane pulses the prompt border while a submission is sending", () => {
  const { container } = open("idle");
  const ta = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, "do it");
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  });
  act(() => { ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
  expect(container.querySelector('li[data-pending="sending"]')).not.toBeNull();
  expect(container.querySelector(CUE)).toBeNull();
  expect(container.querySelector(PULSE)).not.toBeNull();
});
