import { CHANNELS } from "./channels.ts";
import type { CrashReport } from "./health.ts";

export interface HealthBridge {
  reportHealthy(startup?: Promise<unknown>): void;
  reportCrash(info: CrashReport): void;
}

function startupCrash(error: unknown): CrashReport {
  return {
    source: "unhandledrejection",
    error: error instanceof Error ? error.message : String(error),
  };
}

export function createHealthBridge(ipc: { send(channel: string, ...args: unknown[]): void }): HealthBridge {
  return {
    // D2: a renderer only counts as healthy once its startup IPC has resolved. Given that promise,
    // report health:ok on fulfilment and health:crash on rejection so the watchdog rolls back.
    reportHealthy: (startup) => {
      if (startup === undefined) {
        ipc.send(CHANNELS.healthOk);
        return;
      }
      startup.then(
        () => ipc.send(CHANNELS.healthOk),
        (error: unknown) => ipc.send(CHANNELS.healthCrash, startupCrash(error)),
      );
    },
    reportCrash: (info) => ipc.send(CHANNELS.healthCrash, info),
  };
}
