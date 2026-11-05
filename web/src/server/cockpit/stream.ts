import type { CpdEvent } from "./events.ts";

export interface StreamChild {
  write(line: string): void;
  endInput(): void;
  kill(): void;
  onLine(cb: (line: string) => void): void;
  onExit(cb: (info: { code: number | null; stderr: string }) => void): void;
}
export interface StreamPanes { readonly context: unknown; readonly plan: unknown; readonly session: unknown; readonly inbox: unknown }
export interface StreamSnapshot { readonly seq: number; readonly panes: StreamPanes }
export interface StreamDeps {
  spawn(args: string[]): StreamChild;
  scheduler: import("../../protocol/runtime.ts").Scheduler;
}
export interface CockpitStream {
  start(): void;
  stop(): void;
  ready(): boolean;
  onSnapshot(cb: (s: StreamSnapshot) => void): void;
  onEvent(cb: (e: CpdEvent) => void): void;
  onError(cb: (e: Error) => void): void;
  /** Resolves the `value` of the plan_show frame; rejects WebError cpd_failed on an error frame; resolves "down" when not ready. */
  planShow(id: string): Promise<unknown>;
}

export function createCockpitStream(_deps: StreamDeps): CockpitStream {
  throw new Error("not implemented");
}
