import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../protocol/wire.ts";
import { FakeScheduler } from "../testing/scheduler.ts";
import type { HubConnection } from "./hub/connection.ts";
import {
  createWsServer,
  type NewConnection,
  type WsRequest,
  type WsServerDeps,
  type WsServerFactory,
  type WsServerLike,
  type WsSocket,
} from "./ws.ts";

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

const NO_ORIGIN: WsRequest = { headers: {} };
const request = (origin: string): WsRequest => ({ headers: { origin } });

/** Captures the endpoint's connection callback; no port is ever opened. */
function fakeServer() {
  let connected: ((socket: WsSocket, request: WsRequest) => void) | null = null;
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
    connect: (socket: WsSocket, req: WsRequest = NO_ORIGIN): void => {
      if (connected === null) throw new Error("listen was not called");
      connected(socket, req);
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
  return { deps, server, hubs, scheduler };
}

const flush = async (): Promise<void> => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

describe("ws server", () => {
  it("accepts a loopback connection and rejects a foreign origin", () => {
    const h = harness();
    createWsServer(h.deps);

    // An absent Origin, the desktop's "null" (file://) and every loopback host are accepted.
    const accepted = [NO_ORIGIN, request("null"), request("http://127.0.0.1:5173"), request("http://localhost:5173"), request("http://[::1]:5173")];
    for (const req of accepted) {
      const socket = new FakeSocket();
      h.server.connect(socket, req);
      expect(socket.terminated).toBe(false);
    }
    expect(h.hubs).toHaveLength(accepted.length);

    // A foreign Origin is terminated before any hub is created.
    for (const origin of ["http://evil.example", "https://evil.example:8443"]) {
      const socket = new FakeSocket();
      h.server.connect(socket, request(origin));
      expect(socket.terminated).toBe(true);
    }
    expect(h.hubs).toHaveLength(accepted.length);
  });

  it("routes a client message through one hub connection", async () => {
    const h = harness();
    createWsServer(h.deps);
    const socket = new FakeSocket();
    h.server.connect(socket);
    expect(h.hubs).toHaveLength(1);

    socket.emitMessage(JSON.stringify({ type: "cockpit.subscribe" }));
    await flush();
    expect(h.hubs[0]!.handled).toEqual([{ type: "cockpit.subscribe" }]);
    expect(socket.sent).toEqual([JSON.stringify(reply)]);

    // A second socket gets its own hub connection; the first is not reused.
    const second = new FakeSocket();
    h.server.connect(second);
    expect(h.hubs).toHaveLength(2);
    expect(h.hubs[0]!.handled).toEqual([{ type: "cockpit.subscribe" }]);
  });

  it("terminates a socket that misses the pong deadline", () => {
    const h = harness();
    createWsServer(h.deps);

    const silent = new FakeSocket();
    h.server.connect(silent);
    h.scheduler.advance(30_000);
    expect(silent.pings).toBe(1);
    expect(silent.terminated).toBe(false);

    h.scheduler.advance(10_000);
    expect(silent.terminated).toBe(true);
    expect(h.hubs[0]!.closes).toBe(1);

    // A socket that answers the pong inside the window survives its deadline.
    const answered = new FakeSocket();
    h.server.connect(answered);
    h.scheduler.advance(30_000);
    expect(answered.pings).toBe(1);
    answered.emitPong();
    h.scheduler.advance(10_000);
    expect(answered.terminated).toBe(false);
    expect(h.hubs[1]!.closes).toBe(0);
  });
});
