import { failure } from "../lib/errors.ts";
import { diffNames, gitOut, statusPorcelain } from "./git.ts";
import { hookPreMergeArgs, runWt } from "./wt.ts";

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

/** `wt hook pre-merge` in `worktree`. 0 passed, 64 gate failed, anything else is `gate_error`. */
export async function runGate(worktree: string): Promise<GateResult> {
  const r = await runWt(worktree, hookPreMergeArgs());
  const outputTail = (r.stdout + r.stderr).split("\n").slice(-TAIL_LINES).join("\n").trimEnd();
  if (r.code === 0) return { passed: true, exitCode: 0, outputTail };
  if (r.code === GATE_FAILED_EXIT) return { passed: false, exitCode: GATE_FAILED_EXIT, outputTail };
  throw failure("gate_error", `Gate exited ${r.code ?? r.signal}, which is neither pass (0) nor gate failure (${GATE_FAILED_EXIT})`, {
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
