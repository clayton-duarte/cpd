import type { PtySize } from "./channels.ts";

export interface PtyProcess {
  onData(cb: (data: string) => void): void;
  onExit(cb: (event: { exitCode: number }) => void): void;
  write(data: string): void;
  resize(cols: number, rows: number): void;
  kill(): void;
}
export interface PtySpawnOptions {
  name: string;
  cols: number;
  rows: number;
  cwd: string;
  env: Record<string, string>;
}
export interface PtyBackend {
  spawn(file: string, args: string[], options: PtySpawnOptions): PtyProcess;
}
export interface PtySinks {
  data(chunk: string): void;
  exit(exitCode: number): void;
}
export interface PtySession {
  write(data: string): void;
  resize(size: PtySize): void;
  kill(): void;
}
export const CPD_TUI_COMMAND = { file: "cpd", args: ["tui", "--attach"] } as const;
export interface StartPtyOptions {
  backend: PtyBackend;
  size: PtySize;
  cwd: string;
  env: Record<string, string | undefined>;
  sinks: PtySinks;
  command?: { file: string; args: readonly string[] };
}
export function startPty(_options: StartPtyOptions): PtySession {
  throw new Error("not implemented");
}
