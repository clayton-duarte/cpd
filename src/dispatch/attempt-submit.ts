import { realpathSync } from "node:fs";
import { sep } from "node:path";
import { commitsAhead, runGate, tamperCheck, treeClean } from "../git/gate.ts";
import { gitOut, revParse, statusPorcelain } from "../git/git.ts";
import { refused } from "../lib/errors.ts";
import type { Config } from "../lib/config.ts";
import { attemptBranch, reviewGatePath } from "../lib/paths.ts";
import { writeTextAtomic } from "../render/template.ts";

// Contract §11 `cpd attempt submit`: the Worker's only way to finish. Every precondition refuses with exit 3 and a
// code the Worker can act on (the Worker Spec lists them); the first failing one wins, in the order below.
// The last one is the quick check (`wt hook pre-commit`), not the Gate: the Gate runs at the Workflow level (`pr open`).

export const SUBMIT_NEXT = "Stop now. Do not make more changes.";

export type SubmitDeps = {
  wf: string;
  /** The Attempt as the Ledger has it. */
  attempt: { id: string; status: string; worktree: string | null };
  leadBranch: string;
  /** With `none` there is no Reviewer, so no review file is written. */
  reviewMode: Config["reviewMode"];
  /** Records `submitted_at` and provenance `commit`, and moves the Attempt to `in_review`. */
  markSubmitted(commitSha: string): Promise<unknown>;
};

const canon = (p: string): string => {
  try {
    return realpathSync.native(p);
  } catch {
    return p;
  }
};

function insideWorktree(cwd: string, worktree: string): boolean {
  const root = canon(worktree);
  const here = canon(cwd);
  return here === root || here.startsWith(root.endsWith(sep) ? root : root + sep);
}

/** Run the §11 preconditions in `cwd`, write the quick-check output for the Reviewer (unless there is none), then record the submission. */
export async function submitAttempt(cwd: string, d: SubmitDeps): Promise<{ commit: string; next: string }> {
  const { attempt } = d;
  if (attempt.status !== "in_progress") {
    throw refused("illegal_transition", `Attempt ${attempt.id} is ${attempt.status}; only a running Attempt can be submitted`, {
      attempt: attempt.id,
      status: attempt.status,
    });
  }
  const worktree = attempt.worktree;
  if (worktree === null || !insideWorktree(cwd, worktree)) {
    throw refused("wrong_worktree", `run this from inside the Attempt worktree${worktree === null ? "" : ` (${worktree})`}, not ${cwd}`, {
      cwd,
      worktree,
    });
  }
  const branch = attemptBranch(d.wf, attempt.id);
  // The Gate, the diff and the merge all read the Attempt branch: a worktree on another branch would bypass them.
  const onBranch = await gitOut(worktree, ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (onBranch !== branch) {
    throw refused("wrong_worktree", `the worktree must stay on the Attempt branch ${branch}, but is on ${onBranch}`, { expected: branch, actual: onBranch });
  }

  if (!(await treeClean(worktree))) {
    throw refused("dirty_tree", "the worktree has uncommitted or untracked changes: commit them (or delete files that do not belong)", {
      status: (await statusPorcelain(worktree)).slice(0, 50),
    });
  }
  const ahead = await commitsAhead(worktree, d.leadBranch);
  if (ahead < 1) {
    throw refused("no_commits", `the Attempt has no commit on top of ${d.leadBranch}: commit your work`, { leadBranch: d.leadBranch });
  }
  const tampered = await tamperCheck(worktree, d.leadBranch, branch);
  if (tampered.length > 0) {
    throw refused("gate_tampered", `the diff touches files that define the Gate: ${tampered.join(", ")}; revert them to the base version`, { files: tampered });
  }
  const check = await runGate(worktree, "pre-commit");
  if (!check.passed) {
    throw refused("check_failed", `the quick check (hook pre-commit) failed (exit ${check.exitCode}); fix the cause, commit and submit again`, {
      exitCode: check.exitCode,
      outputTail: check.outputTail,
    });
  }

  if (d.reviewMode !== "none") writeTextAtomic(reviewGatePath(d.wf, attempt.id), `${check.outputTail}\n`);
  const commit = await revParse(worktree, "HEAD");
  await d.markSubmitted(commit);
  return { commit, next: SUBMIT_NEXT };
}
