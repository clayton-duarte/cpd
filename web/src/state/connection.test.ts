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
});
