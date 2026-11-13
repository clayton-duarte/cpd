import type { ConnectionState } from "./banner";

export interface OutboxEntry {
  id: string;
  kind: "submit" | "action" | "respond";
  payload: unknown;
  state: "queued" | "sending" | "failed";
  error?: string;
  createdAt: string;
}
export type SendResult = { ok: true } | { ok: false; error: string };
export interface OutboxStorage {
  load(): Promise<OutboxEntry[]>;
  put(entry: OutboxEntry): Promise<void>;
  remove(id: string): Promise<void>;
}
export interface OutboxDeps {
  storage: OutboxStorage;
  send: (entry: OutboxEntry) => Promise<SendResult>;
  now: () => string;
}
export interface Outbox {
  enqueue(
    entry: Omit<OutboxEntry, "state" | "createdAt"> & { createdAt?: string },
  ): Promise<void>;
  flush(): Promise<void>;
  onConnection(state: ConnectionState): Promise<void>;
  list(): OutboxEntry[];
  discard(id: string): Promise<void>;
  subscribe(fn: () => void): () => void;
}

export function createOutbox(_deps: OutboxDeps): Outbox {
  throw new Error("not implemented");
}
