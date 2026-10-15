import { git, tail } from "../git/git.ts";
import { rebaseLeadOntoBase } from "../git/pr-rebase.ts";
import { requireGate } from "../pr/open.ts";
import { logEvent } from "../ledger/audit.ts";
import { parseCommand, readWorkflow, resolveWorkflowId } from "../ledger/workflow.ts";
import { failure, refused } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

/** Contract §11 `pr rebase`: rebase the Lead Branch onto the base, then the Gate, then `git push --force-with-lease`. A conflict changes nothing; a Gate failure leaves the rebased Lead Branch unpushed. */
export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("pr rebase", argv, { wf: { type: "string" } });
  const wf = resolveWorkflowId(values.wf);
  const w = readWorkflow(wf);
  if (w.leadWorktree === null) throw refused("no_lead_worktree", `Workflow ${wf} has no Lead worktree`, { wf });

  const r = await rebaseLeadOntoBase({ leadWorktree: w.leadWorktree, leadBranch: w.leadBranch, base: w.base });
  if (r.result === "conflict") {
    throw refused("rebase_conflict", `rebasing ${w.leadBranch} onto ${w.base} conflicts; nothing was changed`, { files: r.files });
  }
  await requireGate(w.leadWorktree);
  const push = await git(w.leadWorktree, ["push", "--force-with-lease", "origin", w.leadBranch]);
  if (push.code !== 0) throw failure("git_push_failed", `git push exited ${push.code ?? push.signal}`, { stderr: tail(push.stderr) });
  logEvent(wf, "info", "pr_rebased", { leadBranch: w.leadBranch, base: w.base });
  return { ok: true, result: "rebased", leadBranch: w.leadBranch };
}
