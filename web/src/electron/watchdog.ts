import type { HealthRecord } from "./health.ts";
import { HealthParseError } from "./health.ts";
import type { RestorePoints } from "./restore-points.ts";

export interface WatchdogPorts {
  now(): number;
  readHealth(): Promise<HealthRecord | null>;
  pidAlive(pid: number): boolean;
  signalReload(pid: number): void;
  terminate(pid: number): void;
  relaunch(): Promise<void>;
  restore: RestorePoints;
  log(entry: object): Promise<void>;
}

export function productionEnv(_env: Record<string, string | undefined>): Record<string, string | undefined> {
  throw new Error("not implemented");
}

export interface WatchdogOptions {
  stabilizeMs?: number;
  crashDebounceMs?: number;
  loadingGraceMs?: number;
}

export interface Watchdog {
  tick(): Promise<void>;
}

export function createWatchdog(ports: WatchdogPorts, options: WatchdogOptions = {}): Watchdog {
  const stabilizeMs = options.stabilizeMs ?? 3000;
  const crashDebounceMs = options.crashDebounceMs ?? 1000;
  const loadingGraceMs = options.loadingGraceMs ?? 10000;

  let seenEpoch: number | null = null;
  let crashSeenAt: number | null = null;
  let healthySeenAt: number | null = null;
  let candidatePending = false;
  let actedEpoch: number | null = null;
  let rolledBack = false;
  let lastMalformed: string | null = null;

  const log = (entry: Record<string, unknown>) =>
    ports.log({ at: new Date(ports.now()).toISOString(), ...entry });

  function trigger(rec: HealthRecord, now: number): string | null {
    if (rec.status === "exited") return null;
    if (rec.status === "crashed" && crashSeenAt !== null && now - crashSeenAt >= crashDebounceMs) {
      return `crashed: ${rec.source}`;
    }
    if (rec.status === "loading" && now - Date.parse(rec.startedAt) >= loadingGraceMs) {
      return "loading too long";
    }
    if (!ports.pidAlive(rec.pid)) return "pid dead";
    return null;
  }

  async function rollback(rec: HealthRecord, reason: string) {
    const base = { reason, reloadEpoch: rec.reloadEpoch, pid: rec.pid };
    if (rolledBack) {
      await log({ event: "rollback-skipped", ...base, reason: "already rolled back" });
      return;
    }
    if (!(await ports.restore.hasLastGood())) {
      await log({ event: "rollback-skipped", ...base, reason: `no restore point (${reason})` });
      return;
    }
    await log({ event: "rollback", ...base });
    try {
      await ports.restore.restoreLastGood();
      rolledBack = true;
      if (ports.pidAlive(rec.pid)) {
        ports.signalReload(rec.pid);
      } else {
        await log({ event: "relaunch", ...base });
        await ports.relaunch();
      }
    } catch (error) {
      await log({ event: "rollback-failed", ...base, error: (error as Error).message });
    }
  }

  async function observe(rec: HealthRecord) {
    const now = ports.now();
    if (rec.reloadEpoch !== seenEpoch) {
      seenEpoch = rec.reloadEpoch;
      crashSeenAt = null;
      healthySeenAt = null;
      await ports.restore.captureCandidate();
      candidatePending = true;
    }
    if (rec.status === "crashed" && crashSeenAt === null) crashSeenAt = now;
    if (rec.status === "healthy" && healthySeenAt === null) healthySeenAt = now;

    if (rec.status === "healthy" && candidatePending && healthySeenAt !== null && now - healthySeenAt >= stabilizeMs) {
      candidatePending = false;
      await ports.restore.promoteCandidate();
      rolledBack = false;
    }

    if (actedEpoch === rec.reloadEpoch) return;
    const reason = trigger(rec, now);
    if (reason === null) return;
    actedEpoch = rec.reloadEpoch;
    await rollback(rec, reason);
  }

  return {
    async tick() {
      let rec: HealthRecord | null;
      try {
        rec = await ports.readHealth();
      } catch (error) {
        if (!(error instanceof HealthParseError)) throw error;
        if (error.message !== lastMalformed) {
          lastMalformed = error.message;
          await log({ event: "health-malformed", error: error.message });
        }
        return;
      }
      lastMalformed = null;
      if (rec === null) return;
      await observe(rec);
    },
  };
}
