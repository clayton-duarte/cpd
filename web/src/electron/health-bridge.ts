import { CHANNELS } from "./channels.ts";
import type { CrashReport } from "./health.ts";

export interface HealthBridge {
  reportHealthy(): void;
  reportCrash(info: CrashReport): void;
}

export function createHealthBridge(ipc: { send(channel: string, ...args: unknown[]): void }): HealthBridge {
  return {
    reportHealthy: () => ipc.send(CHANNELS.healthOk),
    reportCrash: (info) => ipc.send(CHANNELS.healthCrash, info),
  };
}
