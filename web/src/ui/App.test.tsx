// @vitest-environment jsdom
import { act } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
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
const node = { ref, contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const snapshot = {
  type: "cockpit.snapshot",
  tree: {
    cursor: 1,
    contexts: [{ id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [],
      plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: "s1", sessions: [node] }] }],
  },
  plan: {
    planId: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", counts, review: null, openEscalations: 0,
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

function attach(transport: FakeTransport, container: HTMLElement) {
  const button = container.querySelector<HTMLButtonElement>('button[data-select="plan"]');
  expect(button).not.toBeNull();
  act(() => { button!.click(); });
  act(() => {
    transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "My chat", meta: { ref, footer } });
    transport.emitMessage({
      type: "session.window", sessionId: "s1", mode: "tail",
      window: { items: [{ kind: "user", id: "u:1", text: "hello tail", attachments: [] }], hasEarlier: false, cursor: null },
    });
  });
}

function typeAndEnter(container: HTMLElement, text: string) {
  const ta = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
  expect(ta).not.toBeNull();
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, text);
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  });
  act(() => {
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  });
}

describe("App", () => {
  it("app subscribes to the cockpit and renders the three panes", () => {
    const { transport, container } = start();
    expect(transport.sent).toContainEqual({ type: "cockpit.subscribe" });
    expect(container.querySelector('[aria-label="Context tree"] [data-sidebar="content"], [aria-label="Context tree"] [data-slot="sidebar-content"]')?.textContent).toContain("plan");
    attach(transport, container);
    expect(container.querySelector('section[aria-label="Plan pane"] section[aria-label="Plan plan"]')).not.toBeNull();
    expect(container.querySelector('aside[aria-label="Inbox"]')?.textContent).toContain("up to date, drink some water");
  });

  it("selecting a session attaches and renders its transcript tail", () => {
    const { transport, container } = start();
    attach(transport, container);
    expect(transport.sent).toContainEqual({ type: "session.attach", sessionId: "s1" });
    const log = container.querySelector('main[aria-label="Chat"] [role="log"]');
    expect(log?.textContent).toContain("hello tail");
  });

  it("submit sends a session.submit message through the transport", () => {
    const { transport, container } = start();
    attach(transport, container);
    typeAndEnter(container, "do the thing");
    expect(transport.sent).toContainEqual({
      type: "session.submit", sessionId: "s1", clientSubmissionId: "c1", text: "do the thing", images: [], whenBusy: "followUp",
    });
  });

  it("a dialog message renders the dialog card and answering sends dialog.respond", () => {
    const { transport, container } = start();
    attach(transport, container);
    act(() => transport.emitMessage({
      type: "session.dialog", sessionId: "s1",
      dialog: { id: "d1", interactive: true, method: "cpd_plan_review", title: "Ship it", summary: "All good" },
    }));
    const card = container.querySelector('[data-dialog="cpd_plan_review"]');
    expect(card?.textContent).toContain("Ship it");
    act(() => { card!.querySelector<HTMLButtonElement>('[data-action="approve"]')!.click(); });
    expect(transport.sent).toContainEqual({
      type: "dialog.respond", sessionId: "s1", dialogId: "d1", response: { method: "cpd_plan_review", outcome: "approve" },
    });
  });

  it("dropping an image sends it as an attachment", async () => {
    const { transport, container } = start();
    attach(transport, container);
    const zone = container.querySelector('[data-chat-dropzone]')!;
    expect(zone).not.toBeNull();
    const file = { name: "pic.png", type: "image/png", size: 3 };
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [file], types: ["Files"] } });
    await act(async () => { zone.dispatchEvent(drop); });
    expect(container.querySelector('[data-attachment="image"]')).not.toBeNull();
    typeAndEnter(container, "see image");
    expect(transport.sent).toContainEqual({
      type: "session.submit", sessionId: "s1", clientSubmissionId: "c1", text: "see image",
      images: [{ mimeType: "image/png", dataBase64: "QUJD" }], whenBusy: "followUp",
    });
  });

  it("preserves a blocked target's draft and attachments until its correlated acknowledgement without duplicate sends", async () => {
    const { transport, container } = start();
    const blockedRef = { ...ref, state: "blocked", key: "cpd:worker-loop" };
    const blockedNode = { ...node, ref: blockedRef, dot: { state: "blocked", attention: null } };
    const blockedPlan = { ...snapshot.tree.contexts[0]!.plans[0]!, sessions: [blockedNode] };
    const tree = { ...snapshot.tree, contexts: [{ ...snapshot.tree.contexts[0]!, plans: [blockedPlan] }] };
    act(() => transport.emitMessage({ ...snapshot, tree }));
    act(() => { container.querySelector<HTMLButtonElement>('button[data-select="plan"]')!.click(); });
    act(() => transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "worker-loop", meta: { ref: blockedRef, footer } }));
    const zone = container.querySelector('[data-chat-dropzone]')!;
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [{ name: "pic.png", type: "image/png", size: 3 }], types: ["Files"] } });
    await act(async () => { zone.dispatchEvent(drop); });
    typeAndEnter(container, "do not lose this");
    const ta = container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
    expect(ta.value).toBe("do not lose this");
    expect(container.querySelector('[aria-label="Staged attachments"] [data-attachment="image"]')).not.toBeNull();
    expect(container.querySelector('[data-part="submission-feedback"]')?.textContent).toMatch(/cpd:worker-loop.*(pending|awaiting)/i);
    act(() => { ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
    expect(transport.sent.filter((m) => (m as { type?: string }).type === "session.submit")).toHaveLength(1);
    act(() => transport.emitMessage({ type: "session.submitAck", sessionId: "s1", clientSubmissionId: "other", queued: false }));
    expect(ta.value).toBe("do not lose this");
    act(() => transport.emitMessage({ type: "session.submitAck", sessionId: "s1", clientSubmissionId: "c1", queued: false }));
    expect(ta.value).toBe("");
    expect(container.querySelector('[aria-label="Staged attachments"]')).toBeNull();
    expect(container.querySelector('[data-part="submission-feedback"]')?.textContent ?? "").not.toMatch(/delivered|restored/i);
  });

  it("keeps the draft and attachment on a correlated send failure and does not resend automatically", async () => {
    const { transport, container } = start();
    attach(transport, container);
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [{ name: "pic.png", type: "image/png", size: 3 }], types: ["Files"] } });
    await act(async () => { container.querySelector('[data-chat-dropzone]')!.dispatchEvent(drop); });
    typeAndEnter(container, "keep me");
    act(() => transport.emitMessage({ type: "session.error", sessionId: "s1", clientSubmissionId: "c1", error: { code: "cpd_failed", message: "send failed" } }));
    expect(container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!.value).toBe("keep me");
    expect(container.querySelector('[aria-label="Staged attachments"] [data-attachment="image"]')).not.toBeNull();
    expect(container.querySelector('[data-part="submission-feedback"]')?.textContent).toMatch(/send failed/i);
    expect(transport.sent.filter((m) => (m as { type?: string }).type === "session.submit")).toHaveLength(1);
  });

  it("reports an unknown outcome on disconnect and never replays the retained draft", async () => {
    const { transport, container } = start();
    attach(transport, container);
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [{ name: "pic.png", type: "image/png", size: 3 }], types: ["Files"] } });
    await act(async () => { container.querySelector('[data-chat-dropzone]')!.dispatchEvent(drop); });
    typeAndEnter(container, "uncertain");
    act(() => transport.emitStatus("closed"));
    expect(container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!.value).toBe("uncertain");
    expect(container.querySelector('[aria-label="Staged attachments"] [data-attachment="image"]')).not.toBeNull();
    expect(container.querySelector('[data-part="submission-feedback"]')?.textContent).toMatch(/outcome is unknown/i);
    act(() => transport.emitStatus("open"));
    expect(transport.sent.filter((m) => (m as { type?: string }).type === "session.submit")).toHaveLength(1);
  });

  it("selecting an archived session disables the Composer", () => {
    const transport = new FakeTransport();
    const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => "c1"} />);
    mounted = view;
    const archivedNode = { ...node, archived: true };
    const archivedPlan = { ...snapshot.tree.contexts[0]!.plans[0]!, archived: true, sessions: [archivedNode] };
    const tree = { ...snapshot.tree, contexts: [{ ...snapshot.tree.contexts[0]!, plans: [archivedPlan] }] };
    act(() => transport.emitStatus("open"));
    act(() => transport.emitMessage({ ...snapshot, tree }));
    const trigger = view.container.querySelector<HTMLButtonElement>('[data-section="archive"] button');
    expect(trigger).not.toBeNull();
    act(() => { trigger!.click(); });
    const row = view.container.querySelector<HTMLButtonElement>('[data-section="archive"] button[data-select="plan"]');
    expect(row).not.toBeNull();
    act(() => { row!.click(); });
    act(() => {
      transport.emitMessage({ type: "session.attached", sessionId: "s1", capabilities, title: "My chat", meta: { ref, footer } });
    });
    const ta = view.container.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]');
    expect(ta).not.toBeNull();
    expect(ta!.disabled).toBe(true);
  });
});
