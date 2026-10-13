import { mergeLockPath } from "../lib/paths.ts";
import { withLock } from "../lib/lock.ts";
import { failure, refused } from "../lib/errors.ts";
import { conflictedFiles, git, realpath, rebaseInProgress, revParse, tail } from "./git.ts";
import { GATE_FAILED_EXIT, runGate, tamperCheck, treeClean } from "./gate.ts";
import { MergeJson, mergeArgs, parseWtJson, runWt } from "./wt.ts";

export type MergeResult =
  | "merged"
  | "gate_failed"
  | "conflict"
  | "cas_lost"
  | "dirty_target"
  | "tampered"
  | "post_gate_failed"
  | "unknown";

export type MergeOutcome = {
  result: MergeResult;
  /** Lead Branch SHA before the (last) merge try. After any non-`merged` result the Lead is back at it. */
  preSha: string;
  /** Lead Branch SHA after a successful, post-Gate-verified merge. */
  postSha?: string;
  detail: string;
  /** Conflicting or tampered paths, when the result has any. */
  files?: string[];
};

export type MergeParams = {
  repo: string;
  leadWorktree: string;
  leadBranch: string;
  attemptWorktree: string;
  branch: string;
  /** Override for tests; defaults to MAX_CAS_RETRIES. */
  casRetries?: number;
};

/** Contract §12 step 11: a lost CAS is retried this many times (after the first try). */
export const MAX_CAS_RETRIES = 3;

/** `wt merge` prints this on stderr when the Lead checkout overlaps uncommitted changes (t3 F8; the only text dependency). */
const DIRTY_TARGET_TEXT = "conflicting uncommitted changes";

const leadRef = (p: MergeParams) => revParse(p.leadWorktree, `refs/heads/${p.leadBranch}`);

type Attempt = Omit<MergeOutcome, "preSha">;

/** One `wt merge` try and its classification (t3 "attempt merge"). Leaves no rebase open anywhere. */
async function tryMerge(p: MergeParams, pre: string): Promise<Attempt> {
  const r = await runWt(p.attemptWorktree, mergeArgs(p.leadBranch));
  if (r.code === 0) {
    parseWtJson(MergeJson, r, "merge");
    return { result: "merged", detail: "merged" };
  }
  if (r.code === GATE_FAILED_EXIT) {
    return { result: "gate_failed", detail: `Gate failed in the Attempt worktree:\n${tail(r.stderr + r.stdout)}` };
  }
  if (await rebaseInProgress(p.attemptWorktree)) {
    const files = await conflictedFiles(p.attemptWorktree);
    const abort = await git(p.attemptWorktree, ["rebase", "--abort"]);
    if (abort.code !== 0) {
      throw failure("rebase_abort_failed", "git rebase --abort failed after a merge conflict", {
        worktree: p.attemptWorktree,
        stderr: tail(abort.stderr),
      });
    }
    return { result: "conflict", detail: `rebase conflict in: ${files.join(", ")}`, files };
  }
  if (r.stderr.includes(DIRTY_TARGET_TEXT)) {
    return { result: "dirty_target", detail: `Lead worktree has conflicting uncommitted changes:\n${tail(r.stderr)}` };
  }
  if ((await leadRef(p)) !== pre) {
    return { result: "cas_lost", detail: "the Lead Branch moved during the merge" };
  }
  return {
    result: "unknown",
    detail: `wt merge exited ${r.code ?? r.signal}: ${tail(r.stderr)}`,
  };
}

/** Post-merge Gate in the Lead worktree; roll the Lead back with `reset --keep` unless it passes. */
async function postGate(p: MergeParams, pre: string): Promise<Attempt> {
  const rollback = async () => {
    const r = await git(p.leadWorktree, ["reset", "--keep", pre]);
    if (r.code !== 0) {
      throw failure("rollback_failed", `git reset --keep ${pre} failed in the Lead worktree`, {
        leadWorktree: p.leadWorktree,
        stderr: tail(r.stderr),
      });
    }
  };
  let gate;
  try {
    gate = await runGate(p.leadWorktree);
  } catch (e) {
    await rollback();
    throw e;
  }
  if (!gate.passed) {
    await rollback();
    return { result: "post_gate_failed", detail: `post-merge Gate failed; Lead rolled back to ${pre}:\n${gate.outputTail}` };
  }
  return { result: "merged", detail: "merged" };
}

/**
 * t3 "attempt merge", serialised per repo by the merge lock. The Lead Branch is never left
 * moved or half-merged unless the result is `merged`.
 */
export async function mergeAttempt(p: MergeParams): Promise<MergeOutcome> {
  const lock = mergeLockPath(realpath(p.repo));
  return withLock(lock, `merge ${p.branch}`, async () => {
    let pre = await leadRef(p);
    if (!(await treeClean(p.leadWorktree))) {
      return { result: "dirty_target", preSha: pre, detail: "Lead worktree has uncommitted changes" };
    }
    if (!(await treeClean(p.attemptWorktree))) {
      throw refused("attempt_dirty", "Attempt worktree has uncommitted changes", { worktree: p.attemptWorktree });
    }
    const tampered = await tamperCheck(p.attemptWorktree, p.leadBranch, p.branch);
    if (tampered.length > 0) {
      return { result: "tampered", preSha: pre, detail: `Attempt touches forbidden paths: ${tampered.join(", ")}`, files: tampered };
    }

    for (let retries = 0; ; retries++) {
      pre = await leadRef(p);
      let out = await tryMerge(p, pre);
      if (out.result === "merged") out = await postGate(p, pre);
      if (out.result === "cas_lost" && retries < (p.casRetries ?? MAX_CAS_RETRIES)) continue;
      if (out.result === "merged") return { ...out, preSha: pre, postSha: await leadRef(p) };
      return { ...out, preSha: pre };
    }
  });
}
