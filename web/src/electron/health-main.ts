import type { HealthRecorder } from "./health.ts";

type Listener = (...args: any[]) => void;

export interface AttachHealthDeps {
  recorder: HealthRecorder;
  webContents: { on(event: string, listener: Listener): unknown };
  ipcMain: { on(channel: string, listener: Listener): unknown };
  process: { on(signal: string, listener: Listener): unknown };
  app: { on(event: string, listener: Listener): unknown };
}

export function attachHealth(_deps: AttachHealthDeps): void {
  throw new Error("not implemented");
}
