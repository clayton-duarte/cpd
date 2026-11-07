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
const ref = { id: "s1", key: "ctx:plan", runner: "durable", role: "lead", state: "idle", title: "My chat" } as const;
const node = { ref, contextId: "ctx", planId: "plan", jobKey: null, archived: false, dot: { state: "idle", attention: null } };
const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const unread = (id: number, label: string, sessionId: string | null) => ({
  id, kind: "unread", label, urgency: "white", contextId: "ctx", planId: "plan", jobKey: null, sessionId, url: null,
  openedAt: "2026-01-01T00:00:00Z", ageSeconds: 5, actions: [],
});
const snapshot = (rows: unknown[], sessionId = "s1") => ({
  type: "cockpit.snapshot",
  tree: {
    cursor: 1,
    contexts: [{ id: "ctx", title: "Context One", archived: false, records: 0, openQuestions: 0, sessions: [],
      plans: [{ id: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", archived: false, counts, leadSessionId: sessionId, sessions: [{ ...node, ref: { ...ref, id: sessionId } }] }] }],
  },
  plan: { planId: "plan", contextId: "ctx", title: "Plan One", phase: "dispatch", counts, review: null, openEscalations: 0, workflows: [], jobs: [], cursor: 1 },
  inbox: { openCount: rows.length, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows, cursor: 1 },
});

let mounted: { unmount(): void } | null = null;
afterEach(() => { mounted?.unmount(); mounted = null; });

const dismissals = (t: FakeTransport) =>
  t.sent.filter((m) => (m as { type?: string; action?: { kind?: string } }).type === "cockpit.action" && (m as { action: { kind: string } }).action.kind === "inbox.dismiss")
    .map((m) => (m as { action: { itemId: number } }).action.itemId);
const rowIds = (c: Element) => Array.from(c.querySelectorAll('aside[aria-label="Inbox"] [data-kind="unread"][data-id]')).map((e) => e.getAttribute("data-id"));

it("app hides and dismisses unread rows of the active session", () => {
  const transport = new FakeTransport();
  let n = 0;
  const view = mount(<App transport={transport} scheduler={new FakeScheduler()} reader={reader} newId={() => `id${++n}`} />);
  mounted = view;
  const rows = [unread(11, "mine", "s1"), unread(12, "elsewhere", "s2"), unread(-1, "synthetic", "s1"), unread(13, "no session", null)];
  act(() => transport.emitStatus("open"));
  act(() => transport.emitMessage(snapshot(rows)));
  // no session is active yet: every unread row shows and nothing is dismissed
  expect(rowIds(view.container)).toEqual(["11", "12", "-1", "13"]);
  expect(dismissals(transport)).toEqual([]);
  // the User opens the Lead chat (session s1)
  const planButton = view.container.querySelector<HTMLButtonElement>('button[data-select="plan"]');
  expect(planButton).not.toBeNull();
  act(() => { planButton!.click(); });
  expect(transport.sent).toContainEqual({ type: "session.attach", sessionId: "s1" });
  // the row of the viewed chat is gone at once and dismissed once; rows of other chats stay;
  // a synthetic row (id < 0) is hidden but cannot be dismissed
  expect(rowIds(view.container)).toEqual(["12", "13"]);
  expect(dismissals(transport)).toEqual([11]);
  // a later snapshot that still carries the row (E8 re-opens it) neither shows it nor dismisses it twice
  act(() => transport.emitMessage(snapshot([...rows, unread(14, "again", "s1")])));
  expect(rowIds(view.container)).toEqual(["12", "13"]);
  expect(dismissals(transport)).toEqual([11, 14]);
  act(() => transport.emitMessage(snapshot([...rows, unread(14, "again", "s1")])));
  expect(dismissals(transport)).toEqual([11, 14]);
});

it("a remounted app does not re-hide an already dismissed unread row", () => {
  const first = new FakeTransport();
  const view = mount(<App transport={first} scheduler={new FakeScheduler()} reader={reader} newId={() => "d1"} />);
  mounted = view;
  act(() => first.emitStatus("open"));
  act(() => first.emitMessage(snapshot([unread(21, "mine", "s7")], "s7")));
  const planButton = view.container.querySelector<HTMLButtonElement>('button[data-select="plan"]');
  expect(planButton).not.toBeNull();
  act(() => { planButton!.click(); });
  expect(dismissals(first)).toEqual([21]);
  view.unmount();
  mounted = null;

  // air-d26: a remount starts a fresh App and store; the dismissed-id set lives in app state, not in the
  // per-mount ref, so the stale row is not hidden again and no second inbox.dismiss is sent after the
  // engine has closed the item.
  const second = new FakeTransport();
  const remount = mount(<App transport={second} scheduler={new FakeScheduler()} reader={reader} newId={() => "d2"} />);
  mounted = remount;
  act(() => second.emitStatus("open"));
  act(() => second.emitMessage(snapshot([unread(21, "mine", "s7")], "s7")));
  const planAgain = remount.container.querySelector<HTMLButtonElement>('button[data-select="plan"]');
  expect(planAgain).not.toBeNull();
  act(() => { planAgain!.click(); });
  expect(dismissals(second)).toEqual([]);
  remount.unmount();
  mounted = null;
});
