import { failure, refused } from "../lib/errors.ts";
import { git, rebaseInProgress, revParse, tail, conflictedFiles } from "./git.ts";
import { treeClean } from "./gate.ts";
import { runWt, stepRebaseArgs } from "./wt.ts";

export type RebaseOutcome = { result: "rebased" } | { result: "conflict"; files: string[] };

/** File names from `git merge-tree --write-tree --name-only` output: line 1 is the tree OID, then names up to a blank line. */
function mergeTreeConflicts(stdout: string): string[] {
  const lines = stdout.split("\n").slice(1);
  const end = lines.indexOf("");
  return (end === -1 ? lines : lines.slice(0, end)).filter(Boolean);
}

/**
 * t3 "pr rebase", clean case only. A `git merge-tree` check predicts conflicts without touching
 * the worktree; `wt step rebase` does the work and any failure is aborted at once, so the Lead
 * worktree is never left mid-rebase. Pushing is the caller's job.
 */
export async function rebaseLeadOntoBase(p: { leadWorktree: string; leadBranch: string; base: string }): Promise<RebaseOutcome> {
  if (!(await treeClean(p.leadWorktree))) {
    throw refused("dirty_lead", "Lead worktree has uncommitted changes", { leadWorktree: p.leadWorktree });
  }
  const before = await revParse(p.leadWorktree, `refs/heads/${p.leadBranch}`);
  const mt = await git(p.leadWorktree, ["merge-tree", "--write-tree", "--name-only", p.base, p.leadBranch]);
  if (mt.code === 1) return { result: "conflict", files: mergeTreeConflicts(mt.stdout) };
  if (mt.code !== 0) {
    throw failure("git_failed", `git merge-tree exited ${mt.code ?? mt.signal}`, { stderr: tail(mt.stderr) });
  }

  const r = await runWt(p.leadWorktree, stepRebaseArgs(p.base));
  if (r.code === 0) return { result: "rebased" };

  if (await rebaseInProgress(p.leadWorktree)) {
    const files = await conflictedFiles(p.leadWorktree);
    const abort = await git(p.leadWorktree, ["rebase", "--abort"]);
    if (abort.code !== 0) {
      throw failure("rebase_abort_failed", "git rebase --abort failed in the Lead worktree", { stderr: tail(abort.stderr) });
    }
    if ((await revParse(p.leadWorktree, `refs/heads/${p.leadBranch}`)) !== before) {
      throw failure("rebase_abort_failed", "Lead Branch moved after aborting the rebase", { before });
    }
    return { result: "conflict", files };
  }
  throw failure("wt_failed", `wt step rebase exited ${r.code ?? r.signal}`, { code: r.code, stderr: tail(r.stderr) });
}
