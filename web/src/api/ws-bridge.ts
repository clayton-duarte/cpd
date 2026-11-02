// The dev-layer WebSocket bridge of the live adapter (web/README.md "The WebSocket stream",
// docs/research/web-socket-audit.md). STUB: the implementation Job fills every branch below;
// until then every entry point throws, so the red tests fail at runtime, never at typecheck.
//
// Every external boundary is injected: the token, the Session lookup, the durable host client
// (`web/src/api/host.ts`'s `HostClient`), the pi file read and the `fs.watch` boundary, and the
// `cpd session tail` fallback. No server, socket or real `WebSocket` lives here.
import type { SnapshotFrame } from "../fold/fold.ts";
import type { HostClient } from "./host.ts";

/** One Session the bridge resolves: the row's id, role and runner (web/README.md "Both runners"). */
export interface WsSession {
  id: string;
  role: "lead" | "manager";
  runner: "pi" | "durable";
  title?: string;
  state?: string;
  context?: string;
  name?: string;
}

/** `createWsBridge`'s injected boundaries; never a real socket, server or `WebSocket` in a test. */
export interface WsBridgeDeps {
  /** The configured token (`CPD_WEB_TOKEN`, default `dev-token`). */
  token(): string;
  /** Resolves a Session row by id, or `undefined` when there is none. */
  session(id: string): Promise<WsSession | undefined>;
  /** The durable read: the host-socket client of `web/src/api/host.ts`; `attach(id, onFrame)` streams host frames. */
  host: HostClient;
  /** A pi Session's session file as its full JSONL text, or `undefined` when there is none. */
  readPi(id: string): Promise<string | undefined>;
  /** Watches a pi Session's file (the de-polled `fs.watch` boundary); calls `onChange` on a change and returns a stop closure. */
  watchPi(id: string, onChange: () => void): () => void;
  /** The `cpd session tail` fallback when no live source answers. */
  tail(id: string): Promise<string>;
}

/** One opening WebSocket connection the bridge serves. */
export interface WsConnection {
  id: string;
  token: string;
  /** Writes one JSON text message: one `SnapshotFrame` per snapshot. */
  send(frame: SnapshotFrame): void;
}

/** The bridge: a per-connection handler returning the close closure that stops its reads. */
export interface WsBridge {
  handle(conn: WsConnection): () => void;
}

/** Builds the injected-dependency bridge. STUB until the implementation Job fills it. */
export function createWsBridge(_deps: WsBridgeDeps): WsBridge {
  throw new Error("not implemented");
}
