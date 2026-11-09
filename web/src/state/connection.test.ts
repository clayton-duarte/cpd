import { describe, expect, it, vi } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { FRAME_MS, PENDING_ECHO_MS, createConnection } from "./connection.ts";
import { createStore } from "./store.ts";

class Fake implements Transport {
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
  emitMessage(raw: unknown): void { for (const h of this.messageHandlers) h(raw); }
  emitStatus(s: "open" | "closed"): void { if (!this.closed) for (const h of this.statusHandlers) h(s); }
}

const setup = () => {
  const transport = new Fake();
  const scheduler = new FakeScheduler();
  const store = createStore();
  const connection = createConnection({ transport, scheduler, store, newId: () => "id1" });
  return { transport, scheduler, store, connection };
};

describe("connection", () => {
  it("connect subscribes to the cockpit and reattaches the selected session", () => {
    const { transport, store, connection } = setup();
    connection.start();
    expect(store.getState().connection).toBe("connecting");
    expect(transport.connects).toBe(1);
    connection.selectSession("s1");
    transport.emitStatus("open");
    expect(store.getState().connection).toBe("open");
    expect(transport.sent).toEqual([{ type: "cockpit.subscribe" }, { type: "session.attach", sessionId: "s1" }]);
    transport.sent.length = 0;
    connection.selectSession("s2");
    expect(transport.sent).toEqual([{ type: "session.detach" }, { type: "session.attach", sessionId: "s2" }]);
    transport.sent.length = 0;
    connection.selectSession("s2");
    expect(transport.sent).toEqual([]);
    expect(connection.submit({ text: "hi", images: [], whenBusy: "followUp" })).toBe("id1");
    expect(transport.sent).toEqual([
      { type: "session.submit", sessionId: "s2", clientSubmissionId: "id1", text: "hi", images: [], whenBusy: "followUp" },
    ]);
    expect(store.getState().session?.pending[0]).toMatchObject({ clientSubmissionId: "id1", state: "sending" });
  });

  it("the client re-subscribes after a hub error", () => {
    const { transport, scheduler, store, connection } = setup();
    connection.start();
    transport.emitStatus("open");
    expect(transport.sent).toEqual([{ type: "cockpit.subscribe" }]);

    // The hub answers the ws start gate's race: a subscribe that arrives while the cockpit service
    // is still starting is refused 'cockpit not ready' and is never subscribed (web/CONTRACT.md
    // §20.1). If the client sends `cockpit.subscribe` only when the transport opens, this one
    // error leaves the cockpit empty — no tree, no inbox, no status bar, no transcript — until a
    // full page reload.
    transport.emitMessage({ type: "cockpit.error", error: { code: "internal", message: "cockpit not ready" } });
    expect(store.getState().cockpit.error?.message).toBe("cockpit not ready");
    // The re-subscribe is backed off, not sent synchronously.
    expect(transport.sent).toEqual([{ type: "cockpit.subscribe" }]);

    // After the backoff the client re-sends `cockpit.subscribe`, so the readiness window heals.
    scheduler.advance(10_000);
    const subscribes = transport.sent.filter((m) => (m as { type?: string }).type === "cockpit.subscribe");
    expect(subscribes.length).toBeGreaterThan(1);

    // The later snapshot recovers the cockpit: live data fills and the error clears.
    const tree = { contexts: [], cursor: 1 };
    const inbox = { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 0 };
    transport.emitMessage({ type: "cockpit.snapshot", tree, plan: null, inbox });
    expect(store.getState().cockpit.tree).toBe(tree);
    expect(store.getState().cockpit.inbox).toEqual(inbox);
    expect(store.getState().cockpit.error).toBeNull();
  });

  it("reconnect backs off through the injected scheduler", () => {
    const { transport, scheduler, store, connection } = setup();
    connection.start();
    transport.emitStatus("open");
    transport.emitStatus("closed");
    expect(store.getState().connection).toBe("reconnecting");
    expect(store.getState().retryAttempt).toBe(0);
    expect(scheduler.pending()).toBe(1);
    scheduler.advance(499);
    expect(transport.connects).toBe(1);
    scheduler.advance(1);
    expect(transport.connects).toBe(2);
    transport.emitStatus("closed");
    expect(store.getState().retryAttempt).toBe(1);
    scheduler.advance(999);
    expect(transport.connects).toBe(2);
    scheduler.advance(1);
    expect(transport.connects).toBe(3);
    transport.emitStatus("open");
    expect(store.getState().retryAttempt).toBe(0);
    expect(store.getState().connection).toBe("open");
    transport.emitStatus("closed");
    expect(scheduler.pending()).toBe(1);
    scheduler.advance(500);
    expect(transport.connects).toBe(4);
  });

  it("invalid server messages are rejected loudly", () => {
    const { transport, store, connection } = setup();
    connection.start();
    transport.emitStatus("open");
    transport.emitMessage({ type: "definitely.not.a.message" });
    expect(store.getState().lastError?.code).toBe("protocol_error");
    expect(store.getState().connection).toBe("open");
    transport.emitMessage("not an object");
    expect(store.getState().lastError?.code).toBe("protocol_error");
  });

  it("session.completion is accepted when well formed and rejected when a key is missing", () => {
    const { transport, store, connection } = setup();
    connection.start();
    transport.emitStatus("open");
    const completion = { label: "open_work", text: "Open work · 40%", score: 40, tone: "decision" };
    transport.emitMessage({ type: "session.completion", sessionId: "s1", completion });
    expect(store.getState().lastError).toBeNull();
    transport.emitMessage({ type: "session.completion", completion });
    expect(store.getState().lastError?.code).toBe("protocol_error");
    transport.emitMessage({ type: "session.completion", sessionId: "s1" });
    expect(store.getState().lastError?.code).toBe("protocol_error");
  });

  it("close stops reconnecting", () => {
    const { transport, scheduler, store, connection } = setup();
    connection.start();
    transport.emitStatus("open");
    transport.emitStatus("closed");
    expect(scheduler.pending()).toBe(1);
    connection.close();
    expect(scheduler.pending()).toBe(0);
    expect(transport.closed).toBe(true);
    expect(store.getState().connection).toBe("closed");
    transport.emitStatus("closed");
    scheduler.advance(60_000);
    expect(transport.connects).toBe(1);
    expect(scheduler.pending()).toBe(0);
    connection.close();
    expect(store.getState().connection).toBe("closed");
  });

  const texts = (store: ReturnType<typeof createStore>): string[] =>
    ((store.getState().session?.transcript.items[0] as { blocks: { text: string }[] } | undefined)?.blocks ?? []).map((b) => b.text);
  const delta = (text: string, sessionId = "s1") => ({ type: "session.delta", sessionId, itemId: "a:1", blockIndex: 0, blockType: "text", text });
  const attachedSession = () => {
    const ctx = setup();
    ctx.connection.start();
    ctx.connection.selectSession("s1");
    ctx.transport.emitStatus("open");
    ctx.transport.emitMessage({
      type: "session.window", sessionId: "s1", mode: "tail",
      window: { items: [{ kind: "assistant", id: "a:1", blocks: [{ type: "text", text: "" }], streaming: true }], hasEarlier: false, cursor: null },
    });
    return ctx;
  };

  it("session.append and session.delta are applied once per frame", () => {
    const { transport, scheduler, store } = attachedSession();
    const listener = vi.fn();
    store.subscribe(listener);
    transport.emitMessage(delta("a"));
    transport.emitMessage(delta("b"));
    transport.emitMessage({ type: "session.append", sessionId: "s1", items: [{ kind: "user", id: "u:1", text: "x", attachments: [] }] });
    expect(FRAME_MS).toBe(16);
    expect(listener).not.toHaveBeenCalled();
    expect(texts(store)).toEqual([""]);
    scheduler.advance(FRAME_MS - 1);
    expect(listener).not.toHaveBeenCalled();
    scheduler.advance(1);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(texts(store)).toEqual(["ab"]);
    expect(store.getState().session?.transcript.items.map((i) => i.id)).toEqual(["a:1", "u:1"]);
    expect(scheduler.pending()).toBe(0);
  });

  it("adjacent deltas of one block are merged", () => {
    const { transport, scheduler, store } = attachedSession();
    const spy = vi.spyOn(store, "dispatch");
    transport.emitMessage(delta("a"));
    transport.emitMessage(delta("b"));
    transport.emitMessage(delta("c"));
    scheduler.advance(FRAME_MS);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith({ type: "serverBatch", messages: [delta("abc")] });
  });

  it("a non-batched message flushes the buffer first and keeps order", () => {
    const { transport, scheduler, store } = attachedSession();
    const spy = vi.spyOn(store, "dispatch");
    transport.emitMessage(delta("a"));
    transport.emitMessage({ type: "session.status", sessionId: "s1", busy: true, queued: [] });
    expect(spy.mock.calls.map(([a]) => a.type)).toEqual(["serverBatch", "server"]);
    expect(texts(store)).toEqual(["a"]);
    expect(store.getState().session?.busy).toBe(true);
    expect(scheduler.pending()).toBe(0);
  });

  it("selecting another session drops buffered messages", () => {
    const { transport, scheduler, store, connection } = attachedSession();
    const spy = vi.spyOn(store, "dispatch");
    transport.emitMessage(delta("a"));
    expect(scheduler.pending()).toBe(1);
    connection.selectSession("s2");
    expect(scheduler.pending()).toBe(0);
    scheduler.advance(FRAME_MS);
    expect(spy.mock.calls.map(([a]) => a.type)).toEqual(["selectSession"]);
  });

  it("close drops buffered messages and the timer", () => {
    const { transport, scheduler, store, connection } = attachedSession();
    const spy = vi.spyOn(store, "dispatch");
    transport.emitMessage(delta("a"));
    expect(scheduler.pending()).toBe(1);
    connection.close();
    expect(scheduler.pending()).toBe(0);
    scheduler.advance(FRAME_MS);
    expect(spy.mock.calls.map(([a]) => a.type)).toEqual(["connection"]);
  });

  it("a sent pending row is dismissed after 30 seconds", () => {
    const { transport, scheduler, store, connection } = attachedSession();
    connection.submit({ text: "hi", images: [], whenBusy: "followUp" });
    transport.emitMessage({ type: "session.submitAck", sessionId: "s1", clientSubmissionId: "id1", queued: false });
    expect(store.getState().session?.pending[0]?.state).toBe("sent");
    expect(PENDING_ECHO_MS).toBe(30000);
    scheduler.advance(PENDING_ECHO_MS - 1);
    expect(store.getState().session?.pending).toHaveLength(1);
    scheduler.advance(1);
    expect(store.getState().session?.pending).toEqual([]);
    expect(scheduler.pending()).toBe(0);
  });

  it("an action result with a new Plan or Session id selects it", () => {
    const { transport, store, connection } = setup();
    connection.start();
    transport.emitStatus("open");
    transport.sent.length = 0;
    // plan new: the reply carries the Plan and its Lead session
    transport.emitMessage({ type: "action.result", actionId: "a1", ok: true, output: { plan: { id: "my-plan" }, lead: { session: "s-lead9" } } });
    expect(store.getState().cockpit.selectedPlanId).toBe("my-plan");
    expect(store.getState().selectedSessionId).toBe("s-lead9");
    expect(transport.sent).toEqual([
      { type: "cockpit.selectPlan", planId: "my-plan" },
      { type: "session.attach", sessionId: "s-lead9" },
    ]);
    transport.sent.length = 0;
    // session fork: the reply carries the new session; the previous one is detached
    transport.emitMessage({ type: "action.result", actionId: "a2", ok: true, output: { session: { id: "s-fork" } } });
    expect(store.getState().selectedSessionId).toBe("s-fork");
    expect(store.getState().cockpit.selectedPlanId).toBe("my-plan");
    expect(transport.sent).toEqual([{ type: "session.detach" }, { type: "session.attach", sessionId: "s-fork" }]);
    transport.sent.length = 0;
    // a result without ids, and a failed result, change nothing
    transport.emitMessage({ type: "action.result", actionId: "a3", ok: true, output: { done: true } });
    transport.emitMessage({ type: "action.result", actionId: "a4", ok: false, error: { code: "cpd_failed", message: "no" } });
    expect(store.getState().selectedSessionId).toBe("s-fork");
    expect(transport.sent).toEqual([]);
  });
});

describe("connection optimistic action", () => {
  it("connection_dispatches_optimistic_before_send", () => {
    const { transport, store, connection } = setup();
    const row = { id: 1, kind: "pr-ready", label: "ready to merge", urgency: "normal", contextId: null, planId: "p1", jobKey: null, sessionId: null, url: null, subject: "pr:acme/api#3", workflow: null, openedAt: "t", ageSeconds: 0, actions: [] };
    const inbox = { openCount: 1, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [row], cursor: 1 };
    connection.start();
    transport.emitStatus("open");
    transport.emitMessage({ type: "cockpit.patch", pane: "inbox", data: inbox });
    expect(store.getState().cockpit.inbox?.rows).toHaveLength(1);
    const seen: number[] = [];
    const send = transport.send.bind(transport);
    transport.send = (raw: unknown): void => {
      if ((raw as { type?: string }).type === "cockpit.action") seen.push(store.getState().cockpit.inbox?.rows.length ?? -1);
      send(raw);
    };
    connection.action({ kind: "pr.merge", planId: "p1", pull: "acme/api#3" });
    expect(seen).toEqual([0]);
  });

  it("selectSession on an already selected but unattached session re-sends session.attach, and sends nothing when attached", () => {
    const { transport, store, connection } = setup();
    connection.start();
    connection.selectSession("s1");
    transport.emitStatus("open");
    transport.emitMessage({ type: "session.error", sessionId: "s1", error: { code: "session_not_found", message: "no" } });
    expect(store.getState().session?.attached).toBe(false);
    transport.sent.length = 0;
    connection.selectSession("s1");
    expect(transport.sent).toEqual([{ type: "session.attach", sessionId: "s1" }]);
    expect(store.getState().session?.error).toBeNull();
    transport.emitMessage({
      type: "session.window", sessionId: "s1", mode: "tail",
      window: { items: [], hasEarlier: false, cursor: null },
    });
    expect(store.getState().session?.attached).toBe(true);
    transport.sent.length = 0;
    connection.selectSession("s1");
    expect(transport.sent).toEqual([]);
  });

  it("cockpit.patch tree and cockpit.snapshot retry session.attach after session_not_found only", () => {
    const treeModel = { contexts: [], cursor: 1 };
    const inboxModel = { openCount: 0, engineRunning: true, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, rows: [], cursor: 0 };
    const patch = { type: "cockpit.patch", pane: "tree", data: treeModel };
    const snapshot = { type: "cockpit.snapshot", tree: treeModel, plan: null, inbox: inboxModel };
    const attach = { type: "session.attach", sessionId: "s1" };
    const notFound = { type: "session.error", sessionId: "s1", error: { code: "session_not_found", message: "no" } };
    const opened = () => {
      const ctx = setup();
      ctx.connection.start();
      ctx.connection.selectSession("s1");
      ctx.transport.emitStatus("open");
      return ctx;
    };
    // session_not_found: one attach per tree patch and per snapshot
    const a = opened();
    a.transport.emitMessage(notFound);
    a.transport.sent.length = 0;
    a.transport.emitMessage(patch);
    expect(a.transport.sent).toEqual([attach]);
    a.transport.emitMessage(snapshot);
    expect(a.transport.sent).toEqual([attach, attach]);
    // another error code: no retry
    const b = opened();
    b.transport.emitMessage({ type: "session.error", sessionId: "s1", error: { code: "cpd_failed", message: "x" } });
    b.transport.sent.length = 0;
    b.transport.emitMessage(patch);
    b.transport.emitMessage(snapshot);
    expect(b.transport.sent).toEqual([]);
    // attached: no retry
    const c = opened();
    c.transport.emitMessage(notFound);
    c.transport.emitMessage({ type: "session.window", sessionId: "s1", mode: "tail", window: { items: [], hasEarlier: false, cursor: null } });
    c.transport.sent.length = 0;
    c.transport.emitMessage(patch);
    c.transport.emitMessage(snapshot);
    expect(c.transport.sent).toEqual([]);
    // no session selected: no retry
    const d = setup();
    d.connection.start();
    d.transport.emitStatus("open");
    d.transport.sent.length = 0;
    d.transport.emitMessage(patch);
    d.transport.emitMessage(snapshot);
    expect(d.transport.sent).toEqual([]);
  });
});
