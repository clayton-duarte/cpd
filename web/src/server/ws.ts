// The production WebSocket server: one hub connection per accepted socket, JSON text
// frames over the existing wire union, the server-side protocol ping/pong heartbeat and the
// Origin check (web/CONTRACT.md §15.1, §15.2; docs/contract.md §5.5 "The WebUI transport and
// the serve-stream watchdog", ADR 0015).
//
// Everything the endpoint touches outside itself is injected: the hub factory
// (`newConnection`, web/src/server/plugin.ts:157), the `ws` server (a factory, so tests
// fake it and no test opens a port — the durable client's `SocketFactory` pattern,
// web/src/server/durable/client.ts:94) and the clock (`Scheduler`). The real `ws`
// dependency is pinned by the build Job, not here.
import type { Disposable, Scheduler } from "../protocol/runtime.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import type { HubConnection } from "./hub/connection.ts";

/** The interval of the server's heartbeat protocol ping (web/CONTRACT.md §15.2). */
export const WS_PING_MS = 30_000;
/** How long after a ping the server waits for the pong before it terminates the socket. */
export const WS_PONG_TIMEOUT_MS = 10_000;

/** The slice of a `ws` WebSocket the endpoint uses; the real socket satisfies it. */
export interface WsSocket {
  send(data: string): void;
  ping(): void;
  terminate(): void;
  on(event: "message", listener: (data: unknown) => void): void;
  on(event: "pong", listener: () => void): void;
  on(event: "close", listener: () => void): void;
}

/** The slice of the upgrade request the Origin check reads (the `ws` connection's `req`). */
export interface WsRequest {
  readonly headers: { readonly origin?: string | string[] | undefined };
}

/** Where the endpoint listens (loopback in production, web/CONTRACT.md §15.1). */
export interface WsListenOptions {
  host: string;
  port: number;
  path: string;
}

/** A listening server; the real one wraps `ws.WebSocketServer`, tests fake it. */
export interface WsServerLike {
  close(): void;
}

/** Called once per accepted upgrade, with the request whose Origin the endpoint checks. */
export type WsConnectionHandler = (socket: WsSocket, request: WsRequest) => void;

/** The injected server factory (the `SocketFactory` pattern, web/src/server/durable/client.ts:94). */
export interface WsServerFactory {
  listen(options: WsListenOptions, onConnection: WsConnectionHandler): WsServerLike;
}

/** The factory that builds one hub connection per socket (web/src/server/plugin.ts:157). */
export type NewConnection = (send: (message: ServerMessage) => void) => HubConnection;

export interface WsServerDeps {
  newConnection: NewConnection;
  server: WsServerFactory;
  scheduler: Scheduler;
  /** The cockpit's start; a frame waits on it before it reaches its hub (web/ELECTRON.md). */
  started: Promise<void>;
  onError?: (error: unknown) => void;
  pingMs?: number;
  pongTimeoutMs?: number;
  host?: string;
  port?: number;
  path?: string;
}

/**
 * Starts the endpoint: binds through the injected factory and, per accepted upgrade whose
 * Origin is absent, `null` or loopback, creates one hub connection, routes each JSON client
 * frame to `hub.handle`, writes each `ServerMessage` as one JSON text frame, pings every
 * `pingMs` and terminates a socket that does not pong within `pongTimeoutMs`. A foreign
 * Origin is terminated before any hub is created. `dispose` closes the server and every hub.
 */
export function createWsServer(_deps: WsServerDeps): Disposable {
  throw new Error("createWsServer is not implemented");
}
