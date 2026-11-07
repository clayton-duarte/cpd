// The one client `Transport` over the browser WebSocket API, for the Electron desktop and
// the browser build alike (web/CONTRACT.md §15.1 "The client transport", ADR 0015 option B).
// It carries no reconnect logic: the renderer's existing backoff owns that
// (web/src/state/connection.ts:205-224).
//
// The socket is injected so tests fake it and no test opens a real socket or port
// (owner tests policy; the replaced web/src/electron/transport.ts pattern). The clock
// is injected too, so the wire heartbeat runs on the fake scheduler in tests.
import { WebError } from "./domain.ts";
import type { Disposable, Scheduler, Transport } from "./runtime.ts";

/** The interval of the client's wire `ping` (web/CONTRACT.md §15.2). */
export const WS_CLIENT_PING_MS = 30_000;
/** How long after a wire `ping` the client waits for the `pong` before it closes the socket. */
export const WS_CLIENT_PONG_TIMEOUT_MS = 10_000;

/** The slice of the browser WebSocket the transport uses; the real socket satisfies it. */
export interface WebSocketLike {
  readonly readyState: number;
  send(data: string): void;
  close(): void;
  addEventListener(type: "open", listener: () => void): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  addEventListener(type: "close", listener: () => void): void;
  addEventListener(type: "error", listener: () => void): void;
}

/** Opens one socket for a URL; injected so tests fake it. */
export type WebSocketFactory = (url: string) => WebSocketLike;

export interface WebSocketTransportDeps {
  /** The endpoint to connect to (`ws://127.0.0.1:<port>/cpd-ws` or same-origin `/cpd-ws`). */
  url: string;
  createSocket: WebSocketFactory;
  scheduler: Scheduler;
  pingMs?: number;
  pongTimeoutMs?: number;
}

/**
 * The `Transport` semantics are unchanged (web/src/protocol/runtime.ts): `connect` begins one
 * attempt and `"open"`/`"closed"` arrive through `onStatus`; `send` throws `WebError("internal")`
 * when not open; `close` is final. A `message` frame is parsed and delivered to `onMessage`;
 * the socket's `close` and `error` both surface as `"closed"`, so a stalled peer is seen loud,
 * never silent. The client's wire `ping` is sent every `pingMs` and a `pong` is required within
 * `pongTimeoutMs`; a missing `pong` closes the socket from the client side.
 */
export function createWebSocketTransport(deps: WebSocketTransportDeps): Transport {
  const pingMs = deps.pingMs ?? WS_CLIENT_PING_MS;
  const pongTimeoutMs = deps.pongTimeoutMs ?? WS_CLIENT_PONG_TIMEOUT_MS;
  let disposed = false; // `close()` was called: final, `connect()` is a no-op
  let open = false;
  let socket: WebSocketLike | null = null;
  let pongSeen = false;
  let pingTimer: Disposable | null = null;
  let pongTimer: Disposable | null = null;
  const messageHandlers = new Set<(raw: unknown) => void>();
  const statusHandlers = new Set<(status: "open" | "closed") => void>();
  const subscribe = <T,>(set: Set<T>, handler: T): Disposable => {
    set.add(handler);
    return { dispose: () => { set.delete(handler); } };
  };
  const clearTimers = (): void => {
    pingTimer?.dispose();
    pongTimer?.dispose();
    pingTimer = null;
    pongTimer = null;
  };
  const emitStatus = (status: "open" | "closed"): void => {
    for (const handler of [...statusHandlers]) handler(status);
  };

  /** A socket that closed, errored or missed its pong: a real `closed` status, reconnectable. */
  const closeCurrent = (current: WebSocketLike): void => {
    if (disposed || socket !== current) return;
    open = false;
    socket = null;
    clearTimers();
    current.close();
    emitStatus("closed");
  };

  const schedulePing = (current: WebSocketLike): void => {
    pingTimer = deps.scheduler.after(pingMs, () => {
      pingTimer = null;
      if (disposed || socket !== current || !open) return;
      pongSeen = false;
      current.send(JSON.stringify({ type: "ping" }));
      pongTimer = deps.scheduler.after(pongTimeoutMs, () => {
        pongTimer = null;
        if (disposed || socket !== current || !open) return;
        if (!pongSeen) closeCurrent(current);
      });
      schedulePing(current);
    });
  };

  const openSocket = (current: WebSocketLike): void => {
    current.addEventListener("open", () => {
      if (disposed || socket !== current) return;
      open = true;
      emitStatus("open");
      schedulePing(current);
    });
    current.addEventListener("message", (event) => {
      if (disposed || socket !== current || !open) return;
      let raw: unknown;
      try {
        raw = JSON.parse(typeof event.data === "string" ? event.data : String(event.data));
      } catch {
        return;
      }
      if (typeof raw === "object" && raw !== null && (raw as { type?: unknown }).type === "pong") {
        pongSeen = true;
        return;
      }
      for (const handler of [...messageHandlers]) handler(raw);
    });
    current.addEventListener("close", () => closeCurrent(current));
    current.addEventListener("error", () => closeCurrent(current));
  };

  return {
    connect() {
      if (disposed || socket !== null) return;
      const current = deps.createSocket(deps.url);
      socket = current;
      openSocket(current);
    },
    send(raw) {
      if (disposed || !open || socket === null) throw new WebError("internal", "transport is not open");
      socket.send(JSON.stringify(raw));
    },
    close() {
      if (disposed) return;
      disposed = true;
      open = false;
      clearTimers();
      const current = socket;
      socket = null;
      current?.close();
    },
    onMessage: (handler) => subscribe(messageHandlers, handler),
    onStatus: (handler) => subscribe(statusHandlers, handler),
  };
}
