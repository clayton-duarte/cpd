import type { CrashReport } from "./health.ts";

export interface CrashTarget {
  addEventListener(type: string, listener: (event: any) => void): void;
  removeEventListener(type: string, listener: (event: any) => void): void;
}

export function installCrashSignals(
  _target: CrashTarget,
  _onCrash: (r: CrashReport) => void,
): { errors(): number; dispose(): void } {
  throw new Error("not implemented");
}
