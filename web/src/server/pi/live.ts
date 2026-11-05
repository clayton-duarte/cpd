import type { Scheduler } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";

export const LIVE_RETRY_MS = 1000;

export interface LiveConnection {
  onLine(cb: (line: string) => void): void;
  onClose(cb: () => void): void;
  close(): void;
}

export interface LiveSource {
  /** `committed(ts)` tells whether an assistant `message.timestamp` is already in the transcript. */
  attach(sessionId: string, emit: (msg: SessionServerMessage) => void, committed: (timestamp: number) => boolean): void;
  detach(): void;
}

export interface LiveDeps {
  /** Throws when no socket listens. */
  connect(path: string): LiveConnection;
  scheduler: Scheduler;
  cpdHome: string;
}

export function createLiveSource(_deps: LiveDeps): LiveSource {
  return {
    attach() {
      throw new Error("not implemented");
    },
    detach() {
      throw new Error("not implemented");
    },
  };
}
