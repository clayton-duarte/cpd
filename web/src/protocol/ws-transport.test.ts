import { describe, expect, it } from "vitest";
import { FakeScheduler } from "../testing/scheduler.ts";
import {
  createWebSocketTransport,
  type WebSocketFactory,
  type WebSocketLike,
} from "./ws-transport.ts";

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

const URL = "ws://127.0.0.1:1234/cpd-ws";

describe("websocket transport", () => {
  it("reports open then closed on socket state", () => {
    const f = fakeFactory();
    const transport = createWebSocketTransport({ url: URL, createSocket: f.create, scheduler: new FakeScheduler() });
    const statuses: string[] = [];
    transport.onStatus((s) => statuses.push(s));

    expect(() => transport.send({ type: "cockpit.subscribe" })).toThrow("transport is not open");

    transport.connect();
    expect(f.urls).toEqual([URL]);
    f.sockets[0]!.emitOpen();
    expect(statuses).toEqual(["open"]);

    transport.send({ type: "cockpit.subscribe" });
    expect(JSON.parse(f.sockets[0]!.sent[0]!)).toEqual({ type: "cockpit.subscribe" });

    // A socket close is a real closed status, not a silent stall.
    f.sockets[0]!.emitClose();
    expect(statuses).toEqual(["open", "closed"]);
    expect(() => transport.send({ type: "cockpit.subscribe" })).toThrow("transport is not open");
  });

  it("reports closed on a socket error", () => {
    const f = fakeFactory();
    const transport = createWebSocketTransport({ url: URL, createSocket: f.create, scheduler: new FakeScheduler() });
    const statuses: string[] = [];
    transport.onStatus((s) => statuses.push(s));
    transport.connect();
    f.sockets[0]!.emitOpen();
    f.sockets[0]!.emitError();
    expect(statuses).toEqual(["open", "closed"]);
  });

  it("sends a wire ping and closes when no pong arrives", () => {
    const f = fakeFactory();
    const scheduler = new FakeScheduler();
    const transport = createWebSocketTransport({ url: URL, createSocket: f.create, scheduler, pingMs: 30_000, pongTimeoutMs: 10_000 });
    const statuses: string[] = [];
    transport.onStatus((s) => statuses.push(s));
    transport.connect();
    f.sockets[0]!.emitOpen();
    expect(statuses).toEqual(["open"]);

    scheduler.advance(30_000);
    expect(f.sockets[0]!.sent.map((raw) => JSON.parse(raw))).toEqual([{ type: "ping" }]);

    // A pong inside the window keeps the socket open.
    f.sockets[0]!.emitMessage(JSON.stringify({ type: "pong" }));
    scheduler.advance(10_000);
    expect(statuses).toEqual(["open"]);
    expect(f.sockets[0]!.closed).toBe(false);

    // The next ping goes unanswered, so the client closes the socket itself.
    scheduler.advance(20_000);
    expect(f.sockets[0]!.sent.filter((raw) => JSON.parse(raw).type === "ping")).toHaveLength(2);
    scheduler.advance(10_000);
    expect(f.sockets[0]!.closed).toBe(true);
    expect(statuses).toEqual(["open", "closed"]);
  });
});
