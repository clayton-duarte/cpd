import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { WebError } from "../../protocol/domain.ts";

export interface DedupStorage {
  read(): string | undefined;
  write(s: string): void;
}

export interface DedupStore {
  /** Drops a key so a later run executes again (a delivery that failed must stay retryable). */
  forget(key: string): void;
  run<T>(key: string, exec: () => Promise<T> | T): Promise<T>;
}

type Entry =
  | { ok: true; value: unknown }
  | { ok: false; error: { web: true; code: string; message: string; details?: Record<string, unknown> } | { web: false; message: string } };

/** One JSON file, written atomically (temp file + rename). */
export function fileDedupStorage(file: string): DedupStorage {
  return {
    read: () => {
      try {
        return readFileSync(file, "utf8");
      } catch (e) {
        if ((e as NodeJS.ErrnoException).code === "ENOENT") return undefined;
        throw e;
      }
    },
    write: (s) => {
      mkdirSync(dirname(file), { recursive: true });
      const tmp = `${file}.${process.pid}.tmp`;
      writeFileSync(tmp, s);
      renameSync(tmp, file);
    },
  };
}

const toEntryError = (e: unknown): Extract<Entry, { ok: false }>["error"] =>
  e instanceof WebError
    ? { web: true, code: e.code, message: e.message, ...(e.details === undefined ? {} : { details: e.details }) }
    : { web: false, message: e instanceof Error ? e.message : String(e) };

const fromEntryError = (e: Extract<Entry, { ok: false }>["error"]): Error =>
  e.web ? new WebError(e.code as never, e.message, e.details) : new Error(e.message);

/** Runs each key once; a repeat replays the stored value or error. Keeps the newest `cap` keys. */
export function createDedupStore(opts: { storage: DedupStorage; cap: number }): DedupStore {
  const raw = opts.storage.read();
  const entries = new Map<string, Entry>(raw === undefined ? [] : (JSON.parse(raw) as Array<[string, Entry]>));
  const pending = new Map<string, Promise<Entry>>();

  const remember = (key: string, entry: Entry): void => {
    entries.delete(key);
    entries.set(key, entry);
    while (entries.size > opts.cap) entries.delete(entries.keys().next().value as string);
    opts.storage.write(JSON.stringify([...entries]));
  };
  const replay = <T>(entry: Entry): T => {
    if (entry.ok) return entry.value as T;
    throw fromEntryError(entry.error);
  };

  return {
    forget(key) {
      if (entries.delete(key)) opts.storage.write(JSON.stringify([...entries]));
    },
    async run<T>(key: string, exec: () => Promise<T> | T): Promise<T> {
      const hit = entries.get(key);
      if (hit !== undefined) return replay<T>(hit);
      let inflight = pending.get(key);
      if (inflight === undefined) {
        inflight = (async (): Promise<Entry> => {
          let entry: Entry;
          try {
            entry = { ok: true, value: await exec() };
          } catch (e) {
            entry = { ok: false, error: toEntryError(e) };
          }
          remember(key, entry);
          return entry;
        })().finally(() => pending.delete(key));
        pending.set(key, inflight);
      }
      return replay<T>(await inflight);
    },
  };
}
