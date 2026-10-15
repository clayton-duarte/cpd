import { failure } from "../lib/errors.ts";
import { diffNames, gitOut, statusPorcelain } from "./git.ts";
import { hookArgs, runWt, type Hook } from "./wt.ts";

/** Paths a Worker must never change: they define or run the Gate (t3 F11). */
export const FORBIDDEN_PATHS: readonly string[] = [".config/wt.toml", "verify.sh", ".cpd/"];

const isForbidden = (p: string): boolean => FORBIDDEN_PATHS.some((f) => (f.endsWith("/") ? p.startsWith(f) : p === f));

/**
 * Forbidden paths touched by `branch` relative to `base` (three-dot diff). `--no-renames` makes a
 * rename show as delete + add, so renaming a forbidden file away still reports its old path.
 */
export async function tamperCheck(worktree: string, base: string, branch: string): Promise<string[]> {
  return (await diffNames(worktree, `${base}...${branch}`, ["--no-renames"])).filter(isForbidden);
}

export type GateResult = { passed: boolean; exitCode: number; outputTail: string };

/** Exit code of the Gate wrapper `./verify.sh || exit 64` when the Gate itself failed. */
export const GATE_FAILED_EXIT = 64;
const TAIL_LINES = 60;

/** The code of the `failure` when the hook exits with anything but 0 or 64. */
const ERROR_CODE: Record<Hook, string> = { "pre-merge": "gate_error", "pre-commit": "check_error" };

/**
 * `wt hook <hook>` in `worktree`: `pre-merge` is the Gate (Workflow level), `pre-commit` the quick check (at submit).
 * 0 passed (also when the repo has no such hook table), 64 failed, anything else is `gate_error` / `check_error`.
 */
export async function runGate(worktree: string, hook: Hook = "pre-merge"): Promise<GateResult> {
  const r = await runWt(worktree, hookArgs(hook));
  const outputTail = (r.stdout + r.stderr).split("\n").slice(-TAIL_LINES).join("\n").trimEnd();
  if (r.code === 0) return { passed: true, exitCode: 0, outputTail };
  if (r.code === GATE_FAILED_EXIT) return { passed: false, exitCode: GATE_FAILED_EXIT, outputTail };
  throw failure(ERROR_CODE[hook], `wt hook ${hook} exited ${r.code ?? r.signal}, which is neither pass (0) nor failure (${GATE_FAILED_EXIT})`, {
    worktree,
    code: r.code,
    signal: r.signal,
    outputTail,
  });
}

export async function treeClean(worktree: string): Promise<boolean> {
  return (await statusPorcelain(worktree)).length === 0;
}

/** Commits on HEAD that are not on `base`. */
export async function commitsAhead(worktree: string, base: string): Promise<number> {
  const out = await gitOut(worktree, ["rev-list", "--count", `${base}..HEAD`]);
  return Number(out);
}
