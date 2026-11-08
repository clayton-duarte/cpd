import { WebError } from "../../protocol/domain.ts";
import type { Disposable } from "../../protocol/runtime.ts";
import { toArgv } from "../cpd-argv.ts";

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
export function createCpd(exec: CpdExec): Cpd {
  return {
    async json(args) {
      let res: CpdResult;
      try {
        res = await exec.run(args);
      } catch (e) {
        throw new WebError("cpd_failed", e instanceof Error ? e.message : String(e), { cpdCode: "spawn_failed" });
      }
      const parsed = parseObject(res.stdout);
      if (res.exitCode !== 0) {
        const err = parsed?.["error"];
        if (parsed !== null && parsed["ok"] === false && isObject(err)) {
          const message = typeof err["message"] === "string" ? err["message"] : `cpd exited ${res.exitCode}`;
          throw new WebError("cpd_failed", message, { cpdCode: typeof err["code"] === "string" ? err["code"] : "unknown", exitCode: res.exitCode, details: err["details"] });
        }
        throw new WebError("cpd_failed", res.stderr.trim() || `cpd exited ${res.exitCode}`, { cpdCode: "no_json", exitCode: res.exitCode });
      }
      if (parsed === null || parsed["ok"] !== true) {
        throw new WebError("cpd_malformed_json", "cpd did not print an ok JSON object", { args });
      }
      return parsed;
    },
  };
}
function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}
function parseObject(text: string): Record<string, unknown> | null {
  try {
    const v: unknown = JSON.parse(text);
    return isObject(v) ? v : null;
  } catch {
    return null;
  }
}
export function followArgs(since: number): readonly string[] {
  return toArgv({ kind: "events.follow", since });
}
