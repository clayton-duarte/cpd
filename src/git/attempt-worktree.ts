import { CpdError, failure, refused } from "../lib/errors.ts";
import { branchExists, git, gitOut, mergeBase, realpath, revParse, tail } from "./git.ts";
import { removeArgs, runWt, switchCreateArgs, parseWtJson, SwitchJson } from "./wt.ts";

/** t3 F1: a failed `pre-start` leaves the half-created worktree and branch; remove both. */
async function cleanupFailedCreate(leadWorktree: string, branch: string): Promise<{ ok: boolean; stderr: string }> {
  const r = await runWt(leadWorktree, removeArgs(branch, { forceDelete: true, force: true }));
  const leftover = await branchExists(leadWorktree, branch);
  return { ok: r.code === 0 && !leftover, stderr: tail(r.stderr) };
}

/**
 * Contract §12 step 2. Blocks while setup (`pre-start`) runs, then asserts the Attempt's
 * merge-base with the Lead Branch is the Lead HEAD. Any failure cleans up and throws `setup_failed`.
 */
export async function createAttemptWorktree(
  leadWorktree: string,
  branch: string,
  leadBranch: string,
): Promise<{ path: string }> {
  // `wt remove -D` below must never delete a branch we did not create.
  if (await branchExists(leadWorktree, branch)) {
    throw refused("attempt_branch_exists", `branch ${branch} already exists`, { branch });
  }
  const leadHead = await revParse(leadWorktree, `refs/heads/${leadBranch}`);
  const r = await runWt(leadWorktree, switchCreateArgs(branch, leadBranch));
  const fail = async (message: string, extra: Record<string, unknown>): Promise<CpdError> => {
    const cleanup = await cleanupFailedCreate(leadWorktree, branch);
    return failure("setup_failed", message, { branch, ...extra, cleanup });
  };
  if (r.code !== 0) {
    throw await fail(`attempt worktree setup failed (exit ${r.code ?? r.signal})`, {
      exitCode: r.code,
      signal: r.signal,
      stderrTail: tail(r.stderr),
    });
  }
  let path: string;
  try {
    path = realpath(parseWtJson(SwitchJson, r, "switch").path);
    const base = await mergeBase(path, leadBranch, branch);
    if (base !== leadHead) {
      throw failure("bad_base", `merge-base ${base} is not the Lead HEAD ${leadHead}`, { base, leadHead });
    }
  } catch (e) {
    throw await fail(`attempt worktree failed verification: ${(e as Error).message}`, { exitCode: 0, stderrTail: tail(r.stderr) });
  }
  return { path };
}

/**
 * t3 "attempt remove": remove the worktree, then the guarded `git branch -d` (merged into the
 * Lead's HEAD). Idempotent: missing worktree or branch are skipped. Closing herdr is not done here.
 */
export async function removeAttemptWorktree(repo: string, leadWorktree: string, branch: string): Promise<void> {
  const list = await gitOut(repo, ["worktree", "list", "--porcelain"]);
  const hasWorktree = list.split("\n").includes(`branch refs/heads/${branch}`);
  if (hasWorktree) {
    const r = await runWt(repo, removeArgs(branch));
    if (r.code !== 0) {
      throw failure("wt_failed", `wt remove exited ${r.code ?? r.signal}`, { branch, code: r.code, stderr: tail(r.stderr) });
    }
  }
  if (await branchExists(leadWorktree, branch)) {
    const r = await git(leadWorktree, ["branch", "-d", branch]);
    if (r.code !== 0) {
      throw refused("branch_not_merged", `cannot delete branch ${branch}: ${r.stderr.trim()}`, { branch, stderr: tail(r.stderr) });
    }
  }
}
