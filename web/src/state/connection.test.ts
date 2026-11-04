import { describe, expect, it } from "vitest";
import type { Disposable, Transport } from "../protocol/runtime.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { createConnection } from "./connection.ts";
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
});
