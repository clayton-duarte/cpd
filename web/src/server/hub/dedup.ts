export interface DedupStorage {
  read(): string | undefined;
  write(s: string): void;
}

export interface DedupStore {
  run<T>(key: string, exec: () => Promise<T> | T): Promise<T>;
}

// Stub: not implemented yet. Runs exec every time, stores nothing.
export function createDedupStore(_opts: { storage: DedupStorage; cap: number }): DedupStore {
  return { run: async (_key, exec) => exec() };
}
