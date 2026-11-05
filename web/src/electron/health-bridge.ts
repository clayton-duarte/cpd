import type { CrashReport } from "./health.ts";

export interface HealthBridge {
  reportHealthy(): void;
  reportCrash(info: CrashReport): void;
}

export function createHealthBridge(_ipc: { send(channel: string, ...args: unknown[]): void }): HealthBridge {
  throw new Error("not implemented");
}
