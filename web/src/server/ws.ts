// The production WebSocket server: one hub connection per accepted socket, JSON text
// frames over the existing wire union, the server-side protocol ping/pong heartbeat and the
// Origin check (web/CONTRACT.md §15.1, §15.2; docs/contract.md §5.5 "The WebUI transport and
// the serve-stream watchdog", ADR 0015).
//
// Everything the endpoint touches outside itself is injected: the hub factory
// (`newConnection`, web/src/server/plugin.ts:157), the `ws` server (a factory, so tests
// fake it and no test opens a port — the durable client's `SocketFactory` pattern,
// web/src/server/durable/client.ts:94) and the clock (`Scheduler`). The real `ws`
// dependency is pinned by the build Job, loaded here through `createRequire` because `ws`
// ships no typings.
import { createRequire } from "node:module";
import type { Disposable, Scheduler } from "../protocol/runtime.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import type { HubConnection } from "./hub/connection.ts";

/** The interval of the server's heartbeat protocol ping (web/CONTRACT.md §15.2). */
export const WS_PING_MS = 30_000;
/** How long after a ping the server waits for the pong before it terminates the socket. */
export const WS_PONG_TIMEOUT_MS = 10_000;
/** The path of the one WebSocket endpoint, both builds (web/CONTRACT.md §15.1). */
export const WS_PATH = "/cpd-ws";

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
  listen(options: WsListenOptions, onConnection: WsConnectionHandler, onListening?: (port: number) => void): WsServerLike;
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
  /** Called once with the bound port (loopback, bind port 0, web/CONTRACT.md §15.1). */
  onListening?: (port: number) => void;
  pingMs?: number;
  pongTimeoutMs?: number;
  host?: string;
  port?: number;
  path?: string;
}

/**
 * The Origin check (web/CONTRACT.md §15.1): absent and `null` (the production `file://`
 * renderer) are accepted, a loopback host (`localhost`, `127.0.0.1`, `[::1]`) is accepted,
 * every other Origin is rejected.
 */
export function acceptsOrigin(origin: string | string[] | undefined): boolean {
  if (origin === undefined) return true;
  if (Array.isArray(origin)) return origin.every((o) => acceptsOrigin(o));
  if (origin === "null") return true;
  let host: string;
  try {
    host = new URL(origin).hostname;
  } catch {
    return false;
  }
  return host === "localhost" || host === "127.0.0.1" || host === "::1" || host === "[::1]";
}

/**
 * Starts the endpoint: binds through the injected factory and, per accepted upgrade whose
 * Origin is absent, `null` or loopback, creates one hub connection, routes each JSON client
 * frame to `hub.handle`, writes each `ServerMessage` as one JSON text frame, pings every
 * `pingMs` and terminates a socket that does not pong within `pongTimeoutMs`. A foreign
 * Origin is terminated before any hub is created. `dispose` closes the server and every hub.
 */
export function createWsServer(deps: WsServerDeps): Disposable {
  const pingMs = deps.pingMs ?? WS_PING_MS;
  const pongTimeoutMs = deps.pongTimeoutMs ?? WS_PONG_TIMEOUT_MS;
  const open = new Set<{ dispose: () => void }>();
  let disposed = false;

  const onConnection = (socket: WsSocket): void => {
    let closed = false;
    let pongSeen = false;
    let pingTimer: Disposable | null = null;
    let pongTimer: Disposable | null = null;

    const clearTimers = (): void => {
      pingTimer?.dispose();
      pongTimer?.dispose();
      pingTimer = null;
      pongTimer = null;
    };
    const send = (message: ServerMessage): void => {
      if (!closed) socket.send(JSON.stringify(message));
    };
    const hub = deps.newConnection(send);

    const finish = (): void => {
      if (closed) return;
      closed = true;
      clearTimers();
      void hub.close();
    };

    const schedulePing = (): void => {
      pingTimer = deps.scheduler.after(pingMs, () => {
        pingTimer = null;
        pongSeen = false;
        socket.ping();
        pongTimer = deps.scheduler.after(pongTimeoutMs, () => {
          pongTimer = null;
          if (!pongSeen) socket.terminate();
        });
        schedulePing();
      });
    };

    socket.on("pong", () => {
      pongSeen = true;
    });
    socket.on("close", finish);
    socket.on("message", (data) => {
      if (closed) return;
      let raw: unknown;
      const text = typeof data === "string" ? data : String(data);
      try {
        raw = JSON.parse(text);
      } catch {
        raw = text;
      }
      void deps.started
        .then(() => hub.handle(raw))
        .catch((error: unknown) => deps.onError?.(error));
    });

    const entry = { dispose: () => { finish(); } };
    open.add(entry);
    schedulePing();
  };

  const server = deps.server.listen(
    { host: deps.host ?? "127.0.0.1", port: deps.port ?? 0, path: deps.path ?? WS_PATH },
    (socket, request) => {
      if (disposed) {
        socket.terminate();
        return;
      }
      if (!acceptsOrigin(request.headers.origin)) {
        socket.terminate();
        return;
      }
      onConnection(socket);
    },
    (port) => deps.onListening?.(port),
  );

  return {
    dispose: () => {
      if (disposed) return;
      disposed = true;
      server.close();
      for (const entry of [...open]) entry.dispose();
      open.clear();
    },
  };
}

// ---- the real `ws` factories: the only place the endpoint touches the package ----

/** The slice of `ws.WebSocketServer` the endpoint uses; `ws` ships no typings, so it is declared here. */
interface WsServerInstance {
  on(event: "connection", listener: (socket: WsSocket, request: WsRequest) => void): void;
  on(event: "error", listener: (error: Error) => void): void;
  on(event: "listening", listener: () => void): void;
  address(): unknown;
  handleUpgrade(request: unknown, socket: unknown, head: unknown, callback: (socket: WsSocket) => void): void;
  emit(event: "connection", socket: WsSocket, request: WsRequest): void;
  close(callback?: (error?: Error) => void): void;
}
interface WsServerConstructor {
  new (options?: { host?: string; port?: number; path?: string; noServer?: boolean }): WsServerInstance;
}

function loadWebSocketServer(): WsServerConstructor {
  const require = createRequire(import.meta.url);
  return (require("ws") as { WebSocketServer: WsServerConstructor }).WebSocketServer;
}

/** The real `ws` factory for a standalone loopback listener (the Electron main process, web/CONTRACT.md §15.1). */
export function createListenerFactory(): WsServerFactory {
  const WebSocketServer = loadWebSocketServer();
  return {
    listen(options, onConnection, onListening) {
      const server = new WebSocketServer({ host: options.host, port: options.port, path: options.path });
      server.on("connection", (socket, request) => onConnection(socket, request));
      server.on("error", () => undefined);
      server.on("listening", () => {
        const address = server.address();
        const port = typeof address === "object" && address !== null && "port" in address ? (address as { port: number }).port : options.port;
        onListening?.(port);
      });
      return { close: () => server.close() };
    },
  };
}

/** The http server the Vite dev server exposes: the slice the noServer upgrade needs. */
export interface WebSocketUpgradeServer {
  on(event: "upgrade", listener: (request: unknown, socket: unknown, head: unknown) => void): unknown;
  off?(event: "upgrade", listener: (request: unknown, socket: unknown, head: unknown) => void): unknown;
}

/**
 * The real `ws` factory attached to an existing http server with `noServer` plus an
 * `upgrade` listener on `options.path`, so Vite's own HMR socket is untouched
 * (web/CONTRACT.md §15.1).
 */
export function createUpgradeFactory(server: WebSocketUpgradeServer): WsServerFactory {
  const WebSocketServer = loadWebSocketServer();
  return {
    listen(options, onConnection, onListening) {
      const wss = new WebSocketServer({ noServer: true });
      const onUpgrade = (request: unknown, socket: unknown, head: unknown): void => {
        const url = (request as { url?: string }).url ?? "";
        const path = url.split("?")[0] ?? "";
        if (path !== options.path) return;
        wss.handleUpgrade(request, socket, head, (ws) => {
          wss.emit("connection", ws, request as WsRequest);
        });
      };
      server.on("upgrade", onUpgrade);
      onListening?.(options.port);
      return {
        close: () => {
          server.off?.("upgrade", onUpgrade);
          wss.close();
        },
      };
    },
  };
}
