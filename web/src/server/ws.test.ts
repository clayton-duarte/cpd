import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../protocol/wire.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import type { HubConnection } from "./hub/connection.ts";
import { createWsServer, type NewConnection, type WsServerDeps, type WsServerFactory, type WsServerLike, type WsSocket } from "./ws.ts";

/** A hand-rolled `ws` socket: records what the endpoint sends, drives its handlers. */
class FakeSocket implements WsSocket {
  sent: string[] = [];
  pings = 0;
  terminated = false;
  private readonly messageHandlers: Array<(data: unknown) => void> = [];
  private readonly pongHandlers: Array<() => void> = [];
  private readonly closeHandlers: Array<() => void> = [];

  send(data: string): void {
    this.sent.push(data);
  }
  ping(): void {
    this.pings++;
  }
  terminate(): void {
    this.terminated = true;
    this.emitClose();
  }
  on(event: "message", listener: (data: unknown) => void): void;
  on(event: "pong", listener: () => void): void;
  on(event: "close", listener: () => void): void;
  on(event: "message" | "pong" | "close", listener: ((data: unknown) => void) | (() => void)): void {
    if (event === "message") this.messageHandlers.push(listener as (data: unknown) => void);
    else if (event === "pong") this.pongHandlers.push(listener as () => void);
    else this.closeHandlers.push(listener as () => void);
  }

  emitMessage(data: unknown): void {
    for (const h of [...this.messageHandlers]) h(data);
  }
  emitPong(): void {
    for (const h of [...this.pongHandlers]) h();
  }
  emitClose(): void {
    for (const h of [...this.closeHandlers]) h();
  }
}

/** Captures the endpoint's connection callback; no port is ever opened. */
function fakeServer() {
  let connected: ((socket: WsSocket) => void) | null = null;
  let closes = 0;
  const factory: WsServerFactory = {
    listen: (_options, onConnection) => {
      connected = onConnection;
      const server: WsServerLike = { close: () => { closes++; } };
      return server;
    },
  };
  return {
    factory,
    connect: (socket: WsSocket): void => {
      if (connected === null) throw new Error("listen was not called");
      connected(socket);
    },
    closes: () => closes,
  };
}

type HubRecord = { handled: unknown[]; closes: number; send: (m: ServerMessage) => void };

const reply = { type: "cockpit.error", error: { code: "internal", message: "boom" } } as unknown as ServerMessage;

function harness() {
  const scheduler = new FakeScheduler();
  const server = fakeServer();
  const hubs: HubRecord[] = [];
  const errors: unknown[] = [];
  const newConnection: NewConnection = (send) => {
    const rec: HubRecord = { handled: [], closes: 0, send };
    hubs.push(rec);
    const hub: HubConnection = {
      handle: (raw) => {
        rec.handled.push(raw);
        if ((raw as { type?: unknown }).type === "cockpit.subscribe") send(reply);
        return Promise.resolve();
      },
      close: () => {
        rec.closes++;
        return Promise.resolve();
      },
    };
    return hub;
  };
  const deps: WsServerDeps = { newConnection, server: server.factory, scheduler, started: Promise.resolve(), pingMs: 30_000, pongTimeoutMs: 10_000 };
  return { deps, server, hubs, errors, scheduler, onError: (e: unknown) => errors.push(e) };
}

const flush = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

describe("ws server", () => {
  it("routes a client message through the connection and returns a server message", async () => {
    const h = harness();
    createWsServer(h.deps);
    const socket = new FakeSocket();
    h.server.connect(socket);
    expect(h.hubs).toHaveLength(1);

    socket.emitMessage(JSON.stringify({ type: "cockpit.subscribe" }));
    await flush();
    expect(h.hubs[0]!.handled).toEqual([{ type: "cockpit.subscribe" }]);
    expect(socket.sent).toEqual([JSON.stringify(reply)]);
  });

  it("closes a peer that stops answering the heartbeat", () => {
    const h = harness();
    createWsServer(h.deps);
    const socket = new FakeSocket();
    h.server.connect(socket);

    h.scheduler.advance(30_000);
    expect(socket.pings).toBe(1);
    expect(socket.terminated).toBe(false);

    h.scheduler.advance(10_000);
    expect(socket.terminated).toBe(true);
    expect(h.hubs[0]!.closes).toBe(1);

    socket.emitPong();
    h.scheduler.advance(1_000_000);
    expect(h.hubs[0]!.closes).toBe(1);
  });
});
