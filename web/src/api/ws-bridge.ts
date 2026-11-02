// The dev-layer WebSocket bridge of the live adapter (web/README.md "The WebSocket stream",
// docs/research/web-socket-audit.md). Every external boundary is injected: the token, the Session
// lookup, the durable host client (`web/src/api/host.ts`'s `HostClient`), the pi file read and the
// `fs.watch` boundary, and the `cpd session tail` fallback. No server, socket or real `WebSocket`
// lives here.
//
// One `handle({id, token, send})` call serves one browser connection: the token is checked before
// any read, the Session is resolved, and the same `SnapshotFrame`s the SSE route sends are streamed
// — a durable Session over its host socket (`web/src/translate.ts` folds the host frames onto the
// CLI path's `TranscriptMessage[]`), a pi Session from its file plus an `fs.watch` (de-polled, no
// 1 s timer). The returned closure stops whichever read the Session opened.
import type { SnapshotFrame } from "../fold/fold.ts";
import type { HostFrame, HostClient } from "./host.ts";
import { applyHostFrame, type HostState } from "../translate.ts";
import { parsePiSession, parseTailOutput, toSnapshotFrame, type TranscriptMessage } from "../transcript.ts";
import { snapshotTail } from "./live-server.ts";

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

/**
 * A message as the wire carries it: `parts` is absent when there is nothing but its `text` (the
 * `TranscriptMessage` contract), so a plain user/assistant line is `{id, role, text}` exactly.
 */
function wireMessage(message: TranscriptMessage): TranscriptMessage {
  const parts = message.parts;
  if (parts === undefined || parts.length !== 1) return message;
  const part = parts[0]!;
  if (part.kind === "text" && part.text === message.text) {
    const { parts: _parts, ...rest } = message;
    return rest;
  }
  return message;
}

/** The messages of a snapshot frame, each as the wire carries it. */
function wireMessages(messages: readonly TranscriptMessage[]): TranscriptMessage[] {
  return messages.map(wireMessage);
}

/** Streams a pi Session's file: the first snapshot at once, then a snapshot on each watch change. */
function streamPi(
  conn: WsConnection,
  session: WsSession,
  deps: WsBridgeDeps,
  isClosed: () => boolean,
  stop: (fn: () => void) => void,
): void {
  let seq = 0;
  const emit = (jsonl: string | undefined): void => {
    if (isClosed()) return;
    seq += 1;
    const messages = jsonl === undefined ? [] : wireMessages(parsePiSession(jsonl));
    conn.send(toSnapshotFrame(snapshotTail(messages), "live", seq));
  };

  void (async (): Promise<void> => {
    emit(await deps.readPi(session.id));
    if (isClosed()) return;
    const watcher = deps.watchPi(session.id, () => {
      void (async (): Promise<void> => {
        emit(await deps.readPi(session.id));
      })();
    });
    stop(watcher);
  })();
}

/** Streams a durable Session from its host socket; a host that ends the stream falls back to `cpd session tail`. */
function streamDurable(
  conn: WsConnection,
  session: WsSession,
  deps: WsBridgeDeps,
  isClosed: () => boolean,
  stop: (fn: () => void) => void,
): void {
  let state: HostState = { messages: [], cursor: undefined };
  let seq = 0;
  let sent: string | undefined;
  let fellBack = false;

  const emit = (messages: readonly TranscriptMessage[]): void => {
    if (isClosed()) return;
    const key = JSON.stringify(messages);
    if (key === sent) return;
    sent = key;
    seq += 1;
    conn.send(toSnapshotFrame(snapshotTail(messages), "live", seq));
  };

  const apply = (frame: HostFrame): void => {
    if (frame.type === "snapshot" || frame.type === "events") {
      state = applyHostFrame(state, frame);
      emit(wireMessages(state.messages));
      return;
    }
    // The host refused to keep the stream (`bye` with no reconnect): the live source is gone, so
    // the fallback prints the transcript exactly as the SSE route does.
    if (!fellBack && frame.type === "bye" && frame.reconnect === false) {
      fellBack = true;
      void (async (): Promise<void> => {
        const output = await deps.tail(session.id);
        if (isClosed()) return;
        state = { messages: wireMessages(parseTailOutput(output)), cursor: undefined };
        emit(state.messages);
      })();
    }
  };

  stop(deps.host.attach(session.id, apply));
}

/** Builds the injected-dependency bridge. */
export function createWsBridge(deps: WsBridgeDeps): WsBridge {
  return {
    handle(conn: WsConnection): () => void {
      const stops: Array<() => void> = [];
      let closed = false;
      const isClosed = (): boolean => closed;
      const close = (): void => {
        if (closed) return;
        closed = true;
        for (const stop of stops) stop();
        stops.length = 0;
      };
      const stop = (fn: () => void): void => {
        if (closed) fn();
        else stops.push(fn);
      };

      void (async (): Promise<void> => {
        // The token is checked BEFORE any read or send.
        if (conn.token !== deps.token()) return;
        const session = await deps.session(conn.id);
        if (session === undefined || closed) return;
        if (session.runner === "durable") streamDurable(conn, session, deps, isClosed, stop);
        else streamPi(conn, session, deps, isClosed, stop);
      })();

      return close;
    },
  };
}
