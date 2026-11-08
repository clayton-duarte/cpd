import type { UiMode } from "./channels.ts";
import type { CockpitHealth, HealthLimits } from "./health.ts";
import { DEFAULT_HEALTH_LIMITS } from "./health.ts";
import type { HealthRecord } from "./health.ts";
import { HealthParseError } from "./health.ts";
import type { RestorePoints } from "./restore-points.ts";

export interface WatchdogPorts {
  now(): number;
  readHealth(): Promise<HealthRecord | null>;
  pidAlive(pid: number): boolean;
  /** Unused by the watchdog since rollback relaunches in production; kept optional for existing callers. */
  signalReload?(pid: number): void;
  /** Resolves once the process is gone. */
  terminate(pid: number): void | Promise<void>;
  relaunch(): Promise<void>;
  restore: RestorePoints;
  log(entry: object): Promise<void>;
}

export function productionEnv(env: Record<string, string | undefined>): Record<string, string | undefined> {
  const { CPD_ELECTRON_DEV_URL: _devUrl, ...rest } = env;
  return { ...rest, CPD_MODE: "production" };
}

export interface WatchdogOptions {
  stabilizeMs?: number;
  crashDebounceMs?: number;
  loadingGraceMs?: number;
}

export interface Watchdog {
  tick(): Promise<void>;
}

/** The in-place preview's rollback: the generic watchdog, but the rollback returns to the production WebUI. */
export interface PreviewWatchdogPorts {
  now(): number;
  readHealth(): Promise<HealthRecord | null>;
  pidAlive(pid: number): boolean;
  setMode(mode: UiMode): Promise<void>;
  showErrorBox(title: string, content: string): void;
  log(entry: object): Promise<void>;
}

export function createPreviewWatchdog(ports: PreviewWatchdogPorts, options: WatchdogOptions = {}): Watchdog {
  const crashDebounceMs = options.crashDebounceMs ?? 1000;
  const loadingGraceMs = options.loadingGraceMs ?? 10000;

  let seenEpoch: number | null = null;
  let crashSeenAt: number | null = null;
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

  async function rollback(rec: HealthRecord, reason: string): Promise<void> {
    const base = { reason, reloadEpoch: rec.reloadEpoch, pid: rec.pid };
    if (rolledBack) {
      await log({ event: "rollback-skipped", ...base, reason: "already rolled back" });
      return;
    }
    await log({ event: "rollback", ...base });
    rolledBack = true;
    await ports.setMode("webui");
    ports.showErrorBox("WebUI Preview", ` ${reason}`);
  }

  async function observe(rec: HealthRecord): Promise<void> {
    const now = ports.now();
    if (rec.reloadEpoch !== seenEpoch) {
      seenEpoch = rec.reloadEpoch;
      crashSeenAt = null;
    }
    if (rec.status === "crashed" && crashSeenAt === null) crashSeenAt = now;

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
        await ports.terminate(rec.pid);
        await ports.relaunch();
      } else {
        await log({ event: "relaunch", ...base, mode: "production" });
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

/** A build kept on disk that the watchdog can revert to. */
export type Build = "preview" | "prod" | "head-1" | "head-2";

/** The revert ladder, in order (air-d95): preview > prod > HEAD-1 > HEAD-2. */
export const LADDER: readonly Build[] = ["preview", "prod", "head-1", "head-2"];

/**
 * A structured cpd Event (air-d81, docs/commands.md §11): the shape every Event carries.
 * A detection Event names the detection and the health record that fired it; a recovery Event
 * names the ladder rung taken, or the renderer-only reload.
 */
export interface WatchdogEvent {
  kind: "watchdog.break" | "watchdog.recovery";
  text: string;
  data: Record<string, unknown>;
}

export interface HealthWatchdogPorts {
  now(): number;
  /** The renderer's cockpit and transport report; null when it has written none. */
  readHealth(): Promise<CockpitHealth | null>;
  /** The build the view runs from now. */
  build(): Build;
  /** Swap to another build kept on disk and relaunch the app (a ladder step). */
  revert(build: Build): Promise<void>;
  /** Reload the renderer in place: the app process and the view stay (not a ladder step). */
  reloadRenderer(): Promise<void>;
  /** Commit one structured Event (air-d81). */
  report(event: WatchdogEvent): Promise<void>;
  log(entry: Record<string, unknown>): Promise<void>;
}

export interface HealthWatchdog {
  tick(): Promise<void>;
}

/**
 * The watchdog that reads the cockpit/transport report in every view mode (air-d88), reverts
 * along the ladder (air-d95) and commits an Event for every detection and every recovery
 * (air-d81). `cockpit not ready` is not a ladder step: the build on disk is not broken, so its
 * recovery is the renderer-only reload.
 */
export function createHealthWatchdog(
  ports: HealthWatchdogPorts,
  limits: HealthLimits = DEFAULT_HEALTH_LIMITS,
): HealthWatchdog {
  void ports;
  void limits;
  return {
    async tick() {
      // stub: the implementing change brings the code in line
    },
  };
}
