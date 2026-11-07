import { tuiAttachCommand } from "../server/cpd-argv.ts";
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
export const CPD_TUI_COMMAND = tuiAttachCommand();
export interface StartPtyOptions {
  backend: PtyBackend;
  size: PtySize;
  cwd: string;
  env: Record<string, string | undefined>;
  sinks: PtySinks;
  command?: { file: string; args: readonly string[] };
}
export function startPty(options: StartPtyOptions): PtySession {
  const { backend, size, cwd, sinks } = options;
  const command = options.command ?? CPD_TUI_COMMAND;
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(options.env)) {
    if (value !== undefined) env[key] = value;
  }
  env["TERM"] = "xterm-256color";
  env["COLORTERM"] = "truecolor";
  let proc: PtyProcess;
  try {
    proc = backend.spawn(command.file, [...command.args], {
      name: "xterm-256color",
      cols: size.cols,
      rows: size.rows,
      cwd,
      env,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    sinks.data("failed to start cpd tui --attach: " + message + "\r\n");
    sinks.exit(1);
    return { write() {}, resize() {}, kill() {} };
  }
  let alive = true;
  let killed = false;
  proc.onData((chunk) => {
    sinks.data(chunk);
  });
  proc.onExit((event) => {
    if (!alive) return;
    alive = false;
    sinks.exit(event.exitCode);
  });
  return {
    write(data) {
      if (alive) proc.write(data);
    },
    resize(next) {
      if (alive) proc.resize(next.cols, next.rows);
    },
    kill() {
      if (alive && !killed) {
        killed = true;
        proc.kill();
      }
    },
  };
}
