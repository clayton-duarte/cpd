import type { Disposable } from "../../protocol/runtime.ts";

export interface CpdResult { readonly stdout: string; readonly stderr: string; readonly exitCode: number }
export interface CpdExec { run(args: readonly string[]): Promise<CpdResult> }

export interface FollowProcess {
  onLine(handler: (line: string) => void): Disposable;
  onExit(handler: (info: { code: number | null; stderr: string }) => void): Disposable;
  kill(): void;
}
export interface FollowBoundary { start(args: readonly string[]): FollowProcess }

export interface Cpd {
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>>;
}
export function createCpd(_exec: CpdExec): Cpd {
  throw new Error("not implemented");
}
export function followArgs(_since: number): readonly string[] {
  throw new Error("not implemented");
}
