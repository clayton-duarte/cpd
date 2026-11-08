import type { ContextTreeModel } from "../protocol/cockpit.ts";
import { isStalled } from "./liveness.ts";

export type CrashSource =
  | "error-boundary"
  | "window-error"
  | "unhandledrejection"
  | "render-process-gone"
  | "did-fail-load";

export interface CrashReport {
  source: "error-boundary" | "window-error" | "unhandledrejection";
  error: string;
  stack?: string;
}

export type HealthRecord =
  | { status: "loading"; reloadEpoch: number; pid: number; startedAt: string }
  | { status: "healthy"; reloadEpoch: number; pid: number; startedAt: string; healthyAt: string }
  | {
      status: "crashed";
      reloadEpoch: number;
      pid: number;
      startedAt: string;
      crashedAt: string;
      source: CrashSource;
      error: string;
      stack?: string;
    }
  | { status: "exited"; reloadEpoch: number; pid: number; startedAt: string; exitedAt: string };

export class HealthParseError extends Error {}

export interface HealthFs {
  readFile(f: string): Promise<string>;
  writeFile(f: string, data: string): Promise<void>;
  rename(a: string, b: string): Promise<void>;
  appendFile(f: string, data: string): Promise<void>;
  mkdir(dir: string): Promise<void>;
}

export interface HealthRecorderDeps {
  file: string;
  crashLog: string;
  pid: number;
  now(): Date;
  fs: HealthFs;
}

export interface HealthRecorder {
  init(): Promise<void>;
  begin(): Promise<void>;
  healthy(): Promise<void>;
  crashed(source: CrashSource, error: string, stack?: string): Promise<void>;
  exited(): Promise<void>;
}

const SOURCES: readonly string[] = [
  "error-boundary",
  "window-error",
  "unhandledrejection",
  "render-process-gone",
  "did-fail-load",
];

export function parseHealth(text: string): HealthRecord {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (error) {
    throw new HealthParseError(`malformed health json: ${(error as Error).message}`);
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    throw new HealthParseError("health record is not an object");
  }
  const o = raw as Record<string, unknown>;
  const int = (k: string): number => {
    const v = o[k];
    if (typeof v !== "number" || !Number.isInteger(v) || v <= 0) throw new HealthParseError(`bad field ${k}`);
    return v;
  };
  const str = (k: string): string => {
    const v = o[k];
    if (typeof v !== "string") throw new HealthParseError(`bad field ${k}`);
    return v;
  };
  const base = { reloadEpoch: int("reloadEpoch"), pid: int("pid"), startedAt: str("startedAt") };
  switch (o["status"]) {
    case "loading":
      return { status: "loading", ...base };
    case "healthy":
      return { status: "healthy", ...base, healthyAt: str("healthyAt") };
    case "exited":
      return { status: "exited", ...base, exitedAt: str("exitedAt") };
    case "crashed": {
      const source = str("source");
      if (!SOURCES.includes(source)) throw new HealthParseError("bad field source");
      const stack = o["stack"];
      if (stack !== undefined && typeof stack !== "string") throw new HealthParseError("bad field stack");
      return {
        status: "crashed",
        ...base,
        crashedAt: str("crashedAt"),
        source: source as CrashSource,
        error: str("error"),
        ...(stack === undefined ? {} : { stack }),
      };
    }
    default:
      throw new HealthParseError("unknown status");
  }
}

export function createHealthRecorder(deps: HealthRecorderDeps): HealthRecorder {
  const { file, crashLog, pid, fs } = deps;
  let epoch = 0;
  let current: HealthRecord | undefined;
  let chain: Promise<void> = Promise.resolve();
  let tmpSeq = 0;

  const serial = (job: () => Promise<void>): Promise<void> => {
    const next = chain.then(job);
    chain = next.catch(() => undefined);
    return next;
  };

  const write = async (record: HealthRecord): Promise<void> => {
    current = record;
    await fs.mkdir(file.slice(0, Math.max(file.lastIndexOf("/"), 0)) || "/");
    const tmp = `${file}.${pid}.${tmpSeq++}.tmp`;
    await fs.writeFile(tmp, JSON.stringify(record));
    await fs.rename(tmp, file);
  };

  const startedAt = (): string => current?.startedAt ?? deps.now().toISOString();

  return {
    init: () =>
      serial(async () => {
        try {
          epoch = parseHealth(await fs.readFile(file)).reloadEpoch;
        } catch {
          epoch = 0;
        }
      }),
    begin: () =>
      serial(async () => {
        if (current?.status === "loading") return;
        epoch += 1;
        await write({ status: "loading", reloadEpoch: epoch, pid, startedAt: deps.now().toISOString() });
      }),
    healthy: () =>
      serial(async () => {
        if (current?.status === "crashed") return;
        await write({
          status: "healthy",
          reloadEpoch: epoch,
          pid,
          startedAt: startedAt(),
          healthyAt: deps.now().toISOString(),
        });
      }),
    crashed: (source, error, stack) =>
      serial(async () => {
        const at = deps.now().toISOString();
        const record: HealthRecord = {
          status: "crashed",
          reloadEpoch: epoch,
          pid,
          startedAt: startedAt(),
          crashedAt: at,
          source,
          error,
          ...(stack === undefined ? {} : { stack }),
        };
        await write(record);
        await fs.appendFile(
          crashLog,
          `${JSON.stringify({ at, reloadEpoch: epoch, pid, source, error, ...(stack === undefined ? {} : { stack }) })}\n`,
        );
      }),
    exited: () =>
      serial(async () => {
        await write({
          status: "exited",
          reloadEpoch: epoch,
          pid,
          startedAt: startedAt(),
          exitedAt: deps.now().toISOString(),
        });
      }),
  };
}

/**
 * The cockpit and transport signals the renderer reports beside its health record: what the
 * WebSocket delivered and what the cockpit painted (web/CONTRACT.md §15.2, web/ELECTRON.md
 * §Watchdog). A view that paints but never receives cockpit data is broken, so the watchdog
 * reads these to decide a break.
 */
export interface CockpitHealth {
  /** The renderer boot; every reload bumps it. One watchdog action per epoch. */
  epoch: number;
  /** When this renderer boot started (ms since the Unix epoch). */
  startedAt: number;
  /** The cockpit tree of the last `cockpit.snapshot`; null when no snapshot has arrived. */
  tree: ContextTreeModel | null;
  /** When the last `cockpit.snapshot` arrived (ms). */
  snapshotAt?: number;
  /** The `cockpit.error` message of the last error frame, e.g. "not ready". */
  error?: string;
  /** When the last `cockpit.error` arrived (ms). */
  errorAt?: number;
  /** When the last server heartbeat (`pong`) was seen on the WebSocket (ms). */
  heartbeatAt?: number;
}

/** The cockpit-side breaks the watchdog detects by health (air-d88). */
export type BreakKind = "empty-data" | "cockpit-not-ready" | "websocket-stalled";

export interface Break {
  kind: BreakKind;
  /** One line for the User, naming the signal that fired. */
  detail: string;
}

export interface HealthLimits {
  /** N: how long an empty tree, or `cockpit not ready`, may go without a later snapshot. */
  snapshotGraceMs: number;
  /** How long the WebSocket may go without a server heartbeat before it counts as stalled. */
  heartbeatTimeoutMs: number;
}

export const DEFAULT_HEALTH_LIMITS: HealthLimits = { snapshotGraceMs: 10000, heartbeatTimeoutMs: 5000 };

/**
 * Read the cockpit/transport report the renderer writes beside its health record
 * (`$CPD_HOME/run/cockpit-health.json`). Validation at the boundary: a malformed report raises
 * `HealthParseError`, so a bad write is loud, never a silent fallback.
 */
export function parseCockpitHealth(text: string): CockpitHealth {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (error) {
    throw new HealthParseError(`malformed cockpit health json: ${(error as Error).message}`);
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    throw new HealthParseError("cockpit health is not an object");
  }
  const o = raw as Record<string, unknown>;
  const num = (k: string): number => {
    const v = o[k];
    if (typeof v !== "number" || !Number.isFinite(v)) throw new HealthParseError(`bad field ${k}`);
    return v;
  };
  const optNum = (k: string): number | undefined => {
    const v = o[k];
    if (v === undefined) return undefined;
    if (typeof v !== "number" || !Number.isFinite(v)) throw new HealthParseError(`bad field ${k}`);
    return v;
  };
  const epoch = o["epoch"];
  if (typeof epoch !== "number" || !Number.isInteger(epoch) || epoch <= 0) {
    throw new HealthParseError("bad field epoch");
  }
  const tree = o["tree"];
  if (tree !== null && typeof tree !== "object") {
    throw new HealthParseError("bad field tree");
  }
  const error = o["error"];
  if (error !== undefined && typeof error !== "string") throw new HealthParseError("bad field error");
  const snapshotAt = optNum("snapshotAt");
  const errorAt = optNum("errorAt");
  const heartbeatAt = optNum("heartbeatAt");
  return {
    epoch,
    startedAt: num("startedAt"),
    tree: tree as ContextTreeModel | null,
    ...(snapshotAt === undefined ? {} : { snapshotAt }),
    ...(error === undefined ? {} : { error }),
    ...(errorAt === undefined ? {} : { errorAt }),
    ...(heartbeatAt === undefined ? {} : { heartbeatAt }),
  };
}

/**
 * The break the renderer's report shows, or null when the cockpit and its transport are fine.
 * Checked in order: `cockpit not ready` (the hub subscribe race, web/src/server/hub/connection.ts)
 * beats an empty tree, and a stalled WebSocket beats the loading grace.
 */
export function detectBreak(
  health: CockpitHealth,
  now: number,
  limits: HealthLimits = DEFAULT_HEALTH_LIMITS,
): Break | null {
  // `cockpit not ready` with no later snapshot: the hub's subscribe ran before the cockpit's
  // first load, sent the error and dropped the client, so nothing more comes on its own.
  const notReadyBaseline = health.errorAt ?? health.startedAt;
  const laterSnapshot = health.snapshotAt !== undefined && health.snapshotAt > notReadyBaseline;
  if (health.error === "not ready" && !laterSnapshot && now - notReadyBaseline >= limits.snapshotGraceMs) {
    return {
      kind: "cockpit-not-ready",
      detail: `cockpit.error "not ready" for ${now - notReadyBaseline} ms with no later snapshot`,
    };
  }
  // A stalled WebSocket beats the loading grace: the transport's heartbeat stopped, so no frame
  // can arrive whatever the cockpit painted.
  if (isStalled(health.heartbeatAt, now, limits.heartbeatTimeoutMs)) {
    return {
      kind: "websocket-stalled",
      detail: `no server heartbeat for ${now - (health.heartbeatAt as number)} ms`,
    };
  }
  // Empty data: the view painted but never received the first cockpit snapshot.
  if (health.tree === null && now - health.startedAt >= limits.snapshotGraceMs) {
    return { kind: "empty-data", detail: `no cockpit snapshot ${now - health.startedAt} ms after boot` };
  }
  return null;
}
