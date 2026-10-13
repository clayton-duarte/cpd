import { realpathSync } from "node:fs";
import { run, type RunResult } from "../lib/exec.ts";
import { failure } from "../lib/errors.ts";

/** Environment for every git/wt subprocess: inherited, but never able to prompt or open an editor. */
export const toolEnv = (): NodeJS.ProcessEnv => ({
  ...process.env,
  GIT_TERMINAL_PROMPT: "0",
  GIT_EDITOR: "true",
  GIT_SEQUENCE_EDITOR: "true",
});

export const tail = (s: string, n = 2000): string => (s.length > n ? s.slice(-n) : s);

/** Run `git -C <cwd> <args>`; stdin is /dev/null. Never throws on a non-zero exit. */
export function git(cwd: string, args: string[]): Promise<RunResult> {
  return run("git", ["-C", cwd, ...args], { cwd, env: toolEnv() });
}

/** Run git, require exit 0, return stdout without its trailing newline. */
export async function gitOut(cwd: string, args: string[]): Promise<string> {
  const r = await git(cwd, args);
  if (r.code !== 0) {
    throw failure("git_failed", `git ${args[0]} exited ${r.code ?? r.signal}`, {
      cwd,
      args,
      code: r.code,
      stderr: tail(r.stderr),
    });
  }
  return r.stdout.replace(/\n$/, "");
}

/** Full SHA of `ref` (must resolve to a commit). */
export const revParse = (cwd: string, ref: string): Promise<string> =>
  gitOut(cwd, ["rev-parse", "--verify", "--quiet", `${ref}^{commit}`]);

export const mergeBase = (cwd: string, a: string, b: string): Promise<string> => gitOut(cwd, ["merge-base", a, b]);

/** Paths changed by `range` (e.g. `base...branch`), NUL-delimited so no quoting. */
export async function diffNames(cwd: string, range: string, extra: string[] = []): Promise<string[]> {
  const out = await gitOut(cwd, ["diff", "--name-only", "-z", ...extra, range, "--"]);
  return out.split("\0").filter(Boolean);
}

/** `git status --porcelain` lines (ignored files excluded, untracked included). */
export async function statusPorcelain(cwd: string): Promise<string[]> {
  const out = await gitOut(cwd, ["status", "--porcelain"]);
  return out === "" ? [] : out.split("\n");
}

export const realpath = (p: string): string => realpathSync.native(p);

export async function branchExists(cwd: string, branch: string): Promise<boolean> {
  const r = await git(cwd, ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`]);
  if (r.code === 0) return true;
  if (r.code === 1) return false;
  throw failure("git_failed", `git show-ref exited ${r.code ?? r.signal}`, { cwd, branch, stderr: tail(r.stderr) });
}

/** True when `cwd` is mid-rebase (either backend). */
export async function rebaseInProgress(cwd: string): Promise<boolean> {
  for (const name of ["rebase-merge", "rebase-apply"]) {
    const p = await gitOut(cwd, ["rev-parse", "--path-format=absolute", "--git-path", name]);
    try {
      realpathSync.native(p);
      return true;
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== "ENOENT") throw e;
    }
  }
  return false;
}

/** Unmerged (conflicted) paths in the working tree. */
export async function conflictedFiles(cwd: string): Promise<string[]> {
  const out = await gitOut(cwd, ["diff", "--name-only", "-z", "--diff-filter=U"]);
  return out.split("\0").filter(Boolean);
}
