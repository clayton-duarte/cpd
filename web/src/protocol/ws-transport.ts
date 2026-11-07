// The one client `Transport` over the browser WebSocket API, for the Electron desktop and
// the browser build alike (docs/contract.md §5.5 "The client transport", ADR 0015 §Option B).
// It carries no reconnect logic: the renderer's existing backoff owns that
// (web/src/state/connection.ts:205-224).
//
// The socket is injected so tests fake it and no test opens a real socket or port
// (owner tests policy; the replaced web/src/electron/transport.test.ts pattern).
import type { Disposable, Transport } from "./runtime.ts";

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

const defaultFactory: WebSocketFactory = (url) => new WebSocket(url) as unknown as WebSocketLike;

/**
 * The `Transport` semantics are unchanged (web/src/protocol/runtime.ts:10-15): `connect`
 * begins one attempt and `"open"`/`"closed"` arrive through `onStatus`; `send` throws
 * `WebError("internal")` when not open; `close` is final. A `message` frame is parsed and
 * delivered to `onMessage`; the socket's `close` and `error` both surface as `"closed"`,
 * so a stalled peer is seen loud, never silent.
 */
export function createWebSocketTransport(_url: string, _createSocket: WebSocketFactory = defaultFactory): Transport {
  throw new Error("createWebSocketTransport is not implemented");
}
