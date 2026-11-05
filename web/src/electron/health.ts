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

export function parseHealth(_text: string): HealthRecord {
  throw new Error("not implemented");
}

export function createHealthRecorder(_deps: HealthRecorderDeps): HealthRecorder {
  throw new Error("not implemented");
}
