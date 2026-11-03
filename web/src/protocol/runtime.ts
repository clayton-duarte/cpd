export interface Disposable {
  dispose(): void; // idempotent
}

export interface Scheduler {
  now(): number; // epoch milliseconds
  after(ms: number, run: () => void): Disposable; // one-shot; dispose cancels
}

export interface Transport {
  connect(): void; // begins ONE connection attempt; the outcome arrives via onStatus
  send(raw: unknown): void; // throws WebError("internal") when not open
  close(): void; // idempotent; after close no status or message event fires and connect() is a no-op
  onMessage(handler: (raw: unknown) => void): Disposable;
  onStatus(handler: (status: "open" | "closed") => void): Disposable;
}
