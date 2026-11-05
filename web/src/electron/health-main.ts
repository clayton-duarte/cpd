import { CHANNELS } from "./channels.ts";
import type { CrashReport, HealthRecorder } from "./health.ts";

type Listener = (...args: any[]) => void;

export interface AttachHealthDeps {
  recorder: HealthRecorder;
  webContents: { on(event: string, listener: Listener): unknown };
  ipcMain: { on(channel: string, listener: Listener): unknown };
  process: { on(signal: string, listener: Listener): unknown };
  app: { on(event: string, listener: Listener): unknown };
}

const SOURCES = ["error-boundary", "window-error", "unhandledrejection"];

function isCrashReport(v: unknown): v is CrashReport {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o["source"] === "string" &&
    SOURCES.includes(o["source"]) &&
    typeof o["error"] === "string" &&
    (o["stack"] === undefined || typeof o["stack"] === "string")
  );
}

export function attachHealth(deps: AttachHealthDeps): void {
  const { recorder } = deps;
  const run = (job: Promise<void>): void => {
    job.catch((error: unknown) => console.error(error));
  };
  run(recorder.begin());
  deps.webContents.on("did-start-loading", () => run(recorder.begin()));
  deps.process.on("SIGUSR1", () => run(recorder.begin()));
  deps.ipcMain.on(CHANNELS.healthOk, () => run(recorder.healthy()));
  deps.ipcMain.on(CHANNELS.healthCrash, (_event: unknown, report: unknown) => {
    if (!isCrashReport(report)) {
      console.error("ignoring malformed health:crash payload", report);
      return;
    }
    run(recorder.crashed(report.source, report.error, report.stack));
  });
  deps.webContents.on("render-process-gone", (_event: unknown, details: { reason: string }) => {
    if (details.reason === "clean-exit") return;
    run(recorder.crashed("render-process-gone", details.reason));
  });
  deps.webContents.on(
    "did-fail-load",
    (_event: unknown, errorCode: number, errorDescription: string, validatedURL: string, isMainFrame: boolean) => {
      if (!isMainFrame || errorCode === -3) return;
      run(recorder.crashed("did-fail-load", `${errorDescription} (${errorCode}) ${validatedURL}`));
    },
  );
  deps.app.on("before-quit", () => run(recorder.exited()));
}
