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
