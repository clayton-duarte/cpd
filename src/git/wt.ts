import { z } from "zod";
import { run, type RunResult } from "../lib/exec.ts";
import { failure } from "../lib/errors.ts";
import { tail, toolEnv } from "./git.ts";

/** Every `wt` call: `--yes` (global, before the subcommand), `-C <dir>`, then the subcommand. Stdin is /dev/null. */
export const wtArgs = (dir: string, sub: string[]): string[] => ["--yes", "-C", dir, ...sub];

export const switchCreateArgs = (branch: string, base: string): string[] => [
  "switch", "--create", branch, "--base", base, "--no-cd", "--format", "json",
];
export const switchExistingArgs = (branch: string): string[] => ["switch", branch, "--no-cd", "--format", "json"];
export const mergeArgs = (target: string): string[] => ["merge", target, "--no-squash", "--no-remove", "--format", "json"];
export const hookPreMergeArgs = (): string[] => ["hook", "pre-merge"];
export const removeArgs = (branch: string, opts: { forceDelete?: boolean; force?: boolean } = {}): string[] => [
  "remove", branch, ...(opts.forceDelete ? ["-D"] : []), ...(opts.force ? ["-f"] : []), "--foreground", "--format", "json",
];
export const stepRebaseArgs = (target: string): string[] => ["step", "rebase", target];

export function runWt(dir: string, sub: string[]): Promise<RunResult> {
  return run("wt", wtArgs(dir, sub), { cwd: dir, env: toolEnv() });
}

export const SwitchJson = z.looseObject({ action: z.string(), branch: z.string(), path: z.string().min(1) });
export type SwitchResult = z.infer<typeof SwitchJson>;
/** `wt merge --format json` prints booleans only. */
export const MergeJson = z.looseObject({ branch: z.string(), target: z.string(), rebased: z.boolean() });
export type MergeJsonResult = z.infer<typeof MergeJson>;

/** Parse the stdout of a finished `wt` call; the exit code was already checked by the caller. */
export function parseWtJson<S extends z.ZodType>(schema: S, r: RunResult, what: string): z.infer<S> {
  const details = { code: r.code, stdout: tail(r.stdout), stderr: tail(r.stderr) };
  let raw: unknown;
  try {
    raw = JSON.parse(r.stdout);
  } catch {
    throw failure("bad_tool_output", `wt ${what} printed invalid JSON`, details);
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw failure("bad_tool_output", `wt ${what} output does not match the expected shape`, {
      ...details,
      issues: parsed.error.issues,
    });
  }
  return parsed.data;
}

/** Run a `wt switch` form, require exit 0 and parse its JSON. */
export async function wtSwitch(dir: string, sub: string[]): Promise<SwitchResult> {
  const r = await runWt(dir, sub);
  if (r.code !== 0) {
    throw failure("wt_failed", `wt switch exited ${r.code ?? r.signal}`, { dir, code: r.code, stderr: tail(r.stderr) });
  }
  return parseWtJson(SwitchJson, r, "switch");
}
