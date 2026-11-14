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
  canSend?: (entry: OutboxEntry) => boolean;
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
  let persistent = true;
  // The entries map remains usable when browser storage is denied, full, or later unavailable.
  const persist = async (write: () => Promise<void>): Promise<void> => {
    if (!persistent) return;
    try { await write(); } catch { persistent = false; }
  };
  const put = (entry: OutboxEntry) => persist(() => deps.storage.put(entry));
  const remove = (id: string) => persist(() => deps.storage.remove(id));
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
  const ready = (() => {
    try {
      const initial = deps.storage.load();
      if (Array.isArray(initial)) { merge(initial); return Promise.resolve(); }
      return initial.then(merge, () => { persistent = false; merge([]); });
    } catch { persistent = false; merge([]); return Promise.resolve(); }
  })();

  const sorted = (): OutboxEntry[] =>
    [...entries.values()].sort((a, b) => (a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : 0));

  /** Sends one entry and settles it on its answer. A send that threw (the connection dropped) puts it back to queued. */
  async function sendOne(entry: OutboxEntry): Promise<void> {
    entry.state = "sending";
    delete entry.error;
    notify();
    const stored = put({ ...entry });
    let result: SendResult;
    try {
      result = await deps.send({ ...entry });
    } catch {
      await stored;
      if (entries.has(entry.id)) {
        entry.state = "queued";
        notify();
        await put({ ...entry });
      }
      return;
    }
    await stored;
    if (!entries.has(entry.id)) return; // discarded while sending
    if (result.ok) {
      entries.delete(entry.id);
      notify();
      await remove(entry.id);
    } else {
      entry.state = "failed";
      entry.error = result.error;
      notify();
      await put({ ...entry });
    }
  }

  // Sends every queued entry in order without waiting for the answers (a send must not block the next),
  // then waits for them all. An entry whose send threw is not retried within the same drain.
  async function drain(): Promise<void> {
    active = true;
    const settled: Promise<void>[] = [];
    const attempted = new Set<string>();
    try {
      if (!loaded) await ready;
      while (connection === "open") {
        const next = sorted().find((e) => e.state === "queued" && !attempted.has(e.id) && (deps.canSend?.(e) ?? true));
        if (next === undefined) break;
        attempted.add(next.id);
        settled.push(sendOne(next));
      }
    } finally {
      active = false; // cleared synchronously, so an enqueue right after the last scan starts a new drain
    }
    await Promise.all(settled);
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
      const stored = put({ ...entry });
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
      await remove(id);
    },
    async retry(id) {
      const entry = entries.get(id);
      if (entry === undefined || entry.state !== "failed") return;
      entry.state = "queued";
      delete entry.error;
      notify();
      await put({ ...entry });
      await flush();
    },
    subscribe(fn) {
      subscribers.add(fn);
      return () => subscribers.delete(fn);
    },
  };
}

/** Browser/runtime pieces an id needs; injected so tests use no real crypto or time. */
export interface IdEnv {
  crypto?: { randomUUID?: () => string; getRandomValues?: (a: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer> } | undefined;
  now: () => number;
  random: () => number;
}
/** Request ids: crypto.randomUUID is secure-context only, and the WebUI is also opened over plain-HTTP LAN origins. */
export function createIdGenerator(env: IdEnv): () => string {
  let counter = 0;
  return () => {
    const c = env.crypto;
    if (typeof c?.randomUUID === "function") return c.randomUUID();
    if (typeof c?.getRandomValues === "function") {
      const b = c.getRandomValues(new Uint8Array(16));
      b[6] = (b[6]! & 0x0f) | 0x40;
      b[8] = (b[8]! & 0x3f) | 0x80;
      const h = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
      return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
    }
    return `${env.now().toString(36)}-${(++counter).toString(36)}-${Math.floor(env.random() * 2 ** 32).toString(36)}`;
  };
}
