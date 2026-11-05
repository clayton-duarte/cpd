import type { HealthRecord } from "./health.ts";
import type { RestorePoints } from "./restore-points.ts";

export interface WatchdogPorts {
  now(): number;
  readHealth(): Promise<HealthRecord | null>;
  pidAlive(pid: number): boolean;
  signalReload(pid: number): void;
  relaunch(): Promise<void>;
  restore: RestorePoints;
  log(entry: object): Promise<void>;
}

export interface WatchdogOptions {
  stabilizeMs?: number;
  crashDebounceMs?: number;
  loadingGraceMs?: number;
}

export interface Watchdog {
  tick(): Promise<void>;
}

export function createWatchdog(_ports: WatchdogPorts, _options?: WatchdogOptions): Watchdog {
  throw new Error("not implemented");
}
