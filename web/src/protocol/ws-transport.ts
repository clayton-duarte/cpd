// The one client `Transport` over the browser WebSocket API, for the Electron desktop and
// the browser build alike (web/CONTRACT.md §15.1 "The client transport", ADR 0015 option B).
// It carries no reconnect logic: the renderer's existing backoff owns that
// (web/src/state/connection.ts:205-224).
//
// The socket is injected so tests fake it and no test opens a real socket or port
// (owner tests policy; the replaced web/src/electron/transport.test.ts pattern). The clock
// is injected too, so the wire heartbeat runs on the fake scheduler in tests.
import type { Scheduler, Transport } from "./runtime.ts";

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
export function createWebSocketTransport(_deps: WebSocketTransportDeps): Transport {
  throw new Error("createWebSocketTransport is not implemented");
}
