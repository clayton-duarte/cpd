// The pi live-stream socket of a tmux pi Session (docs/contract.md §6.21). Stub: not implemented yet.
import type { Duplex } from "node:stream";

export interface LiveStreamDeps {
  /** Listens at `path`; throws when it cannot. `onConnection` gets each client as a duplex stream. */
  listen(path: string, onConnection: (connection: Duplex) => void): { close(): void };
  mkdir(path: string, mode: number): void;
  unlink(path: string): void;
  home: string;
  session: string;
  stderr(text: string): void;
}

export interface LiveStreamPi {
  on(event: string, handler: (event: any, ctx?: unknown) => unknown): unknown;
}

export interface LiveStream {
  attach(pi: LiveStreamPi): void;
  dispose(): void;
}

export function createLiveStream(_deps: LiveStreamDeps): LiveStream {
  throw new Error("not implemented");
}
