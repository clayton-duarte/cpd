import type { CrashReport } from "./health.ts";

export interface CrashTarget {
  addEventListener(type: string, listener: (event: any) => void): void;
  removeEventListener(type: string, listener: (event: any) => void): void;
}

const describe = (v: unknown): string => (v instanceof Error ? v.message : String(v));

export function installCrashSignals(
  target: CrashTarget,
  onCrash: (r: CrashReport) => void,
): { errors(): number; dispose(): void } {
  let count = 0;
  const onError = (event: { message?: string; error?: unknown }): void => {
    count++;
    const err = event.error;
    onCrash({
      source: "window-error",
      error: err instanceof Error ? err.message : (event.message ?? String(err)),
      ...(err instanceof Error && err.stack !== undefined ? { stack: err.stack } : {}),
    });
  };
  const onRejection = (event: { reason?: unknown }): void => {
    count++;
    const r = event.reason;
    onCrash({
      source: "unhandledrejection",
      error: describe(r),
      ...(r instanceof Error && r.stack !== undefined ? { stack: r.stack } : {}),
    });
  };
  target.addEventListener("error", onError);
  target.addEventListener("unhandledrejection", onRejection);
  return {
    errors: () => count,
    dispose() {
      target.removeEventListener("error", onError);
      target.removeEventListener("unhandledrejection", onRejection);
    },
  };
}
