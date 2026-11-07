import { describe, expect, it } from "vitest";
import { createConnection } from "../state/connection.ts";
import { createStore } from "../state/store.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import { createWebSocketTransport, type WebSocketFactory, type WebSocketLike } from "./ws-transport.ts";

/** A hand-rolled browser WebSocket: records what the transport sends, drives its handlers. */
class FakeWebSocket implements WebSocketLike {
  readonly readyState = 1;
  sent: string[] = [];
  closed = false;
  private readonly openHandlers: Array<() => void> = [];
  private readonly messageHandlers: Array<(event: { data: unknown }) => void> = [];
  private readonly closeHandlers: Array<() => void> = [];
  private readonly errorHandlers: Array<() => void> = [];

  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.closed = true;
  }
  addEventListener(type: "open", listener: () => void): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  addEventListener(type: "close", listener: () => void): void;
  addEventListener(type: "error", listener: () => void): void;
  addEventListener(type: "open" | "message" | "close" | "error", listener: ((event: { data: unknown }) => void) | (() => void)): void {
    if (type === "open") this.openHandlers.push(listener as () => void);
    else if (type === "message") this.messageHandlers.push(listener as (event: { data: unknown }) => void);
    else if (type === "close") this.closeHandlers.push(listener as () => void);
    else this.errorHandlers.push(listener as () => void);
  }

  emitOpen(): void {
    for (const h of [...this.openHandlers]) h();
  }
  emitMessage(data: unknown): void {
    for (const h of [...this.messageHandlers]) h({ data });
  }
  emitClose(): void {
    for (const h of [...this.closeHandlers]) h();
  }
  emitError(): void {
    for (const h of [...this.errorHandlers]) h();
  }
}

/** One fake socket per `connect`, in order; no URL is ever opened. */
function fakeFactory() {
  const sockets: FakeWebSocket[] = [];
  const urls: string[] = [];
  const create: WebSocketFactory = (url) => {
    urls.push(url);
    const socket = new FakeWebSocket();
    sockets.push(socket);
    return socket;
  };
  return { create, sockets, urls };
}

describe("websocket transport", () => {
  it("delivers server messages to the handler", () => {
    const f = fakeFactory();
    const t = createWebSocketTransport("ws://127.0.0.1:1234/ws", f.create);
    const got: unknown[] = [];
    t.onMessage((raw) => got.push(raw));
    t.connect();
    f.sockets[0]!.emitOpen();
    const message = { type: "cockpit.error", error: { code: "internal", message: "boom" } };
    f.sockets[0]!.emitMessage(JSON.stringify(message));
    expect(got).toEqual([message]);
  });

  it("emits closed when the socket closes", () => {
    const f = fakeFactory();
    const t = createWebSocketTransport("ws://127.0.0.1:1234/ws", f.create);
    const statuses: string[] = [];
    t.onStatus((s) => statuses.push(s));
    t.connect();
    f.sockets[0]!.emitOpen();
    f.sockets[0]!.emitClose();
    expect(statuses).toEqual(["open", "closed"]);
  });

  it("surfaces a stalled peer as a loud error", () => {
    const f = fakeFactory();
    const t = createWebSocketTransport("ws://127.0.0.1:1234/ws", f.create);
    const statuses: string[] = [];
    t.onStatus((s) => statuses.push(s));
    t.connect();
    f.sockets[0]!.emitOpen();
    f.sockets[0]!.emitError();
    expect(statuses).toEqual(["open", "closed"]);
    expect(() => t.send({ type: "cockpit.subscribe" })).toThrow("transport is not open");
  });

  it("resubscribes and resyncs after a reconnect", () => {
    const f = fakeFactory();
    const transport = createWebSocketTransport("ws://127.0.0.1:1234/ws", f.create);
    const scheduler = new FakeScheduler();
    const store = createStore();
    const connection = createConnection({ transport, scheduler, store });

    connection.start();
    expect(f.sockets).toHaveLength(1);
    f.sockets[0]!.emitOpen();
    expect(JSON.parse(f.sockets[0]!.sent[0]!)).toEqual({ type: "cockpit.subscribe" });

    f.sockets[0]!.emitClose();
    expect(store.getState().connection).toBe("reconnecting");
    scheduler.advance(500);
    expect(f.sockets).toHaveLength(2);
    f.sockets[1]!.emitOpen();
    expect(JSON.parse(f.sockets[1]!.sent[0]!)).toEqual({ type: "cockpit.subscribe" });

    const snapshot = { type: "cockpit.snapshot", tree: { plans: [] }, plan: null, inbox: { rows: [], cursor: 0 } };
    f.sockets[1]!.emitMessage(JSON.stringify(snapshot));
    expect(store.getState().connection).toBe("open");
    expect(store.getState().cockpit.tree).toEqual({ plans: [] });
  });
});
