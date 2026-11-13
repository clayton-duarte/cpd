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
  /** An array is taken synchronously (the in-memory store), so a send right after creation is not delayed. */
  load(): Promise<OutboxEntry[]> | OutboxEntry[];
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
  retry(id: string): Promise<void>;
  subscribe(fn: () => void): () => void;
}

export function createOutbox(deps: OutboxDeps): Outbox {
  const entries = new Map<string, OutboxEntry>();
  const subscribers = new Set<() => void>();
  let connection: ConnectionState = "connecting";
  let loaded = false;
  let active = false;
  let running: Promise<void> = Promise.resolve();
  let snapshot: OutboxEntry[] | null = null;

  const notify = (): void => {
    snapshot = null;
    for (const fn of subscribers) fn();
  };
  const merge = (rows: OutboxEntry[]): void => {
    for (const row of rows) {
      // An entry that was sending when the page went away is resent: same id, the server dedups.
      if (!entries.has(row.id)) entries.set(row.id, { ...row, state: row.state === "sending" ? "queued" : row.state });
    }
    loaded = true;
    notify();
  };
  const initial = deps.storage.load();
  const ready: Promise<void> = Array.isArray(initial) ? Promise.resolve() : initial.then(merge);
  if (Array.isArray(initial)) merge(initial);

  const sorted = (): OutboxEntry[] =>
    [...entries.values()].sort((a, b) => (a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0));

  /** Sends one entry; false when the send itself threw (the connection dropped), so the drain stops. */
  async function sendOne(entry: OutboxEntry): Promise<boolean> {
    entry.state = "sending";
    delete entry.error;
    notify();
    const stored = deps.storage.put({ ...entry });
    let result: SendResult;
    try {
      result = await deps.send({ ...entry });
    } catch {
      if (entries.has(entry.id)) {
        entry.state = "queued";
        notify();
        await deps.storage.put({ ...entry });
      }
      await stored;
      return false;
    }
    await stored;
    if (!entries.has(entry.id)) return true; // discarded while sending
    if (result.ok) {
      entries.delete(entry.id);
      notify();
      await deps.storage.remove(entry.id);
    } else {
      entry.state = "failed";
      entry.error = result.error;
      notify();
      await deps.storage.put({ ...entry });
    }
    return true;
  }

  async function drain(): Promise<void> {
    active = true;
    try {
      if (!loaded) await ready;
      for (;;) {
        if (connection !== "open") return;
        const next = sorted().find((e) => e.state === "queued");
        if (next === undefined) return;
        if (!(await sendOne(next))) return;
      }
    } finally {
      active = false; // cleared synchronously, so an enqueue right after the last scan starts a new drain
    }
  }
  const flush = (): Promise<void> => {
    if (active) return running;
    const p = drain();
    if (active) running = p;
    return p;
  };

  return {
    enqueue(input) {
      const entry: OutboxEntry = { ...input, state: "queued", createdAt: input.createdAt ?? deps.now() };
      entries.set(entry.id, entry);
      notify();
      const stored = deps.storage.put({ ...entry });
      return Promise.all([stored, flush()]).then(() => undefined);
    },
    flush,
    onConnection(state) {
      connection = state;
      return state === "open" ? flush() : Promise.resolve();
    },
    list() {
      snapshot ??= sorted().map((e) => ({ ...e }));
      return snapshot;
    },
    async discard(id) {
      entries.delete(id);
      notify();
      await deps.storage.remove(id);
    },
    async retry(id) {
      const entry = entries.get(id);
      if (entry === undefined || entry.state !== "failed") return;
      entry.state = "queued";
      delete entry.error;
      notify();
      await deps.storage.put({ ...entry });
      await flush();
    },
    subscribe(fn) {
      subscribers.add(fn);
      return () => subscribers.delete(fn);
    },
  };
}
